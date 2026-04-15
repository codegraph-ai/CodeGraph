// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting R entities

use codegraph_parser_api::{
    truncate_body_prefix, CallRelation, ComplexityBuilder, ComplexityMetrics, FunctionEntity,
    ImportRelation, Parameter,
};
use tree_sitter::Node;

pub(crate) struct RVisitor<'a> {
    pub source: &'a [u8],
    pub functions: Vec<FunctionEntity>,
    pub imports: Vec<ImportRelation>,
    pub calls: Vec<CallRelation>,
    current_function: Option<String>,
}

impl<'a> RVisitor<'a> {
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
            "binary_operator" => {
                // Check for function assignment: name <- function(...) { ... }
                self.visit_binary_operator(node);
                return;
            }
            "call" => {
                self.visit_call(node);
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child);
        }
    }

    fn visit_binary_operator(&mut self, node: Node) {
        // Pattern: name <- function(params) body
        // or: name = function(params) body
        let text = self.node_text(node);
        if !text.contains("function") {
            // Still recurse for nested function assignments
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                self.visit_node(child);
            }
            return;
        }

        // Get operator
        let mut cursor = node.walk();
        let children: Vec<Node> = node.children(&mut cursor).collect();

        if children.len() < 3 {
            return;
        }

        let lhs = children[0];
        let operator = children[1];
        let op_text = self.node_text(operator);

        // Must be <- or = or <<-
        if op_text != "<-" && op_text != "=" && op_text != "<<-" {
            // Recurse
            for child in &children {
                self.visit_node(*child);
            }
            return;
        }

        let rhs = children[2];

        // Check if RHS is a function_definition
        if rhs.kind() != "function_definition" {
            // Recurse
            for child in &children {
                self.visit_node(*child);
            }
            return;
        }

        let name = self.node_text(lhs);
        if name.is_empty() {
            return;
        }

        let doc_comment = self.extract_doc_comment(node);
        let parameters = self.extract_parameters(rhs);

        let body_prefix = rhs
            .child_by_field_name("body")
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| truncate_body_prefix(t).to_string());

        let complexity = rhs
            .child_by_field_name("body")
            .map(|body| self.calculate_complexity(body));

        let signature = format!(
            "{} <- function({})",
            name,
            parameters
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );

        let func = FunctionEntity {
            name: name.clone(),
            signature,
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async: false,
            is_test: name.starts_with("test_") || name.starts_with("test."),
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

        // Visit body for calls
        let previous_function = self.current_function.take();
        self.current_function = Some(name);

        if let Some(body) = rhs.child_by_field_name("body") {
            self.visit_body_for_calls(body);
        }

        self.current_function = previous_function;
    }

    fn visit_call(&mut self, node: Node) {
        if let Some(func_node) = node.child_by_field_name("function") {
            let func_name = self.node_text(func_node);

            // Check for library/require/source imports
            if func_name == "library" || func_name == "require" || func_name == "source" {
                if let Some(args) = node.child_by_field_name("arguments") {
                    let mut cursor = args.walk();
                    for arg in args.children(&mut cursor) {
                        if arg.kind() == "identifier" || arg.kind() == "string" {
                            let module = self.node_text(arg);
                            let module = module.trim_matches(|c| c == '"' || c == '\'').to_string();
                            if !module.is_empty() && module != "(" && module != ")" && module != ","
                            {
                                self.imports.push(ImportRelation {
                                    importer: "main".to_string(),
                                    imported: module,
                                    symbols: Vec::new(),
                                    is_wildcard: false,
                                    alias: Some(func_name.clone()),
                                });
                                break;
                            }
                        }
                    }
                }
            }

            // Track calls within functions
            if let Some(ref caller) = self.current_function.clone() {
                if !func_name.is_empty()
                    && func_name != "library"
                    && func_name != "require"
                    && func_name != "source"
                {
                    self.calls.push(CallRelation {
                        caller: caller.clone(),
                        callee: func_name,
                        call_site_line: node.start_position().row + 1,
                        is_direct: true,
                        struct_type: None,
                        field_name: None,
                    });
                }
            }
        }
    }

    fn visit_body_for_calls(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "call" {
                self.visit_call(child);
            }
            self.visit_body_for_calls(child);
        }
    }

    fn extract_parameters(&self, node: Node) -> Vec<Parameter> {
        let mut params = Vec::new();
        if let Some(params_node) = node.child_by_field_name("parameters") {
            let mut cursor = params_node.walk();
            for child in params_node.children(&mut cursor) {
                match child.kind() {
                    "identifier" => {
                        params.push(Parameter::new(self.node_text(child)));
                    }
                    "parameter" => {
                        if let Some(name_node) = child.child_by_field_name("name") {
                            let mut param = Parameter::new(self.node_text(name_node));
                            if let Some(default_node) = child.child_by_field_name("default") {
                                param = param.with_default(self.node_text(default_node));
                            }
                            params.push(param);
                        }
                    }
                    "dots" => {
                        params.push(Parameter::new("...").variadic());
                    }
                    _ => {}
                }
            }
        }
        params
    }

    fn extract_doc_comment(&self, node: Node) -> Option<String> {
        if let Some(prev) = node.prev_sibling() {
            if prev.kind() == "comment" {
                let text = self.node_text(prev);
                if text.starts_with("#'") {
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
            "for_statement" | "while_statement" | "repeat_statement" => {
                builder.add_loop();
                builder.enter_scope();
            }
            "binary_operator" => {
                let text = self.node_text(node);
                if text.contains("&&")
                    || text.contains("||")
                    || text.contains("&")
                    || text.contains("|")
                {
                    // Only count && and || (not & and | which are vectorized)
                    let op_node = node.child(1);
                    if let Some(op) = op_node {
                        let op_text = self.node_text(op);
                        if op_text == "&&" || op_text == "||" {
                            builder.add_logical_operator();
                        }
                    }
                }
            }
            "call" => {
                // tryCatch is R's exception handling
                if let Some(func_node) = node.child_by_field_name("function") {
                    let name = self.node_text(func_node);
                    if name == "tryCatch" || name == "try" {
                        builder.add_exception_handler();
                    }
                }
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_for_complexity(child, builder);
        }

        match node.kind() {
            "if_statement" | "for_statement" | "while_statement" | "repeat_statement" => {
                builder.exit_scope();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_visit(source: &[u8]) -> RVisitor<'_> {
        use tree_sitter::Parser;

        let mut parser = Parser::new();
        parser.set_language(&crate::ts_r::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = RVisitor::new(source);
        visitor.visit_node(tree.root_node());
        visitor
    }

    #[test]
    fn test_visitor_function_extraction() {
        let source = b"add <- function(a, b) {\n    a + b\n}";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "add");
    }

    #[test]
    fn test_visitor_library_extraction() {
        let source = b"library(ggplot2)";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "ggplot2");
    }
}
