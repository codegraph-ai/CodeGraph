// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting TOML entities
//!
//! tree-sitter-toml-ng node types (from grammar):
//!   document          — root; children are top-level pairs, tables, comments
//!   table             — `[section]` header PLUS its `pair` children nested inside
//!   table_array_element — `[[section]]` header PLUS its `pair` children nested inside
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
    ///   - `table` nodes (the `[name]` header with its pairs nested inside)
    ///   - `table_array_element` nodes (`[[name]]`, pairs nested inside)
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
        // tree-sitter-toml-ng nests each section's `pair` nodes *under* the
        // `table` node rather than as document siblings, so descend here.
        self.visit_nested_pairs(node);
    }

    /// Begin a `[[section]]` array-of-tables.
    fn start_table_array(&mut self, node: Node) {
        let start_line = node.start_position().row + 1;
        self.flush_section(start_line.saturating_sub(1));

        let name = self.extract_section_name(node);
        self.current_section = Some(name);
        self.current_section_start = start_line;
        self.current_section_end = start_line;
        self.visit_nested_pairs(node);
    }

    /// Visit `pair` children nested directly under a table /
    /// table_array_element node (the tree-sitter-toml-ng layout).
    fn visit_nested_pairs(&mut self, node: Node) {
        let children: Vec<Node> = {
            let mut cursor = node.walk();
            node.children(&mut cursor).collect()
        };
        for child in &children {
            if child.kind() == "pair" {
                let end = child.end_position().row + 1;
                if end > self.current_section_end {
                    self.current_section_end = end;
                }
                let section = self.current_section.clone();
                self.visit_pair(*child, section);
            }
        }
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
                            // UTF-8-safe truncation — see GitHub issue #3.
                            format!(
                                "{}...",
                                codegraph_parser_api::truncate_at_char_boundary(&t, 120)
                            )
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

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::Parser;

    /// Parse TOML source and run the visitor over the document root,
    /// returning the populated visitor for assertions.
    fn visit(source: &str) -> TomlVisitor<'_> {
        let mut parser = Parser::new();
        let language = crate::ts_toml::language();
        parser.set_language(&language).expect("set toml language");
        let tree = parser.parse(source, None).expect("parse toml");
        let mut visitor = TomlVisitor::new(source.as_bytes());
        visitor.visit_document(tree.root_node());
        // Detach the borrow lifetime issue by returning the visitor; the
        // caller holds `source` alive for the returned visitor's lifetime.
        visitor
    }

    #[test]
    fn top_level_pair_has_no_parent_and_bare_signature() {
        let src = "name = \"my-project\"\n";
        let v = visit(src);
        assert_eq!(v.classes.len(), 0, "no [table] header means no class");
        assert_eq!(v.functions.len(), 1);
        let f = &v.functions[0];
        assert_eq!(f.name, "name");
        assert_eq!(f.parent_class, None);
        assert_eq!(f.visibility, "public");
        assert_eq!(f.signature, "name = \"my-project\"");
        assert_eq!(f.line_start, 1);
    }

    #[test]
    fn pair_inside_table_is_prefixed_and_parented() {
        let src = "[package]\nname = \"codegraph\"\n";
        let v = visit(src);
        assert_eq!(v.classes.len(), 1);
        assert_eq!(v.classes[0].name, "package");
        assert_eq!(v.classes[0].visibility, "public");
        let f = v
            .functions
            .iter()
            .find(|f| f.name == "package.name")
            .expect("prefixed key");
        assert_eq!(f.parent_class.as_deref(), Some("package"));
        assert_eq!(f.signature, "name = \"codegraph\"");
    }

    #[test]
    fn section_line_range_spans_its_pairs() {
        // `[package]` on line 1, last pair on line 3 -> class end >= 3.
        let src = "[package]\nname = \"a\"\nversion = \"0.1.0\"\n";
        let v = visit(src);
        assert_eq!(v.classes.len(), 1);
        let c = &v.classes[0];
        assert_eq!(c.line_start, 1);
        assert!(
            c.line_end >= 3,
            "section end should extend to last pair, got {}",
            c.line_end
        );
    }

    #[test]
    fn array_of_tables_flushes_each_header() {
        let src = "[[bin]]\nname = \"server\"\n\n[[bin]]\nname = \"client\"\n";
        let v = visit(src);
        assert_eq!(v.classes.len(), 2);
        assert!(v.classes.iter().all(|c| c.name == "bin"));
    }

    #[test]
    fn dotted_key_is_captured_as_name() {
        let src = "a.b.c = 1\n";
        let v = visit(src);
        assert_eq!(v.functions.len(), 1);
        assert_eq!(v.functions[0].name, "a.b.c");
    }

    #[test]
    fn quoted_key_text_is_preserved() {
        let src = "\"quoted key\" = true\n";
        let v = visit(src);
        assert_eq!(v.functions.len(), 1);
        assert!(
            v.functions[0].name.contains("quoted key"),
            "quoted key should be preserved verbatim, got {}",
            v.functions[0].name
        );
    }

    #[test]
    fn long_value_is_truncated_with_ellipsis() {
        let long = "x".repeat(200);
        let src = format!("key = \"{long}\"\n");
        let v = visit(&src);
        assert_eq!(v.functions.len(), 1);
        let sig = &v.functions[0].signature;
        assert!(
            sig.ends_with("..."),
            "over-long value should be truncated: {sig}"
        );
        // 120-char cap + `...` + `key = ` prefix; well under the raw 200-char value.
        assert!(
            sig.len() < 200,
            "truncated signature should be shorter than raw value"
        );
    }

    #[test]
    fn multiple_top_level_pairs_each_emit_a_function() {
        let src = "name = \"a\"\nversion = \"0.1.0\"\nedition = \"2021\"\n";
        let v = visit(src);
        assert_eq!(v.functions.len(), 3);
        let names: Vec<&str> = v.functions.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"name"));
        assert!(names.contains(&"version"));
        assert!(names.contains(&"edition"));
    }

    #[test]
    fn pair_line_numbers_are_one_indexed_with_leading_blank_lines() {
        // Two blank lines, then the pair on physical line 3.
        let src = "\n\nname = \"a\"\n";
        let v = visit(src);
        assert_eq!(v.functions.len(), 1);
        assert_eq!(v.functions[0].line_start, 3);
        assert_eq!(v.functions[0].line_end, 3);
    }

    #[test]
    fn two_distinct_sections_each_emit_a_class() {
        let src = "[a]\nx = 1\n\n[b]\ny = 2\n";
        let v = visit(src);
        assert_eq!(v.classes.len(), 2);
        let names: Vec<&str> = v.classes.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"a"));
        assert!(names.contains(&"b"));
    }

    #[test]
    fn pairs_are_attributed_to_their_own_section() {
        let src = "[a]\nx = 1\n[b]\ny = 2\n";
        let v = visit(src);
        let fx = v.functions.iter().find(|f| f.name == "a.x").expect("a.x");
        let fy = v.functions.iter().find(|f| f.name == "b.y").expect("b.y");
        assert_eq!(fx.parent_class.as_deref(), Some("a"));
        assert_eq!(fy.parent_class.as_deref(), Some("b"));
    }

    #[test]
    fn empty_section_still_emits_a_class() {
        // `[empty]` header on line 1 with no pairs; the next header flushes it.
        let src = "[empty]\n[next]\nk = 1\n";
        let v = visit(src);
        let empty = v
            .classes
            .iter()
            .find(|c| c.name == "empty")
            .expect("empty section class");
        assert_eq!(empty.line_start, 1);
        assert_eq!(empty.line_end, 1);
        // No pair belongs to the empty section.
        assert!(v
            .functions
            .iter()
            .all(|f| f.parent_class.as_deref() != Some("empty")));
    }

    #[test]
    fn inline_table_value_is_captured_in_signature() {
        let src = "point = { x = 1, y = 2 }\n";
        let v = visit(src);
        assert_eq!(v.functions.len(), 1);
        let sig = &v.functions[0].signature;
        assert!(sig.starts_with("point = {"), "inline table sig: {sig}");
        assert!(sig.contains("x = 1"));
    }

    #[test]
    fn array_value_is_captured_in_signature() {
        let src = "ports = [8000, 8001, 8002]\n";
        let v = visit(src);
        assert_eq!(v.functions.len(), 1);
        assert_eq!(v.functions[0].signature, "ports = [8000, 8001, 8002]");
    }

    #[test]
    fn scalar_value_types_are_preserved_verbatim() {
        let src = "port = 8080\nratio = 1.5\nenabled = true\n";
        let v = visit(src);
        let port = v.functions.iter().find(|f| f.name == "port").unwrap();
        let ratio = v.functions.iter().find(|f| f.name == "ratio").unwrap();
        let enabled = v.functions.iter().find(|f| f.name == "enabled").unwrap();
        assert_eq!(port.signature, "port = 8080");
        assert_eq!(ratio.signature, "ratio = 1.5");
        assert_eq!(enabled.signature, "enabled = true");
    }

    #[test]
    fn value_exactly_at_cap_is_not_truncated() {
        // A string value whose full text (quotes included) is exactly 120 chars
        // must NOT get an ellipsis — truncation triggers only above 120.
        let inner = "x".repeat(118); // "..." + 118 + "..." quotes = 120 chars
        let src = format!("key = \"{inner}\"\n");
        let v = visit(&src);
        assert_eq!(v.functions.len(), 1);
        let sig = &v.functions[0].signature;
        assert!(
            !sig.ends_with("..."),
            "exactly-120-char value should not be truncated: len {}",
            sig.len()
        );
    }

    #[test]
    fn comment_lines_do_not_emit_entities() {
        let src = "# a header comment\nname = \"a\"\n# trailing comment\n";
        let v = visit(src);
        assert_eq!(v.functions.len(), 1, "only the pair, not the comments");
        assert_eq!(v.functions[0].name, "name");
    }

    #[test]
    fn top_level_pair_before_a_table_is_not_parented() {
        // `version` precedes any `[table]`, so it stays section-less; the pair
        // after the header is parented.
        let src = "version = \"0.1.0\"\n[deps]\nserde = \"1\"\n";
        let v = visit(src);
        let version = v.functions.iter().find(|f| f.name == "version").unwrap();
        assert_eq!(version.parent_class, None);
        let serde = v.functions.iter().find(|f| f.name == "deps.serde").unwrap();
        assert_eq!(serde.parent_class.as_deref(), Some("deps"));
    }

    #[test]
    fn array_of_tables_pairs_are_prefixed_with_section() {
        let src = "[[bin]]\nname = \"server\"\n";
        let v = visit(src);
        let f = v
            .functions
            .iter()
            .find(|f| f.name == "bin.name")
            .expect("prefixed array-of-tables key");
        assert_eq!(f.parent_class.as_deref(), Some("bin"));
        assert_eq!(f.signature, "name = \"server\"");
    }

    #[test]
    fn dotted_section_name_is_preserved_and_prefixes_keys() {
        let src = "[tool.black]\nline-length = 88\n";
        let v = visit(src);
        assert_eq!(v.classes.len(), 1);
        assert_eq!(v.classes[0].name, "tool.black");
        let f = v
            .functions
            .iter()
            .find(|f| f.name == "tool.black.line-length")
            .expect("dotted section prefix");
        assert_eq!(f.parent_class.as_deref(), Some("tool.black"));
    }
}
