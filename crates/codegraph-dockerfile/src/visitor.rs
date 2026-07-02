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
        parser
            .set_language(&crate::ts_dockerfile::language())
            .unwrap();
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
            visitor
                .functions
                .iter()
                .map(|f| &f.name)
                .collect::<Vec<_>>()
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
            user_dir
                .body_prefix
                .as_deref()
                .unwrap_or("")
                .contains("root"),
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
        let expose_count = visitor
            .functions
            .iter()
            .filter(|f| f.name == "EXPOSE")
            .count();
        assert_eq!(expose_count, 2, "expected two EXPOSE directives");
    }

    #[test]
    fn test_visitor_captures_secrets_in_env() {
        let source = b"ENV API_KEY=abc123\nARG SECRET=hardcoded\n";
        let visitor = parse_and_visit(source);
        let env = visitor
            .functions
            .iter()
            .find(|f| f.name == "ENV")
            .expect("ENV missing");
        assert!(env.body_prefix.as_deref().unwrap_or("").contains("API_KEY"));
        let arg = visitor
            .functions
            .iter()
            .find(|f| f.name == "ARG")
            .expect("ARG missing");
        assert!(arg.body_prefix.as_deref().unwrap_or("").contains("SECRET"));
    }

    #[test]
    fn test_line_numbers_are_one_indexed() {
        // FROM is on the first physical line, so line_start == line_end == 1.
        let source = b"FROM alpine:3\n";
        let visitor = parse_and_visit(source);
        let from = visitor
            .functions
            .iter()
            .find(|f| f.name == "FROM")
            .expect("FROM missing");
        assert_eq!(from.line_start, 1);
        assert_eq!(from.line_end, 1);
    }

    #[test]
    fn test_second_directive_reports_later_line() {
        // The RUN directive sits on physical line 2.
        let source = b"FROM alpine:3\nRUN echo hi\n";
        let visitor = parse_and_visit(source);
        let run = visitor
            .functions
            .iter()
            .find(|f| f.name == "RUN")
            .expect("RUN missing");
        assert_eq!(run.line_start, 2);
    }

    #[test]
    fn test_emitted_entity_uses_default_fields() {
        // Every directive is a plain public FunctionEntity with no params,
        // flags, or class - the IaC scanner only relies on name/body_prefix.
        let source = b"FROM alpine:3\n";
        let visitor = parse_and_visit(source);
        let from = &visitor.functions[0];
        assert_eq!(from.visibility, "public");
        assert!(from.parameters.is_empty());
        assert!(!from.is_async);
        assert!(!from.is_test);
        assert!(!from.is_static);
        assert!(!from.is_abstract);
        assert!(from.return_type.is_none());
        assert!(from.doc_comment.is_none());
        assert!(from.attributes.is_empty());
        assert!(from.parent_class.is_none());
        assert!(from.complexity.is_none());
    }

    #[test]
    fn test_signature_is_first_line_of_multiline_directive() {
        // A RUN with a line continuation spans two physical lines. The
        // signature keeps only the first line while body_prefix keeps both.
        let source = b"RUN apt-get update && \\\n    apt-get install -y curl\n";
        let visitor = parse_and_visit(source);
        let run = visitor
            .functions
            .iter()
            .find(|f| f.name == "RUN")
            .expect("RUN missing");
        assert!(
            !run.signature.contains('\n'),
            "signature should be a single line, got {:?}",
            run.signature
        );
        assert!(run.signature.contains("apt-get update"));
        // body_prefix retains the continuation content.
        let body = run.body_prefix.as_deref().unwrap_or("");
        assert!(body.contains("apt-get install"), "body was {body:?}");
        // The directive spans two physical lines.
        assert!(run.line_end > run.line_start);
    }

    #[test]
    fn test_latest_tag_preserved_in_body() {
        // The IaC scanner flags `:latest`; ensure the tag survives extraction.
        let source = b"FROM nginx:latest\n";
        let visitor = parse_and_visit(source);
        let from = &visitor.functions[0];
        assert!(from
            .body_prefix
            .as_deref()
            .unwrap_or("")
            .contains(":latest"));
        assert!(from.signature.contains(":latest"));
    }

    #[test]
    fn test_comment_only_source_yields_no_directives() {
        let source = b"# just a comment\n# another comment\n";
        let visitor = parse_and_visit(source);
        assert!(
            visitor.functions.is_empty(),
            "expected no directives from comments, got {:?}",
            visitor
                .functions
                .iter()
                .map(|f| &f.name)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_empty_source_yields_no_directives() {
        let visitor = parse_and_visit(b"");
        assert!(visitor.functions.is_empty());
    }

    #[test]
    fn test_copy_directive_extracted() {
        let source = b"COPY . /app\n";
        let visitor = parse_and_visit(source);
        let copy = visitor
            .functions
            .iter()
            .find(|f| f.name == "COPY")
            .expect("COPY missing");
        assert!(copy.body_prefix.as_deref().unwrap_or("").contains("/app"));
    }

    #[test]
    fn test_directive_names_are_uppercased() {
        // Directives written in lowercase still map to the uppercase name
        // derived from the `<name>_instruction` node kind.
        let source = b"from alpine:3\nrun echo hi\n";
        let visitor = parse_and_visit(source);
        let names: Vec<&str> = visitor.functions.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"FROM"), "missing FROM in {names:?}");
        assert!(names.contains(&"RUN"), "missing RUN in {names:?}");
    }

    #[test]
    fn test_directive_source_order_preserved() {
        // Directives are emitted in the order they appear in the file.
        let source = b"FROM alpine:3\nWORKDIR /app\nCOPY . .\nRUN make\nCMD [\"./app\"]\n";
        let visitor = parse_and_visit(source);
        let names: Vec<&str> = visitor.functions.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["FROM", "WORKDIR", "COPY", "RUN", "CMD"]);
    }

    #[test]
    fn test_line_numbers_increase_across_directives() {
        // Each successive directive reports a strictly larger start line.
        let source = b"FROM alpine:3\nWORKDIR /app\nRUN make\n";
        let visitor = parse_and_visit(source);
        let starts: Vec<usize> = visitor.functions.iter().map(|f| f.line_start).collect();
        assert_eq!(starts, vec![1, 2, 3]);
    }

    #[test]
    fn test_leading_blank_lines_offset_line_numbers() {
        // A blank first line pushes the FROM directive to physical line 2.
        let source = b"\nFROM alpine:3\n";
        let visitor = parse_and_visit(source);
        let from = visitor
            .functions
            .iter()
            .find(|f| f.name == "FROM")
            .expect("FROM missing");
        assert_eq!(from.line_start, 2);
        assert_eq!(from.line_end, 2);
    }

    #[test]
    fn test_multistage_from_as_stage_name_preserved() {
        // Multi-stage builds use `FROM <img> AS <stage>`; the AS clause must
        // survive so the scanner can reason about build stages.
        let source = b"FROM golang:1.22 AS builder\nFROM scratch\n";
        let visitor = parse_and_visit(source);
        let froms: Vec<&FunctionEntity> = visitor
            .functions
            .iter()
            .filter(|f| f.name == "FROM")
            .collect();
        assert_eq!(froms.len(), 2, "expected two FROM directives");
        assert!(froms[0]
            .body_prefix
            .as_deref()
            .unwrap_or("")
            .contains("AS builder"));
    }

    #[test]
    fn test_expose_port_captured_in_body() {
        // The IaC scanner flags exposed port 22; ensure it survives extraction.
        let source = b"EXPOSE 22\n";
        let visitor = parse_and_visit(source);
        let expose = &visitor.functions[0];
        assert_eq!(expose.name, "EXPOSE");
        assert!(expose.body_prefix.as_deref().unwrap_or("").contains("22"));
    }

    #[test]
    fn test_arg_without_value_extracted() {
        // A bare ARG with no default value is still emitted as a directive.
        let source = b"ARG VERSION\n";
        let visitor = parse_and_visit(source);
        let arg = visitor
            .functions
            .iter()
            .find(|f| f.name == "ARG")
            .expect("ARG missing");
        assert!(arg.body_prefix.as_deref().unwrap_or("").contains("VERSION"));
    }

    #[test]
    fn test_label_directive_extracted() {
        let source = b"LABEL maintainer=\"team@example.com\"\n";
        let visitor = parse_and_visit(source);
        let label = visitor
            .functions
            .iter()
            .find(|f| f.name == "LABEL")
            .expect("LABEL missing");
        assert!(label
            .body_prefix
            .as_deref()
            .unwrap_or("")
            .contains("maintainer"));
    }

    #[test]
    fn test_signature_equals_body_for_single_line_directive() {
        // For a one-line directive with no continuation, the single-line
        // signature and the (untruncated) body_prefix carry the same text.
        let source = b"USER 1000\n";
        let visitor = parse_and_visit(source);
        let user = &visitor.functions[0];
        assert_eq!(user.signature, user.body_prefix.as_deref().unwrap_or(""));
        assert_eq!(user.signature, "USER 1000");
    }

    #[test]
    fn test_body_prefix_truncated_to_max_chars() {
        // An oversized RUN command is truncated to exactly BODY_PREFIX_MAX_CHARS.
        use codegraph_parser_api::BODY_PREFIX_MAX_CHARS;
        let long_arg = "x".repeat(2000);
        let source = format!("RUN echo {long_arg}\n");
        let visitor = parse_and_visit(source.as_bytes());
        let run = visitor
            .functions
            .iter()
            .find(|f| f.name == "RUN")
            .expect("RUN missing");
        let body = run.body_prefix.as_deref().unwrap_or("");
        assert_eq!(body.len(), BODY_PREFIX_MAX_CHARS);
    }

    #[test]
    fn test_directive_with_inline_comment_after() {
        // A trailing comment on its own line is not attributed to the
        // preceding directive and yields no extra entity.
        let source = b"FROM alpine:3\n# trailing note\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "FROM");
    }
}
