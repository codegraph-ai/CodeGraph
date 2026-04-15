// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting Zig entities (ABI 15 grammar)

use codegraph_parser_api::{
    truncate_body_prefix, CallRelation, ClassEntity, ComplexityBuilder, ComplexityMetrics,
    FunctionEntity, ImportRelation, Parameter,
};
use tree_sitter::Node;

pub(crate) struct ZigVisitor<'a> {
    pub source: &'a [u8],
    pub functions: Vec<FunctionEntity>,
    pub classes: Vec<ClassEntity>,
    pub imports: Vec<ImportRelation>,
    pub calls: Vec<CallRelation>,
    current_function: Option<String>,
    current_struct: Option<String>,
}

impl<'a> ZigVisitor<'a> {
    pub fn new(source: &'a [u8]) -> Self {
        Self {
            source,
            functions: Vec::new(),
            classes: Vec::new(),
            imports: Vec::new(),
            calls: Vec::new(),
            current_function: None,
            current_struct: None,
        }
    }

    fn node_text(&self, node: Node) -> String {
        node.utf8_text(self.source).unwrap_or("").to_string()
    }

    /// Find the first child with a given kind.
    fn find_child_by_kind<'b>(&self, node: Node<'b>, kind: &str) -> Option<Node<'b>> {
        let mut cursor = node.walk();
        let result = node.children(&mut cursor).find(|c| c.kind() == kind);
        result
    }

    pub fn visit_node(&mut self, node: Node) {
        match node.kind() {
            "function_declaration" => {
                self.visit_function(node);
                return;
            }
            "variable_declaration" => {
                self.visit_variable_declaration(node);
            }
            "struct_declaration" | "enum_declaration" | "union_declaration" => {
                self.visit_struct(node);
                return; // visit_struct recurses into children for methods
            }
            "test_declaration" => {
                self.visit_test(node);
                return;
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child);
        }
    }

    fn visit_function(&mut self, node: Node) {
        // ABI 15: child_by_field_name("name") works for function_declaration
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_default();

        if name.is_empty() {
            return;
        }

        // Build signature from the first line of the function text
        let signature = self
            .node_text(node)
            .lines()
            .next()
            .unwrap_or("")
            .to_string();

        let doc_comment = self.extract_doc_comment(node);
        let parameters = self.extract_parameters(node);
        let return_type = self.extract_return_type(node);
        let is_pub = {
            let mut cursor = node.walk();
            let result = node.children(&mut cursor).any(|c| c.kind() == "pub");
            result
        };

        // ABI 15: child_by_field_name("body") works for function_declaration
        let body_node = node.child_by_field_name("body");

        let body_prefix = body_node
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| truncate_body_prefix(t).to_string());

        let complexity = body_node.map(|body| self.calculate_complexity(body));

        let func = FunctionEntity {
            name: name.clone(),
            signature,
            visibility: if is_pub { "public" } else { "private" }.to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async: false,
            is_test: false,
            is_static: false,
            is_abstract: false,
            parameters,
            return_type,
            doc_comment,
            attributes: Vec::new(),
            parent_class: self.current_struct.clone(),
            complexity,
            body_prefix,
        };

        self.functions.push(func);

        let previous_function = self.current_function.take();
        self.current_function = Some(name);

        if let Some(body) = body_node {
            self.visit_body_for_calls(body);
        }

        self.current_function = previous_function;
    }

    fn visit_variable_declaration(&mut self, node: Node) {
        // Look for @import calls: const std = @import("std");
        let text = self.node_text(node);
        if text.contains("@import") {
            if let Some(start) = text.find("@import(\"") {
                let after = &text[start + 9..];
                if let Some(end) = after.find('"') {
                    let module = &after[..end];
                    self.imports.push(ImportRelation {
                        importer: "main".to_string(),
                        imported: module.to_string(),
                        symbols: Vec::new(),
                        is_wildcard: false,
                        alias: None,
                    });
                }
            }
        }
    }

    fn visit_struct(&mut self, node: Node) {
        // In ABI 15, structs are: variable_declaration -> [pub?, const, identifier, =, struct_declaration, ;]
        // The name is the identifier child of the parent variable_declaration.
        let name = if let Some(parent) = node.parent() {
            if parent.kind() == "variable_declaration" {
                self.find_child_by_kind(parent, "identifier")
                    .map(|n| self.node_text(n))
                    .unwrap_or_default()
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        if name.is_empty() {
            return;
        }

        let doc_comment = if let Some(parent) = node.parent() {
            self.extract_doc_comment(parent)
        } else {
            None
        };

        let kind = node.kind();
        let attrs = match kind {
            "enum_declaration" => vec!["enum".to_string()],
            "union_declaration" => vec!["union".to_string()],
            _ => vec!["struct".to_string()],
        };

        let class_entity = ClassEntity {
            name: name.clone(),
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_abstract: false,
            is_interface: false,
            base_classes: Vec::new(),
            implemented_traits: Vec::new(),
            methods: Vec::new(),
            fields: Vec::new(),
            doc_comment,
            attributes: attrs,
            type_parameters: Vec::new(),
            body_prefix: None,
        };

        self.classes.push(class_entity);

        // Recurse into struct children to find methods (function_declaration nodes)
        let previous_struct = self.current_struct.take();
        self.current_struct = Some(name);

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child);
        }

        self.current_struct = previous_struct;
    }

    fn visit_test(&mut self, node: Node) {
        // ABI 15: test_declaration has children [test, string, block]
        // Try field name first, then fall back to finding string child by kind
        let name = node
            .child_by_field_name("name")
            .or_else(|| self.find_child_by_kind(node, "string"))
            .map(|n| {
                let t = self.node_text(n);
                t.trim_matches('"').to_string()
            })
            .unwrap_or_else(|| "test".to_string());

        let func = FunctionEntity {
            name,
            signature: self
                .node_text(node)
                .lines()
                .next()
                .unwrap_or("")
                .to_string(),
            visibility: "private".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async: false,
            is_test: true,
            is_static: false,
            is_abstract: false,
            parameters: Vec::new(),
            return_type: None,
            doc_comment: None,
            attributes: Vec::new(),
            parent_class: None,
            complexity: None,
            body_prefix: None,
        };

        self.functions.push(func);
    }

    fn visit_body_for_calls(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            // ABI 15: function calls are "call_expression", builtins are "builtin_function"
            if child.kind() == "call_expression" || child.kind() == "builtin_function" {
                if let Some(ref caller) = self.current_function.clone() {
                    let callee = child
                        .child_by_field_name("function")
                        .or_else(|| child.child(0))
                        .map(|n| self.node_text(n))
                        .unwrap_or_default();
                    if !callee.is_empty() {
                        self.calls.push(CallRelation {
                            caller: caller.clone(),
                            callee,
                            call_site_line: child.start_position().row + 1,
                            is_direct: true,
                            struct_type: None,
                            field_name: None,
                        });
                    }
                }
            }
            self.visit_body_for_calls(child);
        }
    }

    fn extract_parameters(&self, node: Node) -> Vec<Parameter> {
        let mut params = Vec::new();
        // ABI 15: child_by_field_name("parameters") does NOT work;
        // find the "parameters" child by kind instead
        let params_node = self.find_child_by_kind(node, "parameters");
        if let Some(params_node) = params_node {
            let mut cursor = params_node.walk();
            for child in params_node.children(&mut cursor) {
                if child.kind() == "parameter" {
                    // ABI 15: child_by_field_name("name") and ("type") work on parameter nodes
                    if let Some(name_node) = child.child_by_field_name("name") {
                        let name = self.node_text(name_node);
                        let param_type =
                            child.child_by_field_name("type").map(|t| self.node_text(t));
                        let mut param = Parameter::new(name);
                        if let Some(t) = param_type {
                            param = param.with_type(t);
                        }
                        params.push(param);
                    }
                }
            }
        }
        params
    }

    fn extract_return_type(&self, node: Node) -> Option<String> {
        // ABI 15: no "return_type" field. The return type is the named node
        // between the parameters node and the body (block) node.
        // Function children: [pub?, fn, identifier, parameters, return_type_node, block]
        let mut found_params = false;
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "parameters" {
                found_params = true;
                continue;
            }
            if found_params && child.is_named() && child.kind() != "block" {
                return Some(self.node_text(child));
            }
            if child.kind() == "block" {
                break;
            }
        }
        None
    }

    fn extract_doc_comment(&self, node: Node) -> Option<String> {
        if let Some(prev) = node.prev_sibling() {
            if prev.kind() == "line_comment" || prev.kind() == "doc_comment" {
                let text = self.node_text(prev);
                if text.starts_with("///") || text.starts_with("//!") {
                    return Some(text);
                }
            }
        }
        None
    }

    fn calculate_complexity(&self, body: Node) -> ComplexityMetrics {
        let mut builder = ComplexityBuilder::new();
        self.visit_for_complexity(body, &mut builder);
        builder.build()
    }

    fn visit_for_complexity(&self, node: Node, builder: &mut ComplexityBuilder) {
        match node.kind() {
            "if_expression" | "if_statement" => {
                builder.add_branch();
                builder.enter_scope();
            }
            "else" | "else_clause" => {
                builder.add_branch();
            }
            "switch_expression" | "switch_statement" => {
                builder.enter_scope();
            }
            "switch_prong" => {
                builder.add_branch();
            }
            "for_expression" | "for_statement" | "while_expression" | "while_statement" => {
                builder.add_loop();
                builder.enter_scope();
            }
            "catch" => {
                builder.add_exception_handler();
            }
            "binary_expression" => {
                let text = self.node_text(node);
                if text.contains("and") || text.contains("or") {
                    builder.add_logical_operator();
                }
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_for_complexity(child, builder);
        }

        match node.kind() {
            "if_expression" | "if_statement" | "for_expression" | "for_statement"
            | "while_expression" | "while_statement" | "switch_expression" | "switch_statement" => {
                builder.exit_scope();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use tree_sitter::Parser;

    fn parse_and_visit(source: &[u8]) -> ZigVisitor<'_> {
        let mut parser = Parser::new();
        parser.set_language(&crate::ts_zig::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = ZigVisitor::new(source);
        visitor.visit_node(tree.root_node());
        visitor
    }

    #[test]
    fn test_visitor_function_extraction() {
        let source = b"pub fn add(a: i32, b: i32) i32 {\n    return a + b;\n}";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "add");
        assert_eq!(visitor.functions[0].visibility, "public");
    }

    #[test]
    fn test_visitor_import_extraction() {
        let source = b"const std = @import(\"std\");";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "std");
    }
}
