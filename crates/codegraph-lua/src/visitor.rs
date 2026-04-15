// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting Lua entities

use codegraph_parser_api::{
    truncate_body_prefix, CallRelation, ComplexityBuilder, ComplexityMetrics, FunctionEntity,
    ImportRelation, Parameter,
};
use tree_sitter::Node;

pub(crate) struct LuaVisitor<'a> {
    pub source: &'a [u8],
    pub functions: Vec<FunctionEntity>,
    pub imports: Vec<ImportRelation>,
    pub calls: Vec<CallRelation>,
    current_function: Option<String>,
}

impl<'a> LuaVisitor<'a> {
    pub fn new(source: &'a [u8]) -> Self {
        Self {
            source,
            functions: Vec::new(),
            imports: Vec::new(),
            calls: Vec::new(),
            current_function: None,
        }
    }

    fn node_text(&self, node: Node) -> String {
        node.utf8_text(self.source).unwrap_or("").to_string()
    }

    pub fn visit_node(&mut self, node: Node) {
        match node.kind() {
            "function_declaration" => {
                self.visit_function_declaration(node);
                return;
            }
            "variable_declaration" => {
                // Check for local function pattern or require calls
                self.visit_variable_declaration(node);
                return;
            }
            "function_call" => {
                self.visit_function_call(node);
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child);
        }
    }

    fn visit_function_declaration(&mut self, node: Node) {
        let name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_default();

        if name.is_empty() {
            return;
        }

        let signature = self
            .node_text(node)
            .lines()
            .next()
            .unwrap_or("")
            .to_string();

        let doc_comment = self.extract_doc_comment(node);
        let parameters = self.extract_parameters(node);

        let body_prefix = node
            .child_by_field_name("body")
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| truncate_body_prefix(t).to_string());

        let complexity = node
            .child_by_field_name("body")
            .map(|body| self.calculate_complexity(body));

        let is_local = {
            let text = self.node_text(node);
            text.starts_with("local ")
        };

        let func = FunctionEntity {
            name: name.clone(),
            signature,
            visibility: if is_local { "private" } else { "public" }.to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async: false,
            is_test: false,
            is_static: false,
            is_abstract: false,
            parameters,
            return_type: None,
            doc_comment,
            attributes: Vec::new(),
            parent_class: None,
            complexity,
            body_prefix,
        };

        self.functions.push(func);

        let previous_function = self.current_function.take();
        self.current_function = Some(name);

        if let Some(body) = node.child_by_field_name("body") {
            self.visit_body_for_calls(body);
        }

        self.current_function = previous_function;
    }

    fn visit_variable_declaration(&mut self, node: Node) {
        let text = self.node_text(node);

        // Check for require: local foo = require("bar")
        if text.contains("require") {
            self.extract_require_from_text(&text);
        }

        // Check for local function assigned to variable
        // local foo = function(...) ... end
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "function_definition" {
                // Get the variable name from the assignment
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = self.node_text(name_node);
                    if !name.is_empty() {
                        let signature = format!("local {} = function", name);
                        let body_prefix = child
                            .child_by_field_name("body")
                            .and_then(|b| b.utf8_text(self.source).ok())
                            .filter(|t| !t.is_empty())
                            .map(|t| truncate_body_prefix(t).to_string());

                        let complexity = child
                            .child_by_field_name("body")
                            .map(|body| self.calculate_complexity(body));

                        let parameters = self.extract_parameters(child);

                        let func = FunctionEntity {
                            name: name.clone(),
                            signature,
                            visibility: "private".to_string(),
                            line_start: node.start_position().row + 1,
                            line_end: child.end_position().row + 1,
                            is_async: false,
                            is_test: false,
                            is_static: false,
                            is_abstract: false,
                            parameters,
                            return_type: None,
                            doc_comment: None,
                            attributes: Vec::new(),
                            parent_class: None,
                            complexity,
                            body_prefix,
                        };

                        self.functions.push(func);
                    }
                }
            } else {
                self.visit_node(child);
            }
        }
    }

    fn visit_function_call(&mut self, node: Node) {
        // Check for require("module") calls
        let text = self.node_text(node);
        if text.starts_with("require") {
            self.extract_require_from_text(&text);
        }

        // Track calls if inside a function
        if let Some(ref caller) = self.current_function.clone() {
            if let Some(name_node) = node.child_by_field_name("name") {
                let callee = self.node_text(name_node);
                if !callee.is_empty() && callee != "require" {
                    self.calls.push(CallRelation {
                        caller: caller.clone(),
                        callee,
                        call_site_line: node.start_position().row + 1,
                        is_direct: true,
                        struct_type: None,
                        field_name: None,
                    });
                }
            }
        }
    }

    fn extract_require_from_text(&mut self, text: &str) {
        // Match require("module") or require('module')
        if let Some(start) = text.find("require(") {
            let after = &text[start + 8..];
            let quote = after.chars().next();
            if let Some(q) = quote {
                if q == '"' || q == '\'' {
                    if let Some(end) = after[1..].find(q) {
                        let module = &after[1..1 + end];
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
    }

    fn visit_body_for_calls(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "function_call" {
                self.visit_function_call(child);
            }
            self.visit_body_for_calls(child);
        }
    }

    fn extract_parameters(&self, node: Node) -> Vec<Parameter> {
        let mut params = Vec::new();
        if let Some(params_node) = node.child_by_field_name("parameters") {
            let mut cursor = params_node.walk();
            for child in params_node.children(&mut cursor) {
                if child.kind() == "identifier" {
                    params.push(Parameter::new(self.node_text(child)));
                } else if child.kind() == "spread" {
                    params.push(Parameter::new("...").variadic());
                }
            }
        }
        params
    }

    fn extract_doc_comment(&self, node: Node) -> Option<String> {
        if let Some(prev) = node.prev_sibling() {
            if prev.kind() == "comment" {
                let text = self.node_text(prev);
                if text.starts_with("---") || text.starts_with("--!") {
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
            "if_statement" => {
                builder.add_branch();
                builder.enter_scope();
            }
            "elseif_statement" => {
                builder.add_branch();
            }
            "else_statement" => {
                builder.add_branch();
            }
            "for_statement" | "for_generic_statement" | "while_statement" | "repeat_statement" => {
                builder.add_loop();
                builder.enter_scope();
            }
            "binary_expression" => {
                let text = self.node_text(node);
                if text.contains(" and ") || text.contains(" or ") {
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
            "if_statement"
            | "for_statement"
            | "for_generic_statement"
            | "while_statement"
            | "repeat_statement" => {
                builder.exit_scope();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_visit(source: &[u8]) -> LuaVisitor<'_> {
        use tree_sitter::Parser;

        let mut parser = Parser::new();
        parser.set_language(&tree_sitter_lua::LANGUAGE.into()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = LuaVisitor::new(source);
        visitor.visit_node(tree.root_node());
        visitor
    }

    #[test]
    fn test_visitor_function_extraction() {
        let source = b"function greet(name)\n    print(\"Hello, \" .. name)\nend";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "greet");
    }

    #[test]
    fn test_visitor_require_extraction() {
        let source = b"local json = require(\"json\")";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "json");
    }
}
