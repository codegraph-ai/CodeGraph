// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting YAML entities

use codegraph_parser_api::{truncate_body_prefix, FunctionEntity};
use tree_sitter::Node;

pub(crate) struct YamlVisitor<'a> {
    pub source: &'a [u8],
    pub functions: Vec<FunctionEntity>,
}

impl<'a> YamlVisitor<'a> {
    pub fn new(source: &'a [u8]) -> Self {
        Self {
            source,
            functions: Vec::new(),
        }
    }

    fn node_text(&self, node: Node) -> String {
        node.utf8_text(self.source).unwrap_or("").to_string()
    }

    /// Visit the root stream node, then each document, then the top-level block_mapping.
    pub fn visit_node(&mut self, node: Node) {
        match node.kind() {
            "stream" | "document" => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.visit_node(child);
                }
            }
            "block_mapping" => {
                // Each child of a block_mapping is a block_mapping_pair
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "block_mapping_pair" {
                        self.visit_top_level_pair(child);
                    }
                }
            }
            _ => {
                // Recurse for other wrapper nodes (e.g. block_node)
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.visit_node(child);
                }
            }
        }
    }

    fn visit_top_level_pair(&mut self, node: Node) {
        // A block_mapping_pair has fields "key" and "value"
        let key_node = match node.child_by_field_name("key") {
            Some(k) => k,
            None => return,
        };

        let key_text = self.node_text(key_node).trim().to_string();
        if key_text.is_empty() {
            return;
        }

        // Use the value as the body_prefix for searchability
        let body_prefix = node
            .child_by_field_name("value")
            .and_then(|v| v.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| truncate_body_prefix(t).to_string());

        // Signature: "key: <value-preview>"
        let signature = if let Some(ref bp) = body_prefix {
            let preview: String = bp.lines().next().unwrap_or("").to_string();
            format!("{key_text}: {preview}")
        } else {
            key_text.clone()
        };

        let func = FunctionEntity {
            name: key_text,
            signature,
            visibility: "public".to_string(),
            line_start: node.start_position().row + 1,
            line_end: node.end_position().row + 1,
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

    fn parse_and_visit(source: &[u8]) -> YamlVisitor<'_> {
        use tree_sitter::Parser;

        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_yaml::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = YamlVisitor::new(source);
        visitor.visit_node(tree.root_node());
        visitor
    }

    #[test]
    fn test_visitor_top_level_keys() {
        let source = b"apiVersion: apps/v1\nkind: Deployment\n";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 2);
        assert_eq!(visitor.functions[0].name, "apiVersion");
        assert_eq!(visitor.functions[1].name, "kind");
    }

    #[test]
    fn empty_source_yields_no_functions() {
        let visitor = parse_and_visit(b"");
        assert!(visitor.functions.is_empty());
    }

    #[test]
    fn comment_only_source_yields_no_functions() {
        let visitor = parse_and_visit(b"# just a comment\n");
        assert!(visitor.functions.is_empty());
    }

    #[test]
    fn scalar_key_records_signature_with_value_preview() {
        let visitor = parse_and_visit(b"apiVersion: apps/v1\n");
        assert_eq!(visitor.functions.len(), 1);
        let f = &visitor.functions[0];
        assert_eq!(f.name, "apiVersion");
        assert_eq!(f.signature, "apiVersion: apps/v1");
        assert_eq!(f.body_prefix.as_deref(), Some("apps/v1"));
    }

    #[test]
    fn function_entity_uses_public_defaults_and_no_metadata() {
        let visitor = parse_and_visit(b"kind: Deployment\n");
        let f = &visitor.functions[0];
        assert_eq!(f.visibility, "public");
        assert!(!f.is_async);
        assert!(!f.is_test);
        assert!(!f.is_static);
        assert!(!f.is_abstract);
        assert!(f.parameters.is_empty());
        assert!(f.attributes.is_empty());
        assert!(f.return_type.is_none());
        assert!(f.doc_comment.is_none());
        assert!(f.parent_class.is_none());
        assert!(f.complexity.is_none());
    }

    #[test]
    fn line_bounds_are_one_based() {
        let visitor = parse_and_visit(b"apiVersion: apps/v1\nkind: Deployment\n");
        assert_eq!(visitor.functions[0].line_start, 1);
        assert_eq!(visitor.functions[0].line_end, 1);
        assert_eq!(visitor.functions[1].line_start, 2);
        assert_eq!(visitor.functions[1].line_end, 2);
    }

    #[test]
    fn nested_mapping_keys_are_not_extracted() {
        let source = b"metadata:\n  name: my-app\n  labels:\n    app: web\n";
        let visitor = parse_and_visit(source);
        // Only the top-level `metadata` key becomes a function.
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "metadata");
    }

    #[test]
    fn nested_value_becomes_body_prefix_and_signature_preview() {
        let source = b"metadata:\n  name: my-app\n  ns: default\n";
        let visitor = parse_and_visit(source);
        let f = &visitor.functions[0];
        // body_prefix is the whole indented value block.
        let bp = f.body_prefix.as_deref().unwrap();
        assert!(bp.contains("name: my-app"));
        // signature previews only the first line of the value.
        assert_eq!(f.signature, "metadata: name: my-app");
    }

    #[test]
    fn multi_line_value_spans_multiple_lines() {
        let source = b"spec:\n  replicas: 3\n  paused: false\n";
        let visitor = parse_and_visit(source);
        let f = &visitor.functions[0];
        assert_eq!(f.name, "spec");
        assert_eq!(f.line_start, 1);
        // The value block spans past its content lines, so line_end exceeds line_start.
        assert!(f.line_end > f.line_start);
    }

    #[test]
    fn sequence_value_records_body_prefix() {
        let source = b"items:\n  - a\n  - b\n";
        let visitor = parse_and_visit(source);
        let f = &visitor.functions[0];
        assert_eq!(f.name, "items");
        let bp = f.body_prefix.as_deref().unwrap();
        assert!(bp.contains('a'));
    }

    #[test]
    fn multi_document_stream_extracts_keys_from_each_document() {
        let source = b"kind: A\n---\nkind: B\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 2);
        assert_eq!(visitor.functions[0].signature, "kind: A");
        assert_eq!(visitor.functions[1].signature, "kind: B");
    }

    #[test]
    fn quoted_scalar_value_is_captured_in_body_prefix() {
        let visitor = parse_and_visit(b"name: \"my value\"\n");
        let f = &visitor.functions[0];
        assert_eq!(f.name, "name");
        assert!(f.body_prefix.as_deref().unwrap().contains("my value"));
    }

    #[test]
    fn key_without_value_uses_key_as_signature_and_no_body_prefix() {
        // A pair with an empty value yields no value node, so body_prefix stays None
        // and the signature falls back to the bare key.
        let visitor = parse_and_visit(b"enabled:\n");
        assert_eq!(visitor.functions.len(), 1);
        let f = &visitor.functions[0];
        assert_eq!(f.name, "enabled");
        assert_eq!(f.signature, "enabled");
        assert!(f.body_prefix.is_none());
    }

    #[test]
    fn leading_blank_lines_offset_line_numbers() {
        let visitor = parse_and_visit(b"\n\nkind: Deployment\n");
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].line_start, 3);
        assert_eq!(visitor.functions[0].line_end, 3);
    }

    #[test]
    fn numeric_value_preview_in_signature() {
        let visitor = parse_and_visit(b"replicas: 3\n");
        let f = &visitor.functions[0];
        assert_eq!(f.name, "replicas");
        assert_eq!(f.signature, "replicas: 3");
        assert_eq!(f.body_prefix.as_deref(), Some("3"));
    }

    #[test]
    fn boolean_value_preview_in_signature() {
        let visitor = parse_and_visit(b"paused: false\n");
        let f = &visitor.functions[0];
        assert_eq!(f.signature, "paused: false");
    }

    #[test]
    fn flow_mapping_value_captured_in_body_prefix() {
        let visitor = parse_and_visit(b"labels: {app: web, tier: fe}\n");
        assert_eq!(visitor.functions.len(), 1);
        let f = &visitor.functions[0];
        assert_eq!(f.name, "labels");
        assert!(f.body_prefix.as_deref().unwrap().contains("app: web"));
    }

    #[test]
    fn flow_sequence_value_captured_in_body_prefix() {
        let visitor = parse_and_visit(b"ports: [80, 443]\n");
        let f = &visitor.functions[0];
        assert_eq!(f.name, "ports");
        assert!(f.body_prefix.as_deref().unwrap().contains("80"));
    }

    #[test]
    fn comment_between_keys_does_not_break_extraction() {
        let source = b"a: 1\n# a comment\nb: 2\n";
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 2);
        assert_eq!(visitor.functions[0].name, "a");
        assert_eq!(visitor.functions[1].name, "b");
    }

    #[test]
    fn three_top_level_keys_preserve_source_order() {
        let source = b"first: 1\nsecond: 2\nthird: 3\n";
        let visitor = parse_and_visit(source);
        let names: Vec<&str> = visitor.functions.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["first", "second", "third"]);
    }

    #[test]
    fn oversized_value_body_prefix_is_truncated() {
        use codegraph_parser_api::BODY_PREFIX_MAX_CHARS;
        // A single scalar longer than the limit is truncated to exactly the max.
        let big = "x".repeat(BODY_PREFIX_MAX_CHARS * 2);
        let source = format!("key: {big}\n");
        let visitor = parse_and_visit(source.as_bytes());
        let f = &visitor.functions[0];
        assert_eq!(
            f.body_prefix.as_deref().unwrap().len(),
            BODY_PREFIX_MAX_CHARS
        );
    }

    #[test]
    fn signature_previews_only_first_line_of_flow_value() {
        // A flow mapping spanning multiple physical lines still previews only the first.
        let source = b"data: {\n  a: 1,\n  b: 2\n}\n";
        let visitor = parse_and_visit(source);
        let f = &visitor.functions[0];
        assert_eq!(f.name, "data");
        assert!(!f.signature.contains('\n'));
    }
}
