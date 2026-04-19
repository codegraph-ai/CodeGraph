// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for Dockerfile directives.
//!
//! Each top-level directive (FROM, RUN, USER, EXPOSE, ENV, COPY, etc.) is captured
//! as a `FunctionEntity` so the IaC security scanner can match patterns like
//! `USER root`, `:latest` images, hardcoded secrets in ENV/ARG, exposed port 22, etc.
//!
//! The directive's full source text is stored in `body_prefix`.

use codegraph_parser_api::{truncate_body_prefix, FunctionEntity};
use tree_sitter::Node;

pub(crate) struct DockerfileVisitor<'a> {
    pub source: &'a [u8],
    pub functions: Vec<FunctionEntity>,
}

impl<'a> DockerfileVisitor<'a> {
    pub fn new(source: &'a [u8]) -> Self {
        Self {
            source,
            functions: Vec::new(),
        }
    }

    fn node_text(&self, node: Node) -> String {
        node.utf8_text(self.source).unwrap_or("").to_string()
    }

    /// Walk the tree, emitting one FunctionEntity per directive node we recognise.
    /// We accept any node whose kind ends with `_instruction` so we don't have
    /// to enumerate every directive (forward-compatible with new ones the
    /// grammar may add).
    pub fn visit_node(&mut self, node: Node) {
        let kind = node.kind();

        if kind.ends_with("_instruction") {
            self.emit_directive(node);
            // Don't descend — directives are leaves for our purposes.
            return;
        }

        // Some grammars wrap things in stage / source_file containers; just recurse.
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child);
        }
    }

    fn emit_directive(&mut self, node: Node) {
        // The directive name is `<NAME>_instruction`; convert to upper-case.
        let kind = node.kind();
        let directive_name = kind
            .strip_suffix("_instruction")
            .unwrap_or(kind)
            .to_uppercase();

        let raw = self.node_text(node);
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return;
        }

        let line_start = node.start_position().row + 1;
        let line_end = node.end_position().row + 1;

        // Body prefix == full directive text (truncated to BODY_PREFIX_MAX_CHARS).
        // The IaC scanner relies on this to match patterns like `USER root`,
        // `EXPOSE 22`, `FROM ...:latest`, `ENV API_KEY=...`, etc.
        let body_prefix = Some(truncate_body_prefix(trimmed).to_string());

        let signature = trimmed
            .lines()
            .next()
            .unwrap_or(&directive_name)
            .to_string();

        let func = FunctionEntity {
            name: directive_name.clone(),
            signature,
            visibility: "public".to_string(),
            line_start,
            line_end,
            is_async: false,
            is_test: false,
            is_static: false,
            is_abstract: false,
            parameters: Vec::new(),
            return_type: None,
            doc_comment: None,
            attributes: Vec::new(),
            parent_class: None,
            complexity: None,
            body_prefix,
        };

        self.functions.push(func);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_visit(source: &[u8]) -> DockerfileVisitor<'_> {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&crate::ts_dockerfile::language()).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut visitor = DockerfileVisitor::new(source);
        visitor.visit_node(tree.root_node());
        visitor
    }

    #[test]
    fn test_visitor_extracts_from_directive() {
        let source = b"FROM python:3.11\n";
        let visitor = parse_and_visit(source);
        assert!(
            visitor.functions.iter().any(|f| f.name == "FROM"),
            "expected a FROM directive, got: {:?}",
            visitor.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_visitor_captures_directive_body() {
        let source = b"USER root\n";
        let visitor = parse_and_visit(source);
        let user_dir = visitor
            .functions
            .iter()
            .find(|f| f.name == "USER")
            .expect("USER directive missing");
        assert!(
            user_dir.body_prefix.as_deref().unwrap_or("").contains("root"),
            "expected body to contain 'root', got {:?}",
            user_dir.body_prefix
        );
    }

    #[test]
    fn test_visitor_extracts_multiple_directives() {
        let source = b"FROM alpine:3\nUSER root\nEXPOSE 22\nEXPOSE 8080\nCMD [\"sh\"]\n";
        let visitor = parse_and_visit(source);
        let names: Vec<&str> = visitor.functions.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"FROM"), "missing FROM in {names:?}");
        assert!(names.contains(&"USER"), "missing USER in {names:?}");
        assert!(names.contains(&"EXPOSE"), "missing EXPOSE in {names:?}");
        assert!(names.contains(&"CMD"), "missing CMD in {names:?}");
        // Two EXPOSE directives expected
        let expose_count = visitor.functions.iter().filter(|f| f.name == "EXPOSE").count();
        assert_eq!(expose_count, 2, "expected two EXPOSE directives");
    }

    #[test]
    fn test_visitor_captures_secrets_in_env() {
        let source = b"ENV API_KEY=abc123\nARG SECRET=hardcoded\n";
        let visitor = parse_and_visit(source);
        let env = visitor.functions.iter().find(|f| f.name == "ENV").expect("ENV missing");
        assert!(env.body_prefix.as_deref().unwrap_or("").contains("API_KEY"));
        let arg = visitor.functions.iter().find(|f| f.name == "ARG").expect("ARG missing");
        assert!(arg.body_prefix.as_deref().unwrap_or("").contains("SECRET"));
    }
}
