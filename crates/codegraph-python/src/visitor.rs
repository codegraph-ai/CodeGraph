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

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::{Parser, Tree};

    fn parse(source: &str) -> Tree {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .expect("load python grammar");
        parser.parse(source, None).expect("parse python source")
    }

    /// First node of the given kind found in a pre-order walk of the tree.
    fn find_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
        if node.kind() == kind {
            return Some(node);
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(found) = find_kind(child, kind) {
                return Some(found);
            }
        }
        None
    }

    /// Extract the docstring of the first function in `source`.
    fn docstring_of(source: &str) -> Option<String> {
        let tree = parse(source);
        let func = find_kind(tree.root_node(), "function_definition").expect("function node");
        let body = func.child_by_field_name("body").expect("body node");
        extract_docstring(source.as_bytes(), body)
    }

    #[test]
    fn triple_quoted_docstring_is_trimmed() {
        let src = "def f():\n    \"\"\"  spaced doc  \"\"\"\n    pass\n";
        assert_eq!(docstring_of(src).as_deref(), Some("spaced doc"));
    }

    #[test]
    fn triple_single_quoted_docstring() {
        let src = "def f():\n    '''doc3'''\n    pass\n";
        assert_eq!(docstring_of(src).as_deref(), Some("doc3"));
    }

    #[test]
    fn single_quoted_docstring() {
        let src = "def f():\n    \"one line\"\n    pass\n";
        assert_eq!(docstring_of(src).as_deref(), Some("one line"));
    }

    #[test]
    fn comment_before_docstring_is_skipped() {
        let src = "def f():\n    # a comment\n    \"\"\"real doc\"\"\"\n    pass\n";
        assert_eq!(docstring_of(src).as_deref(), Some("real doc"));
    }

    #[test]
    fn assignment_first_yields_no_docstring() {
        let src = "def f():\n    x = 1\n    return x\n";
        assert_eq!(docstring_of(src), None);
    }

    #[test]
    fn non_expression_statement_stops_search() {
        // `pass` is a pass_statement (not a comment or expression_statement),
        // so the scan breaks immediately and finds no docstring.
        let src = "def f():\n    pass\n";
        assert_eq!(docstring_of(src), None);
    }

    /// Extract decorators of the first decorated definition in `source`.
    fn decorators_of(source: &str) -> Vec<String> {
        let tree = parse(source);
        let node = find_kind(tree.root_node(), "decorated_definition").expect("decorated node");
        extract_decorators(source.as_bytes(), node)
    }

    #[test]
    fn single_decorator_is_prefixed() {
        let src = "@staticmethod\ndef f():\n    pass\n";
        assert_eq!(decorators_of(src), vec!["@staticmethod".to_string()]);
    }

    #[test]
    fn decorator_with_arguments_is_preserved() {
        let src = "@app.get(\"/users\")\ndef f():\n    pass\n";
        assert_eq!(decorators_of(src), vec!["@app.get(\"/users\")".to_string()]);
    }

    #[test]
    fn multiple_decorators_kept_in_order() {
        let src = "@staticmethod\n@app.route(\"/x\")\ndef f():\n    pass\n";
        assert_eq!(
            decorators_of(src),
            vec![
                "@staticmethod".to_string(),
                "@app.route(\"/x\")".to_string()
            ]
        );
    }

    #[test]
    fn plain_function_has_no_decorators() {
        let src = "def f():\n    pass\n";
        let tree = parse(src);
        let func = find_kind(tree.root_node(), "function_definition").expect("function node");
        assert!(extract_decorators(src.as_bytes(), func).is_empty());
    }

    #[test]
    fn empty_triple_quoted_docstring_yields_empty_string() {
        // Six quotes: the outer trim/strip leaves an empty inner slice.
        let src = "def f():\n    \"\"\"\"\"\"\n    pass\n";
        assert_eq!(docstring_of(src).as_deref(), Some(""));
    }

    #[test]
    fn only_first_docstring_string_is_returned() {
        // Two consecutive string statements: the scan returns on the first.
        let src = "def f():\n    \"first\"\n    \"second\"\n    pass\n";
        assert_eq!(docstring_of(src).as_deref(), Some("first"));
    }

    #[test]
    fn non_string_expression_does_not_stop_search() {
        // A bare call is an expression_statement with no string child, so the
        // scan does not break and still finds the following docstring.
        let src = "def f():\n    print()\n    \"doc\"\n    pass\n";
        assert_eq!(docstring_of(src).as_deref(), Some("doc"));
    }

    #[test]
    fn multiple_comments_before_docstring_are_skipped() {
        let src = "def f():\n    # one\n    # two\n    \"\"\"doc\"\"\"\n    pass\n";
        assert_eq!(docstring_of(src).as_deref(), Some("doc"));
    }

    #[test]
    fn internal_content_preserved_after_outer_trim() {
        // Only the leading/trailing whitespace is trimmed; internal newlines stay.
        let src = "def f():\n    \"\"\"line1\n    line2\"\"\"\n    pass\n";
        assert_eq!(docstring_of(src).as_deref(), Some("line1\n    line2"));
    }

    #[test]
    fn prefixed_string_statement_is_not_treated_as_docstring() {
        // A byte-string literal parses as an `expression_statement > string`, but its
        // text starts with `b"` - matching neither the triple-quote nor the single
        // `"`/`'` arms - so it falls through without yielding a docstring.
        let src = "def f():\n    b\"not a doc\"\n    pass\n";
        assert_eq!(docstring_of(src), None);
    }

    #[test]
    fn class_body_docstring_is_extracted() {
        let src = "class C:\n    \"\"\"class doc\"\"\"\n    pass\n";
        let tree = parse(src);
        let class = find_kind(tree.root_node(), "class_definition").expect("class node");
        let body = class.child_by_field_name("body").expect("body node");
        assert_eq!(
            extract_docstring(src.as_bytes(), body).as_deref(),
            Some("class doc")
        );
    }

    #[test]
    fn decorated_class_definition_decorators_extracted() {
        let src = "@dataclass\nclass C:\n    pass\n";
        assert_eq!(decorators_of(src), vec!["@dataclass".to_string()]);
    }

    #[test]
    fn dotted_decorator_without_arguments_preserved() {
        let src = "@app.route\ndef f():\n    pass\n";
        assert_eq!(decorators_of(src), vec!["@app.route".to_string()]);
    }
}
