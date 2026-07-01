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

    #[test]
    fn test_resource_full_props() {
        let source = br#"resource "aws_instance" "web" {
  ami = "ami-12345"
}"#;
        let visitor = parse_and_visit(source);
        let f = &visitor.functions[0];
        assert_eq!(f.signature, "resource \"aws_instance\" \"web\"");
        assert_eq!(f.visibility, "public");
        assert_eq!(f.line_start, 1);
        assert_eq!(f.line_end, 3);
        // Non-block-type props are all defaults for HCL entities.
        assert!(!f.is_async);
        assert!(!f.is_test);
        assert!(!f.is_static);
        assert!(!f.is_abstract);
        assert!(f.return_type.is_none());
        assert!(f.doc_comment.is_none());
        assert!(f.parent_class.is_none());
        assert!(f.complexity.is_none());
        assert!(f.attributes.is_empty());
        assert!(f.parameters.is_empty());
        // body_prefix captures the block body text.
        assert!(f.body_prefix.as_deref().unwrap().contains("ami-12345"));
    }

    #[test]
    fn test_data_signature() {
        let source = br#"data "aws_ami" "ubuntu" {
  most_recent = true
}"#;
        let visitor = parse_and_visit(source);
        assert_eq!(
            visitor.functions[0].signature,
            "data \"aws_ami\" \"ubuntu\""
        );
        assert_eq!(visitor.functions[0].visibility, "public");
    }

    #[test]
    fn test_output_signature() {
        let source = br#"output "instance_ip" {
  value = "1.2.3.4"
}"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions[0].signature, "output \"instance_ip\"");
    }

    #[test]
    fn test_variable_signature_and_parameter() {
        let source = br#"variable "region" {
  type = string
}"#;
        let visitor = parse_and_visit(source);
        let f = &visitor.functions[0];
        assert_eq!(f.name, "var.region");
        assert_eq!(f.signature, "variable \"region\"");
        // variable synthesizes a single parameter named after the label.
        assert_eq!(f.parameters.len(), 1);
        assert_eq!(f.parameters[0].name, "region");
    }

    #[test]
    fn test_provider_extraction() {
        let source = br#"provider "aws" {
  region = "us-east-1"
}"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "provider.aws");
        assert_eq!(visitor.functions[0].signature, "provider \"aws\"");
    }

    #[test]
    fn test_module_import_fields() {
        let source = br#"module "vpc" {
  source = "./modules/vpc"
}"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 1);
        let imp = &visitor.imports[0];
        assert_eq!(imp.importer, "main");
        assert_eq!(imp.imported, "./modules/vpc");
        assert_eq!(imp.alias, Some("vpc".to_string()));
        assert!(imp.symbols.is_empty());
        assert!(!imp.is_wildcard);
    }

    #[test]
    fn test_module_without_source_falls_back_to_label() {
        // No `source` attribute → imported defaults to the module label.
        let source = br#"module "vpc" {
  cidr = "10.0.0.0/16"
}"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "vpc");
        assert_eq!(visitor.imports[0].alias, Some("vpc".to_string()));
    }

    #[test]
    fn test_resource_body_nested_block_not_recursed() {
        // Matched block arms push a function and do NOT recurse into the body,
        // so a nested block inside a resource is not extracted.
        let source = br#"resource "aws_instance" "web" {
  network_interface {
    device_index = 0
  }
}"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "aws_instance.web");
    }

    #[test]
    fn test_unknown_block_recurses_into_body() {
        // An unrecognized block type falls through to the default arm, which
        // recurses into the body so nested matched blocks are still extracted.
        let source = br#"check "health" {
  data "http" "test" {
    url = "https://example.com"
  }
}"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "data.http.test");
    }

    #[test]
    fn test_terraform_block_yields_no_function() {
        let source = br#"terraform {
  required_version = ">= 1.0"
}"#;
        let visitor = parse_and_visit(source);
        assert!(visitor.functions.is_empty());
        assert!(visitor.imports.is_empty());
    }

    #[test]
    fn test_empty_source() {
        let visitor = parse_and_visit(b"");
        assert!(visitor.functions.is_empty());
        assert!(visitor.imports.is_empty());
        assert!(visitor.calls.is_empty());
    }

    #[test]
    fn test_multiple_resources_ordering() {
        let source = br#"resource "aws_instance" "web" {
  ami = "a"
}

resource "aws_s3_bucket" "data" {
  bucket = "b"
}"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 2);
        assert_eq!(visitor.functions[0].name, "aws_instance.web");
        assert_eq!(visitor.functions[1].name, "aws_s3_bucket.data");
    }

    #[test]
    fn test_resource_single_label_skipped() {
        // resource with fewer than 2 labels does not match the resource arm;
        // it falls through to the default arm (no function, recurse empty body).
        let source = br#"resource "aws_instance" {
  ami = "a"
}"#;
        let visitor = parse_and_visit(source);
        assert!(visitor.functions.is_empty());
    }

    #[test]
    fn test_module_source_strips_quotes() {
        // The source attribute value has its surrounding double-quotes stripped.
        let source = br#"module "net" {
  source = "terraform-aws-modules/vpc/aws"
}"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports[0].imported, "terraform-aws-modules/vpc/aws");
    }

    #[test]
    fn test_data_line_numbers_and_body_prefix() {
        // Leading blank line offsets the 1-indexed line numbers; body_prefix
        // captures the block body text.
        let source = br#"
data "aws_ami" "ubuntu" {
  most_recent = true
}"#;
        let visitor = parse_and_visit(source);
        let f = &visitor.functions[0];
        assert_eq!(f.line_start, 2);
        assert_eq!(f.line_end, 4);
        assert!(f.body_prefix.as_deref().unwrap().contains("most_recent"));
    }

    #[test]
    fn test_output_line_numbers_and_body_prefix() {
        let source = br#"output "instance_ip" {
  value = "1.2.3.4"
}"#;
        let visitor = parse_and_visit(source);
        let f = &visitor.functions[0];
        assert_eq!(f.line_start, 1);
        assert_eq!(f.line_end, 3);
        assert!(f.body_prefix.as_deref().unwrap().contains("1.2.3.4"));
    }

    #[test]
    fn test_provider_body_prefix() {
        let source = br#"provider "aws" {
  region = "us-east-1"
}"#;
        let visitor = parse_and_visit(source);
        let f = &visitor.functions[0];
        assert!(f.body_prefix.as_deref().unwrap().contains("us-east-1"));
    }

    #[test]
    fn test_variable_body_prefix() {
        let source = br#"variable "region" {
  type    = string
  default = "us-west-2"
}"#;
        let visitor = parse_and_visit(source);
        let f = &visitor.functions[0];
        assert!(f.body_prefix.as_deref().unwrap().contains("us-west-2"));
    }

    #[test]
    fn test_two_modules_ordering() {
        let source = br#"module "vpc" {
  source = "./modules/vpc"
}

module "eks" {
  source = "./modules/eks"
}"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 2);
        assert_eq!(visitor.imports[0].imported, "./modules/vpc");
        assert_eq!(visitor.imports[0].alias, Some("vpc".to_string()));
        assert_eq!(visitor.imports[1].imported, "./modules/eks");
        assert_eq!(visitor.imports[1].alias, Some("eks".to_string()));
    }

    #[test]
    fn test_module_source_non_string_falls_back_to_label() {
        // A `source` whose value is not a string literal (a bare reference)
        // is not extractable, so imported falls back to the module label.
        let source = br#"module "vpc" {
  source = local.vpc_source
}"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "vpc");
    }

    #[test]
    fn test_resource_extra_labels_uses_first_two() {
        // A resource with more than two labels still names from the first two.
        let source = br#"resource "aws_instance" "web" "extra" {
  ami = "a"
}"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "aws_instance.web");
    }

    #[test]
    fn test_data_single_label_skipped() {
        // data with fewer than 2 labels does not match; no function emitted.
        let source = br#"data "aws_ami" {
  most_recent = true
}"#;
        let visitor = parse_and_visit(source);
        assert!(visitor.functions.is_empty());
    }

    #[test]
    fn test_provider_without_label_skipped() {
        // provider with no label fails the guard and yields no function.
        let source = br#"provider {
  region = "us-east-1"
}"#;
        let visitor = parse_and_visit(source);
        assert!(visitor.functions.is_empty());
    }

    #[test]
    fn test_module_without_label_yields_no_import() {
        // module with no label falls through to the default arm; no import.
        let source = br#"module {
  source = "./modules/vpc"
}"#;
        let visitor = parse_and_visit(source);
        assert!(visitor.imports.is_empty());
    }

    #[test]
    fn test_mixed_blocks_functions_and_imports_recorded_separately() {
        // A resource and a module in one file populate functions and imports
        // independently.
        let source = br#"resource "aws_instance" "web" {
  ami = "a"
}

module "vpc" {
  source = "./modules/vpc"
}"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "aws_instance.web");
        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "./modules/vpc");
    }

    #[test]
    fn test_body_prefix_truncated_to_max_chars() {
        // An oversized block body is truncated to BODY_PREFIX_MAX_CHARS.
        let filler = "x".repeat(2000);
        let source = format!("resource \"aws_instance\" \"web\" {{\n  ami = \"{filler}\"\n}}");
        let visitor = parse_and_visit(source.as_bytes());
        let f = &visitor.functions[0];
        assert_eq!(
            f.body_prefix.as_deref().unwrap().len(),
            codegraph_parser_api::BODY_PREFIX_MAX_CHARS
        );
    }

    #[test]
    fn test_nested_block_extracted_via_recursion() {
        // An unrecognized block type falls through to the default arm, which
        // recurses into its body and extracts a nested recognized block.
        let source = br#"group {
  resource "aws_instance" "web" {
    ami = "a"
  }
}"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "aws_instance.web");
    }

    #[test]
    fn test_module_without_source_attribute_falls_back_to_label() {
        // A module block with no `source` attribute at all uses the label as
        // the imported path.
        let source = br#"module "vpc" {
  cidr = "10.0.0.0/16"
}"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "vpc");
    }

    #[test]
    fn test_empty_body_resource_has_no_body_prefix() {
        // An empty block body yields no body_prefix (filtered out as empty).
        let source = br#"resource "aws_instance" "web" {}"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 1);
        assert!(visitor.functions[0].body_prefix.is_none());
    }

    #[test]
    fn test_module_import_flags_default() {
        // A module import records no symbols and is not a wildcard.
        let source = br#"module "vpc" {
  source = "./modules/vpc"
}"#;
        let visitor = parse_and_visit(source);
        let imp = &visitor.imports[0];
        assert!(imp.symbols.is_empty());
        assert!(!imp.is_wildcard);
        assert_eq!(imp.importer, "main");
    }

    #[test]
    fn test_resource_visibility_is_public() {
        let source = br#"resource "aws_instance" "web" {
  ami = "a"
}"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions[0].visibility, "public");
    }

    #[test]
    fn test_data_signature_format() {
        let source = br#"data "aws_ami" "ubuntu" {
  most_recent = true
}"#;
        let visitor = parse_and_visit(source);
        assert_eq!(
            visitor.functions[0].signature,
            "data \"aws_ami\" \"ubuntu\""
        );
    }

    #[test]
    fn test_resource_signature_format() {
        let source = br#"resource "aws_instance" "web" {
  ami = "a"
}"#;
        let visitor = parse_and_visit(source);
        assert_eq!(
            visitor.functions[0].signature,
            "resource \"aws_instance\" \"web\""
        );
    }

    #[test]
    fn test_variable_signature_and_param_name() {
        let source = br#"variable "region" {
  type = string
}"#;
        let visitor = parse_and_visit(source);
        let f = &visitor.functions[0];
        assert_eq!(f.signature, "variable \"region\"");
        assert_eq!(f.parameters[0].name, "region");
    }

    #[test]
    fn test_locals_block_yields_nothing() {
        // A locals block is unrecognized; its attribute-only body produces no
        // functions or imports.
        let source = br#"locals {
  common_tags = "x"
}"#;
        let visitor = parse_and_visit(source);
        assert!(visitor.functions.is_empty());
        assert!(visitor.imports.is_empty());
    }

    #[test]
    fn test_provider_line_numbers() {
        let source = br#"provider "aws" {
  region = "us-east-1"
}"#;
        let visitor = parse_and_visit(source);
        let f = &visitor.functions[0];
        assert_eq!(f.name, "provider.aws");
        assert_eq!(f.line_start, 1);
        assert_eq!(f.line_end, 3);
    }

    #[test]
    fn test_output_multiple_labels_uses_first() {
        // output signature/name derive from the first label only.
        let source = br#"output "ip" "extra" {
  value = "1.2.3.4"
}"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions[0].name, "output.ip");
        assert_eq!(visitor.functions[0].signature, "output \"ip\"");
    }
}
