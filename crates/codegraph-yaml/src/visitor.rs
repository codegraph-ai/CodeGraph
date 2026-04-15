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
}
