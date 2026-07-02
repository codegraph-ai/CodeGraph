// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST visitor for extracting CSS entities

use codegraph_parser_api::{CallRelation, FunctionEntity, ImportRelation};
use tree_sitter::Node;

pub(crate) struct CssVisitor<'a> {
    pub source: &'a [u8],
    pub functions: Vec<FunctionEntity>,
    pub imports: Vec<ImportRelation>,
    pub calls: Vec<CallRelation>,
}

impl<'a> CssVisitor<'a> {
    pub fn new(source: &'a [u8]) -> Self {
        Self {
            source,
            functions: Vec::new(),
            imports: Vec::new(),
            calls: Vec::new(),
        }
    }

    fn node_text(&self, node: Node) -> String {
        node.utf8_text(self.source).unwrap_or("").to_string()
    }

    pub fn visit_node(&mut self, node: Node) {
        match node.kind() {
            "rule_set" => {
                self.visit_rule_set(node);
                // Do not recurse — rule_set is a leaf in our model
            }
            "import_statement" => {
                self.visit_import_statement(node);
            }
            "media_statement" => {
                // Recurse into @media blocks to collect nested rule_sets
                self.visit_media_statement(node);
            }
            "keyframes_statement" => {
                // Skip — keyframe blocks are animation internals, not selectors
            }
            _ => {
                let mut cursor = node.walk();
                let children: Vec<_> = node.children(&mut cursor).collect();
                drop(cursor);
                for child in children {
                    self.visit_node(child);
                }
            }
        }
    }

    fn visit_rule_set(&mut self, node: Node) {
        // tree-sitter-css uses positional children, not named fields.
        // The `selectors` node is always the first child of a rule_set.
        let mut cursor = node.walk();
        let children: Vec<_> = node.children(&mut cursor).collect();
        drop(cursor);

        let selector_node = children.iter().find(|c| c.kind() == "selectors");
        let selector = selector_node
            .map(|n| self.node_text(*n))
            .unwrap_or_default();

        let selector = selector.trim().to_string();
        if selector.is_empty() {
            return;
        }

        let block_node = children.iter().find(|c| c.kind() == "block");
        let body_prefix = block_node
            .and_then(|b| b.utf8_text(self.source).ok())
            .filter(|t| !t.is_empty())
            .map(|t| codegraph_parser_api::truncate_body_prefix(t).to_string());

        let func = FunctionEntity {
            name: selector.clone(),
            signature: selector,
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

    fn visit_import_statement(&mut self, node: Node) {
        // import_statement children: @import, then either string_value or call_expression
        let mut cursor = node.walk();
        let children: Vec<_> = node.children(&mut cursor).collect();
        drop(cursor);

        for child in &children {
            match child.kind() {
                "string_value" => {
                    // @import "path.css" — get the string_content child
                    if let Some(path) = self.extract_string_content(*child) {
                        self.push_import(path);
                    }
                    return;
                }
                "call_expression" => {
                    // @import url("path.css")
                    if let Some(path) = self.extract_url_path(*child) {
                        self.push_import(path);
                    }
                    return;
                }
                _ => {}
            }
        }
    }

    fn visit_media_statement(&mut self, node: Node) {
        // Find the block child and recurse into it to collect nested rule_sets
        let mut cursor = node.walk();
        let children: Vec<_> = node.children(&mut cursor).collect();
        drop(cursor);

        for child in children {
            if child.kind() == "block" {
                // Recurse into the block's children — rule_sets live here
                let mut block_cursor = child.walk();
                let block_children: Vec<_> = child.children(&mut block_cursor).collect();
                drop(block_cursor);
                for block_child in block_children {
                    self.visit_node(block_child);
                }
                return;
            }
        }
    }

    /// Extract the text from a `string_value` node's `string_content` child.
    fn extract_string_content(&self, node: Node) -> Option<String> {
        let mut cursor = node.walk();
        let children: Vec<_> = node.children(&mut cursor).collect();
        drop(cursor);

        for child in &children {
            if child.kind() == "string_content" {
                let text = self.node_text(*child);
                if !text.is_empty() {
                    return Some(text);
                }
            }
        }

        // Fallback: strip outer quotes from the full string_value text
        let raw = self.node_text(node);
        let trimmed = raw.trim().trim_matches('"').trim_matches('\'').to_string();
        if !trimmed.is_empty() {
            Some(trimmed)
        } else {
            None
        }
    }

    /// Extract the path from a `call_expression` that is `url(...)`.
    fn extract_url_path(&self, node: Node) -> Option<String> {
        // call_expression → arguments → string_value → string_content
        let mut cursor = node.walk();
        let children: Vec<_> = node.children(&mut cursor).collect();
        drop(cursor);

        for child in &children {
            if child.kind() == "arguments" {
                let mut arg_cursor = child.walk();
                let args: Vec<_> = child.children(&mut arg_cursor).collect();
                drop(arg_cursor);

                for arg in &args {
                    if arg.kind() == "string_value" {
                        return self.extract_string_content(*arg);
                    }
                }
            }
        }
        None
    }

    fn push_import(&mut self, path: String) {
        self.imports.push(ImportRelation {
            importer: "main".to_string(),
            imported: path,
            symbols: Vec::new(),
            is_wildcard: false,
            alias: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::Parser;

    fn parse_and_visit(source: &[u8]) -> CssVisitor<'_> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_css::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();

        let mut visitor = CssVisitor::new(source);
        visitor.visit_node(tree.root_node());
        visitor
    }

    fn dump_ast(source: &[u8]) {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_css::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();
        dump_node(tree.root_node(), source, 0);
    }

    fn dump_node(node: tree_sitter::Node, source: &[u8], depth: usize) {
        let indent = "  ".repeat(depth);
        let text = if node.child_count() == 0 {
            format!(" = {:?}", node.utf8_text(source).unwrap_or("?"))
        } else {
            String::new()
        };
        println!(
            "{}{} [{}-{}]{}",
            indent,
            node.kind(),
            node.start_position().row + 1,
            node.end_position().row + 1,
            text
        );
        let mut cursor = node.walk();
        let children: Vec<_> = node.children(&mut cursor).collect();
        drop(cursor);
        for child in children {
            dump_node(child, source, depth + 1);
        }
    }

    #[test]
    fn test_dump_css_ast() {
        let source = br#"
@import "reset.css";
@import url("variables.css");

:root {
    --primary: #333;
}

body {
    margin: 0;
}

.container {
    max-width: 1200px;
}

h1, h2 {
    color: red;
}

.btn:hover {
    background: blue;
}

@media (max-width: 768px) {
    .container {
        padding: 0;
    }
}

@keyframes fadeIn {
    from { opacity: 0; }
    to { opacity: 1; }
}
"#;
        println!("\n=== CSS AST DUMP ===");
        dump_ast(source);
        println!("===================\n");
    }

    #[test]
    fn test_visitor_rule_set_extraction() {
        let source = b".container {\n    max-width: 1200px;\n}";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert!(
            visitor.functions[0].name.contains("container"),
            "expected selector containing 'container', got: {:?}",
            visitor.functions[0].name
        );
    }

    #[test]
    fn test_visitor_import_string_extraction() {
        let source = b"@import \"reset.css\";";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "reset.css");
    }

    #[test]
    fn test_visitor_import_url_extraction() {
        let source = b"@import url(\"variables.css\");";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "variables.css");
    }

    #[test]
    fn test_visitor_media_nested_rules() {
        let source = br#"
@media (max-width: 768px) {
    .container {
        padding: 0;
    }
    body {
        font-size: 14px;
    }
}
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(
            visitor.functions.len(),
            2,
            "expected 2 nested rules in @media, got: {:?}",
            visitor
                .functions
                .iter()
                .map(|f| &f.name)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_visitor_keyframes_skipped() {
        let source = br#"
@keyframes fadeIn {
    from { opacity: 0; }
    to { opacity: 1; }
}
"#;
        let visitor = parse_and_visit(source);
        // keyframe blocks are not extracted as selectors
        assert_eq!(visitor.functions.len(), 0);
    }

    #[test]
    fn test_rule_set_metadata_fields() {
        let source = b".container {\n    max-width: 1200px;\n}";
        let visitor = parse_and_visit(source);

        let func = &visitor.functions[0];
        // signature mirrors the selector text and the name
        assert_eq!(func.name, ".container");
        assert_eq!(func.signature, ".container");
        assert_eq!(func.visibility, "public");
        // 1-based line bounds: rule spans row 0..=2
        assert_eq!(func.line_start, 1);
        assert_eq!(func.line_end, 3);
        // CSS rules carry no complexity/return/doc/parent metadata
        assert!(func.complexity.is_none());
        assert!(func.return_type.is_none());
        assert!(func.doc_comment.is_none());
        assert!(func.parent_class.is_none());
        assert!(func.parameters.is_empty());
        assert!(func.attributes.is_empty());
        assert!(!func.is_async && !func.is_test && !func.is_static && !func.is_abstract);
    }

    #[test]
    fn test_rule_set_body_prefix_captured() {
        let source = b".container {\n    max-width: 1200px;\n}";
        let visitor = parse_and_visit(source);

        let body = visitor.functions[0]
            .body_prefix
            .as_ref()
            .expect("expected body_prefix from the block");
        // body_prefix is the block text (braces included)
        assert!(body.starts_with('{'), "got: {body:?}");
        assert!(body.contains("max-width"), "got: {body:?}");
    }

    #[test]
    fn test_multiple_top_level_rules() {
        let source = b"body {\n    margin: 0;\n}\n.btn {\n    color: red;\n}";
        let visitor = parse_and_visit(source);

        let names: Vec<_> = visitor.functions.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["body", ".btn"]);
    }

    #[test]
    fn test_grouped_selectors_kept_as_single_rule() {
        let source = b"h1, h2 {\n    color: red;\n}";
        let visitor = parse_and_visit(source);

        // A comma-grouped selector list is one rule_set, kept verbatim (trimmed)
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "h1, h2");
    }

    #[test]
    fn test_pseudo_class_selector_preserved() {
        let source = b".btn:hover {\n    background: blue;\n}";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, ".btn:hover");
    }

    #[test]
    fn test_root_pseudo_selector() {
        let source = b":root {\n    --primary: #333;\n}";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, ":root");
    }

    #[test]
    fn test_import_string_single_quotes() {
        let source = b"@import 'reset.css';";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "reset.css");
    }

    #[test]
    fn test_import_url_single_quotes() {
        let source = b"@import url('variables.css');";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "variables.css");
    }

    #[test]
    fn test_import_relation_fields() {
        let source = b"@import \"reset.css\";";
        let visitor = parse_and_visit(source);

        let imp = &visitor.imports[0];
        assert_eq!(imp.importer, "main");
        assert!(imp.symbols.is_empty());
        assert!(!imp.is_wildcard);
        assert!(imp.alias.is_none());
    }

    #[test]
    fn test_mixed_imports_and_rules() {
        let source = br#"
@import "reset.css";
@import url("vars.css");
body {
    margin: 0;
}
.container {
    max-width: 1200px;
}
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.imports.len(), 2);
        assert_eq!(visitor.imports[0].imported, "reset.css");
        assert_eq!(visitor.imports[1].imported, "vars.css");
        assert_eq!(visitor.functions.len(), 2);
    }

    #[test]
    fn test_keyframes_skipped_but_sibling_rule_kept() {
        let source = br#"
@keyframes fadeIn {
    from { opacity: 0; }
    to { opacity: 1; }
}
.box {
    color: red;
}
"#;
        let visitor = parse_and_visit(source);
        // keyframe inner blocks are skipped; the following top-level rule is kept
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, ".box");
    }

    #[test]
    fn test_media_line_bounds_of_nested_rule() {
        let source = br#"@media (max-width: 768px) {
    .container {
        padding: 0;
    }
}
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 1);
        // the nested rule starts on row 1 (1-based line 2), not the @media line
        assert_eq!(visitor.functions[0].line_start, 2);
        assert_eq!(visitor.functions[0].name, ".container");
    }

    #[test]
    fn test_empty_source_yields_nothing() {
        let visitor = parse_and_visit(b"");
        assert!(visitor.functions.is_empty());
        assert!(visitor.imports.is_empty());
        assert!(visitor.calls.is_empty());
    }

    #[test]
    fn test_comment_only_source_yields_nothing() {
        let visitor = parse_and_visit(b"/* just a comment */");
        assert!(visitor.functions.is_empty());
        assert!(visitor.imports.is_empty());
    }

    #[test]
    fn test_id_selector_preserved() {
        let source = b"#header {\n    height: 60px;\n}";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "#header");
    }

    #[test]
    fn test_attribute_selector_preserved() {
        let source = b"input[type=\"text\"] {\n    border: 1px;\n}";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "input[type=\"text\"]");
    }

    #[test]
    fn test_universal_selector_preserved() {
        let source = b"* {\n    margin: 0;\n}";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "*");
    }

    #[test]
    fn test_descendant_combinator_selector_kept_verbatim() {
        let source = b".nav a {\n    color: blue;\n}";
        let visitor = parse_and_visit(source);

        // A descendant combinator is one selector node, whitespace preserved between parts
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, ".nav a");
    }

    #[test]
    fn test_child_combinator_selector_kept_verbatim() {
        let source = b".nav > a {\n    color: blue;\n}";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, ".nav > a");
    }

    #[test]
    fn test_two_media_blocks_each_contribute_a_rule() {
        let source = br#"
@media (max-width: 768px) {
    .a { padding: 0; }
}
@media (min-width: 769px) {
    .b { padding: 8px; }
}
"#;
        let visitor = parse_and_visit(source);
        let names: Vec<_> = visitor.functions.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec![".a", ".b"]);
    }

    #[test]
    fn test_import_before_keyframes_recorded() {
        let source = br#"
@import "reset.css";
@keyframes spin {
    from { transform: rotate(0); }
    to { transform: rotate(360deg); }
}
"#;
        let visitor = parse_and_visit(source);
        // the import is kept; the keyframes block contributes no functions
        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "reset.css");
        assert!(visitor.functions.is_empty());
    }

    #[test]
    fn test_import_order_preserved_across_forms() {
        let source = br#"
@import "a.css";
@import url("b.css");
@import 'c.css';
"#;
        let visitor = parse_and_visit(source);
        let paths: Vec<_> = visitor
            .imports
            .iter()
            .map(|i| i.imported.as_str())
            .collect();
        assert_eq!(paths, vec!["a.css", "b.css", "c.css"]);
    }

    #[test]
    fn test_body_prefix_truncated_to_max_chars() {
        // Build a block whose text exceeds BODY_PREFIX_MAX_CHARS (1024 bytes)
        let mut source = Vec::from(&b".big {\n"[..]);
        for _ in 0..200 {
            source.extend_from_slice(b"    color: red;\n");
        }
        source.extend_from_slice(b"}");
        let visitor = parse_and_visit(&source);

        let body = visitor.functions[0]
            .body_prefix
            .as_ref()
            .expect("expected body_prefix");
        assert_eq!(
            body.len(),
            codegraph_parser_api::BODY_PREFIX_MAX_CHARS,
            "body_prefix should be truncated to the max byte length"
        );
    }

    #[test]
    fn test_media_query_condition_not_extracted_as_rule() {
        // The @media condition itself must never become a function; only the
        // nested rule_set does.
        let source = br#"@media screen and (max-width: 768px) {
    .container { padding: 0; }
}
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, ".container");
    }

    #[test]
    fn test_unquoted_url_import_is_dropped() {
        // @import url(foo.css); — the argument parses as a plain_value, not a
        // string_value, so extract_url_path finds no path and no import is pushed.
        let source = b"@import url(foo.css);";
        let visitor = parse_and_visit(source);

        assert!(
            visitor.imports.is_empty(),
            "unquoted url() argument should yield no import, got: {:?}",
            visitor.imports
        );
    }

    #[test]
    fn test_signature_equals_selector_name() {
        let source = b".btn:hover {\n    background: blue;\n}";
        let visitor = parse_and_visit(source);

        let f = &visitor.functions[0];
        assert_eq!(f.name, ".btn:hover");
        assert_eq!(f.signature, f.name);
    }

    #[test]
    fn test_rule_line_end_spans_full_block() {
        let source = b".card {\n    color: red;\n    padding: 0;\n}";
        let visitor = parse_and_visit(source);

        let f = &visitor.functions[0];
        assert_eq!(f.line_start, 1);
        // closing brace is on the 4th line
        assert_eq!(f.line_end, 4);
    }

    #[test]
    fn test_grouped_selector_kept_as_single_rule() {
        let source = b"h1, h2 {\n    color: red;\n}";
        let visitor = parse_and_visit(source);

        // A comma-grouped selector is one `selectors` node → one function, verbatim.
        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, "h1, h2");
    }

    #[test]
    fn test_body_prefix_contains_block_text() {
        let source = b".box {\n    color: red;\n}";
        let visitor = parse_and_visit(source);

        let body = visitor.functions[0]
            .body_prefix
            .as_ref()
            .expect("expected body_prefix");
        // Short blocks are not truncated: the braces and declaration are present.
        assert!(body.contains("color: red"));
        assert!(body.starts_with('{'));
    }

    #[test]
    fn test_pseudo_element_selector_preserved() {
        let source = b".tooltip::before {\n    content: \"\";\n}";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, ".tooltip::before");
    }

    #[test]
    fn test_empty_block_rule_still_extracted() {
        let source = b".empty {\n}";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, ".empty");
        // an empty `{}` block still yields a body_prefix of the braces
        assert!(visitor.functions[0].body_prefix.is_some());
    }

    #[test]
    fn test_nested_rule_inside_rule_set_not_extracted() {
        // rule_set is treated as a leaf (no recursion), so a CSS-nested rule
        // inside another rule_set is dropped — only the outer selector is kept.
        let source = b".card {\n    color: red;\n    .inner { color: blue; }\n}";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.functions.len(), 1);
        assert_eq!(visitor.functions[0].name, ".card");
    }

    #[test]
    fn test_import_with_trailing_media_query_records_path() {
        // @import "x.css" screen; — the string_value is matched first and the
        // trailing media query is ignored.
        let source = b"@import \"print.css\" print;";
        let visitor = parse_and_visit(source);

        assert_eq!(visitor.imports.len(), 1);
        assert_eq!(visitor.imports[0].imported, "print.css");
    }

    #[test]
    fn test_media_rule_body_prefix_and_line_end() {
        let source = br#"@media (max-width: 768px) {
    .container {
        padding: 0;
        margin: 0;
    }
}
"#;
        let visitor = parse_and_visit(source);
        assert_eq!(visitor.functions.len(), 1);
        let f = &visitor.functions[0];
        // nested rule spans lines 2..5 (1-based)
        assert_eq!(f.line_start, 2);
        assert_eq!(f.line_end, 5);
        assert!(f.body_prefix.as_ref().unwrap().contains("padding: 0"));
    }

    #[test]
    fn test_multiple_top_level_rules_source_order() {
        let source = br#"a { color: red; }
b { color: green; }
c { color: blue; }
"#;
        let visitor = parse_and_visit(source);
        let names: Vec<_> = visitor.functions.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b", "c"]);
    }
}
