// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting Python entities using tree-sitter
//!
//! This module implements a tree-sitter based visitor that walks the Python AST
//! and extracts functions, classes, and their relationships.

use tree_sitter::Node;

/// Extract the first docstring from a block node
pub fn extract_docstring(source: &[u8], node: Node) -> Option<String> {
    // Look for the first expression_statement that contains a string
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "expression_statement" {
            let mut expr_cursor = child.walk();
            for expr_child in child.children(&mut expr_cursor) {
                if expr_child.kind() == "string" {
                    let text = expr_child.utf8_text(source).unwrap_or("");
                    // Remove quotes
                    let text = text.trim();
                    if text.starts_with("\"\"\"") || text.starts_with("'''") {
                        let inner = &text[3..text.len().saturating_sub(3)];
                        return Some(inner.trim().to_string());
                    } else if text.starts_with('"') || text.starts_with('\'') {
                        let inner = &text[1..text.len().saturating_sub(1)];
                        return Some(inner.trim().to_string());
                    }
                }
            }
        } else if child.kind() != "comment" {
            // Stop looking after non-docstring statements
            break;
        }
    }
    None
}

/// Extract decorator names from a decorated definition
pub fn extract_decorators(source: &[u8], node: Node) -> Vec<String> {
    let mut decorators = Vec::new();
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        if child.kind() == "decorator" {
            let text = child.utf8_text(source).unwrap_or("").trim();
            // Preserve full decorator text including arguments for route detection.
            // e.g., "@app.get(\"/users\")" stays as-is rather than truncating to "@app.get"
            let name = text.trim_start_matches('@').trim();
            decorators.push(format!("@{name}"));
        }
    }

    decorators
}
