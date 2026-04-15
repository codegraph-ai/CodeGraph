// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting Bash entities

use codegraph_parser_api::{
    truncate_body_prefix, CallRelation, ComplexityBuilder, ComplexityMetrics, FunctionEntity,
    ImportRelation,
};
use tree_sitter::Node;

pub(crate) struct BashVisitor<'a> {
    pub source: &'a [u8],
    pub functions: Vec<FunctionEntity>,
    pub imports: Vec<ImportRelation>,
    pub calls: Vec<CallRelation>,
    current_function: Option<String>,
}

impl<'a> BashVisitor<'a> {
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
            "function_definition" => {
                self.visit_function_definition(node);
                return;
            }
            "command" => {
                self.visit_command(node);
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child);
        }
    }

    fn visit_function_definition(&mut self, node: Node) {
        // Bash function_definition has a "name" field
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

        let body_node = node.child_by_field_name("body");

        let body_prefix = body_node
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| truncate_body_prefix(t).to_string());

        let complexity = body_node.map(|body| self.calculate_complexity(body));

        let func = FunctionEntity {
            name: name.clone(),
            signature,
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
            is_async: false,
            is_test: false,
            is_static: false,
            is_abstract: false,
            parameters: Vec::new(), // Bash uses positional params $1, $2 — not declared in signature
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

    fn visit_command(&mut self, node: Node) {
        // The "name" field of a command is a command_name node; get its text
        let cmd_name = node
            .child_by_field_name("name")
            .map(|n| self.node_text(n))
            .unwrap_or_default();

        // Check for source / . (dot) imports
        if cmd_name == "source" || cmd_name == "." {
            // The first named "argument" field holds the file path
            if let Some(arg) = node.child_by_field_name("argument") {
                let imported = self.node_text(arg);
                let imported = imported.trim_matches(|c| c == '"' || c == '\'').to_string();
                if !imported.is_empty() {
                    self.imports.push(ImportRelation {
                        importer: "main".to_string(),
                        imported,
                        symbols: Vec::new(),
                        is_wildcard: false,
                        alias: None,
                    });
                }
            }
            return;
        }

        // Track calls if inside a function
        if let Some(ref caller) = self.current_function.clone() {
            if !cmd_name.is_empty() {
                self.calls.push(CallRelation {
                    caller: caller.clone(),
                    callee: cmd_name,
                    call_site_line: node.start_position().row + 1,
                    is_direct: true,
                    struct_type: None,
                    field_name: None,
                });
            }
        }
    }

    fn visit_body_for_calls(&mut self, node: Node) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "command" {
                self.visit_command(child);
            }
            self.visit_body_for_calls(child);
        }
    }

    fn extract_doc_comment(&self, node: Node) -> Option<String> {
        if let Some(prev) = node.prev_sibling() {
            if prev.kind() == "comment" {
                let text = self.node_text(prev);
                if text.starts_with('#') {
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
            "elif_clause" | "else_clause" => {
                builder.add_branch();
            }
            "for_statement" | "while_statement" | "until_statement" | "c_style_for_statement" => {
                builder.add_loop();
                builder.enter_scope();
            }
            "case_statement" => {
                builder.add_branch();
                builder.enter_scope();
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
            | "while_statement"
            | "until_statement"
            | "c_style_for_statement"
            | "case_statement" => {
                builder.exit_scope();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_visit(source: &[u8]) -> BashVisitor<'_> {
        use tree_sitter::Parser;

        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_bash::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = BashVisitor::new(source);
        visitor.visit_node(tree.root_node());
        visitor
    }

    #[test]
    fn test_visitor_function_extraction() {
        let source = b"greet() {\n    echo \"Hello\"\n}\n";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "greet");
    }

    #[test]
    fn test_visitor_source_import() {
        let source = b"source ./lib.sh\n";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "./lib.sh");
    }
}
