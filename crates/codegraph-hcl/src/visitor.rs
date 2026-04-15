// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting HCL/Terraform entities

use codegraph_parser_api::{
    truncate_body_prefix, CallRelation, FunctionEntity, ImportRelation, Parameter,
};
use tree_sitter::Node;

pub(crate) struct HclVisitor<'a> {
    pub source: &'a [u8],
    pub functions: Vec<FunctionEntity>,
    pub imports: Vec<ImportRelation>,
    pub calls: Vec<CallRelation>,
}

impl<'a> HclVisitor<'a> {
    pub fn new(source: &'a [u8]) -> Self {
        Self {
            source,
            functions: Vec::new(),
            imports: Vec::new(),
            calls: Vec::new(),
        }
    }

    fn node_text(&self, node: Node) -> String {
        node.utf8_text(self.source).unwrap_or("").to_string()
    }

    /// Extract the text content of a `string_lit` node (strips surrounding quotes).
    fn string_lit_value(&self, node: Node) -> String {
        let raw = self.node_text(node);
        // string_lit wraps in double-quotes: "value"
        raw.trim_matches('"').to_string()
    }

    pub fn visit_node(&mut self, node: Node) {
        match node.kind() {
            "block" => {
                self.visit_block(node);
            }
            _ => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.visit_node(child);
                }
            }
        }
    }

    fn visit_block(&mut self, node: Node) {
        // Collect children in order to determine block type and labels.
        // Structure: identifier [string_lit*] block_start body block_end
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();

        // First child is the block type identifier
        let block_type = children
            .first()
            .filter(|n| n.kind() == "identifier")
            .map(|n| self.node_text(*n))
            .unwrap_or_default();

        if block_type.is_empty() {
            return;
        }

        // Collect string_lit labels (come after the identifier, before block_start)
        let labels: Vec<String> = children
            .iter()
            .skip(1) // skip the type identifier
            .take_while(|n| n.kind() == "string_lit")
            .map(|n| self.string_lit_value(*n))
            .collect();

        // Find body node for body_prefix
        let body_node = children.iter().find(|n| n.kind() == "body");

        match block_type.as_str() {
            // resource "type" "name" { ... }  → function named "type.name"
            "resource" if labels.len() >= 2 => {
                let name = format!("{}.{}", labels[0], labels[1]);
                let signature = format!("resource \"{}\" \"{}\"", labels[0], labels[1]);
                self.push_function(name, signature, "public", node, body_node);
            }
            // data "type" "name" { ... }  → function named "data.type.name"
            "data" if labels.len() >= 2 => {
                let name = format!("data.{}.{}", labels[0], labels[1]);
                let signature = format!("data \"{}\" \"{}\"", labels[0], labels[1]);
                self.push_function(name, signature, "public", node, body_node);
            }
            // output "name" { ... }  → function
            "output" if !labels.is_empty() => {
                let name = format!("output.{}", labels[0]);
                let signature = format!("output \"{}\"", labels[0]);
                self.push_function(name, signature, "public", node, body_node);
            }
            // variable "name" { ... }  → function with parameter semantics
            "variable" if !labels.is_empty() => {
                let name = format!("var.{}", labels[0]);
                let signature = format!("variable \"{}\"", labels[0]);
                let params = vec![Parameter::new(&labels[0])];
                self.push_function_with_params(name, signature, "public", node, body_node, params);
            }
            // module "name" { ... }  → import (reference to external module)
            "module" if !labels.is_empty() => {
                // Try to extract the source attribute value for the import path
                let source_val = body_node
                    .and_then(|b| self.find_attribute_value(*b, "source"))
                    .unwrap_or_else(|| labels[0].clone());

                self.imports.push(ImportRelation {
                    importer: "main".to_string(),
                    imported: source_val,
                    symbols: Vec::new(),
                    is_wildcard: false,
                    alias: Some(labels[0].clone()),
                });
            }
            // provider "name" { ... }  → function
            "provider" if !labels.is_empty() => {
                let name = format!("provider.{}", labels[0]);
                let signature = format!("provider \"{}\"", labels[0]);
                self.push_function(name, signature, "public", node, body_node);
            }
            // locals { ... }  → no specific entity; skip into body
            // terraform { ... }  → skip into body
            _ => {
                // Recurse into body for nested blocks
                if let Some(body) = body_node {
                    let mut body_cursor = body.walk();
                    for child in body.children(&mut body_cursor) {
                        self.visit_node(child);
                    }
                }
            }
        }
    }

    fn push_function(
        &mut self,
        name: String,
        signature: String,
        visibility: &str,
        node: Node,
        body_node: Option<&Node>,
    ) {
        self.push_function_with_params(name, signature, visibility, node, body_node, Vec::new());
    }

    fn push_function_with_params(
        &mut self,
        name: String,
        signature: String,
        visibility: &str,
        node: Node,
        body_node: Option<&Node>,
        parameters: Vec<Parameter>,
    ) {
        let body_prefix = body_node
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| truncate_body_prefix(t).to_string());

        let func = FunctionEntity {
            name,
            signature,
            visibility: visibility.to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async: false,
            is_test: false,
            is_static: false,
            is_abstract: false,
            parameters,
            return_type: None,
            doc_comment: None,
            attributes: Vec::new(),
            parent_class: None,
            complexity: None,
            body_prefix,
        };

        self.functions.push(func);
    }

    /// Search a `body` node's direct `attribute` children for one with the given key,
    /// returning the string value of the expression if it's a string literal.
    fn find_attribute_value(&self, body: Node, key: &str) -> Option<String> {
        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            if child.kind() == "attribute" {
                // attribute: identifier expression
                let mut attr_cursor = child.walk();
                let attr_children: Vec<Node> = child.children(&mut attr_cursor).collect();
                if let Some(id_node) = attr_children.first() {
                    if id_node.kind() == "identifier" && self.node_text(*id_node) == key {
                        // Find the expression → literal_value → string_lit
                        if let Some(expr_node) =
                            attr_children.iter().find(|n| n.kind() == "expression")
                        {
                            return self.extract_string_from_expression(*expr_node);
                        }
                    }
                }
            }
        }
        None
    }

    fn extract_string_from_expression(&self, expr: Node) -> Option<String> {
        let mut cursor = expr.walk();
        for child in expr.children(&mut cursor) {
            match child.kind() {
                "literal_value" => {
                    // literal_value → string_lit
                    let mut lv_cursor = child.walk();
                    for lv_child in child.children(&mut lv_cursor) {
                        if lv_child.kind() == "string_lit" {
                            return Some(self.string_lit_value(lv_child));
                        }
                    }
                }
                "string_lit" => {
                    return Some(self.string_lit_value(child));
                }
                _ => {}
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_visit(source: &[u8]) -> HclVisitor<'_> {
        use tree_sitter::Parser;

        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_hcl::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = HclVisitor::new(source);
        visitor.visit_node(tree.root_node());
        visitor
    }

    #[test]
    fn test_resource_extraction() {
        let source = br#"
resource "aws_instance" "web" {
  ami           = "ami-12345"
  instance_type = "t3.micro"
}
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "aws_instance.web");
    }

    #[test]
    fn test_variable_extraction() {
        let source = br#"
variable "instance_type" {
  type    = string
  default = "t3.micro"
}
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "var.instance_type");
        assert_eq!(visitor.functions[0].parameters.len(), 1);
        assert_eq!(visitor.functions[0].parameters[0].name, "instance_type");
    }

    #[test]
    fn test_module_extraction() {
        let source = br#"
module "vpc" {
  source = "./modules/vpc"
  cidr   = "10.0.0.0/16"
}
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "./modules/vpc");
        assert_eq!(visitor.imports[0].alias, Some("vpc".to_string()));
    }

    #[test]
    fn test_output_extraction() {
        let source = br#"
output "instance_ip" {
  value = "1.2.3.4"
}
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "output.instance_ip");
    }

    #[test]
    fn test_data_extraction() {
        let source = br#"
data "aws_ami" "ubuntu" {
  most_recent = true
}
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "data.aws_ami.ubuntu");
    }

    #[test]
    fn test_full_terraform_file() {
        let source = br#"
terraform {
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
  }
}

variable "instance_type" {
  type    = string
  default = "t3.micro"
}

resource "aws_instance" "web" {
  ami           = "ami-12345"
  instance_type = var.instance_type
}

module "vpc" {
  source = "./modules/vpc"
  cidr   = "10.0.0.0/16"
}

output "instance_ip" {
  value = aws_instance.web.public_ip
}
"#;
        let visitor = parse_and_visit(source);
        // variable + resource + output = 3 functions
        assert_eq!(visitor.functions.len(), 3);
        // module = 1 import
        assert_eq!(visitor.imports.len(), 1);

        let names: Vec<&str> = visitor.functions.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"var.instance_type"));
        assert!(names.contains(&"aws_instance.web"));
        assert!(names.contains(&"output.instance_ip"));
    }
}
