// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting TOML entities
//!
//! tree-sitter-toml-ng node types (from grammar):
//!   document          — root; children are pairs, tables, comments at top level
//!   table             — `[section]` header line only (pairs follow as siblings)
//!   table_array_element — `[[section]]` header line only
//!   pair              — key = value  (child of document, table, or table_array_element)
//!   dotted_key        — a.b.c style key
//!   bare_key          — unquoted identifier
//!   quoted_key        — "quoted" key
//!   string / integer / float / boolean / array / inline_table — value types
//!   comment           — # ...
//!
//! Mapping:
//!   `[table]` / `[[array-of-tables]]` → ClassEntity (makes sections searchable)
//!   `key = value` pairs               → FunctionEntity (property proxy)

use codegraph_parser_api::{ClassEntity, FunctionEntity};
use tree_sitter::Node;

pub struct TomlVisitor<'a> {
    pub source: &'a [u8],
    /// Table / array-of-tables sections
    pub classes: Vec<ClassEntity>,
    /// Key-value pairs as property proxies
    pub functions: Vec<FunctionEntity>,
    /// Currently active section name
    current_section: Option<String>,
    current_section_start: usize,
    current_section_end: usize,
}

impl<'a> TomlVisitor<'a> {
    pub fn new(source: &'a [u8]) -> Self {
        Self {
            source,
            classes: Vec::new(),
            functions: Vec::new(),
            current_section: None,
            current_section_start: 0,
            current_section_end: 0,
        }
    }

    fn node_text(&self, node: Node) -> String {
        node.utf8_text(self.source).unwrap_or("").to_string()
    }

    /// Visit the root document node.
    ///
    /// In tree-sitter-toml-ng the document's direct children are:
    ///   - `pair` nodes (top-level key-value pairs)
    ///   - `table` nodes (just the `[name]` header; pairs follow as siblings)
    ///   - `table_array_element` nodes (`[[name]]`)
    ///   - `comment` nodes
    pub fn visit_document(&mut self, node: Node) {
        let children: Vec<Node> = {
            let mut cursor = node.walk();
            node.children(&mut cursor).collect()
        };

        for child in &children {
            match child.kind() {
                "table" => self.start_table(*child),
                "table_array_element" => self.start_table_array(*child),
                "pair" => {
                    let end = child.end_position().row + 1;
                    // Update section end if this pair extends it
                    if self.current_section.is_some() && end > self.current_section_end {
                        self.current_section_end = end;
                    }
                    let section = self.current_section.clone();
                    self.visit_pair(*child, section);
                }
                _ => {} // comment, newline, ERROR, etc.
            }
        }

        // Flush the final section
        let doc_end = node.end_position().row + 1;
        self.flush_section(doc_end);
    }

    /// Begin a `[section]` table.
    fn start_table(&mut self, node: Node) {
        let start_line = node.start_position().row + 1;
        // Flush the previous section
        self.flush_section(start_line.saturating_sub(1));

        let name = self.extract_section_name(node);
        self.current_section = Some(name);
        self.current_section_start = start_line;
        self.current_section_end = start_line;
    }

    /// Begin a `[[section]]` array-of-tables.
    fn start_table_array(&mut self, node: Node) {
        let start_line = node.start_position().row + 1;
        self.flush_section(start_line.saturating_sub(1));

        let name = self.extract_section_name(node);
        self.current_section = Some(name);
        self.current_section_start = start_line;
        self.current_section_end = start_line;
    }

    /// Emit the current section as a ClassEntity and reset state.
    fn flush_section(&mut self, end_line: usize) {
        if let Some(name) = self.current_section.take() {
            let actual_end = self.current_section_end.max(end_line);
            let mut class = ClassEntity::new(&name, self.current_section_start, actual_end);
            class.visibility = "public".to_string();
            self.classes.push(class);
            self.current_section_start = 0;
            self.current_section_end = 0;
        }
    }

    /// Visit a `key = value` pair.
    fn visit_pair(&mut self, node: Node, parent_section: Option<String>) {
        let start_line = node.start_position().row + 1;
        let end_line = node.end_position().row + 1;

        // Collect children to find key and value
        let children: Vec<Node> = {
            let mut cursor = node.walk();
            node.children(&mut cursor).collect()
        };

        // First non-comment, non-punctuation child is the key
        let key_name = children
            .iter()
            .find(|c| matches!(c.kind(), "bare_key" | "quoted_key" | "dotted_key" | "key"))
            .map(|k| self.node_text(*k))
            .unwrap_or_else(|| {
                // Fall back to first named child
                node.named_child(0)
                    .map(|c| self.node_text(c))
                    .unwrap_or_else(|| "unknown".to_string())
            });

        // Last named child is typically the value
        let value_text = {
            let named_count = node.named_child_count();
            if named_count >= 2 {
                node.named_child(named_count - 1)
                    .map(|v| {
                        let t = self.node_text(v);
                        if t.len() > 120 {
                            format!("{}...", &t[..120])
                        } else {
                            t
                        }
                    })
                    .unwrap_or_default()
            } else {
                String::new()
            }
        };

        let full_name = if let Some(ref section) = parent_section {
            format!("{section}.{key_name}")
        } else {
            key_name.clone()
        };

        let signature = if value_text.is_empty() {
            key_name.clone()
        } else {
            format!("{key_name} = {value_text}")
        };

        let mut func = FunctionEntity::new(&full_name, start_line, end_line);
        func.signature = signature;
        func.visibility = "public".to_string();
        func.parent_class = parent_section;

        self.functions.push(func);
    }

    /// Extract the section name from a table / table_array_element node.
    fn extract_section_name(&self, node: Node) -> String {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "dotted_key" | "quoted_key" | "bare_key" | "key" => {
                    return self.node_text(child);
                }
                _ => {}
            }
        }
        // Fallback: strip brackets from raw text
        self.node_text(node)
            .trim_matches(|c| c == '[' || c == ']' || c == ' ' || c == '\n')
            .to_string()
    }
}
