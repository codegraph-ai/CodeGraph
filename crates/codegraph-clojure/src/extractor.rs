// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST extraction for Clojure source code

use codegraph_parser_api::{CodeIR, ModuleEntity, ParserConfig, ParserError};
use std::path::Path;
use tree_sitter::Parser;

use crate::visitor::ClojureVisitor;

/// Extract code entities and relationships from Clojure source code
pub(crate) fn extract(
    source: &str,
    file_path: &Path,
    _config: &ParserConfig,
) -> Result<CodeIR, ParserError> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_clojure::LANGUAGE.into())
        .map_err(|e| ParserError::ParseError(file_path.to_path_buf(), e.to_string()))?;

    let tree = parser.parse(source, None).ok_or_else(|| {
        ParserError::ParseError(file_path.to_path_buf(), "Failed to parse".to_string())
    })?;

    let root_node = tree.root_node();

    let mut ir = CodeIR::new(file_path.to_path_buf());

    let module_name = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();
    ir.module = Some(ModuleEntity {
        name: module_name,
        path: file_path.display().to_string(),
        language: "clojure".to_string(),
        line_count: source.lines().count(),
        doc_comment: None,
        attributes: Vec::new(),
    });

    let mut visitor = ClojureVisitor::new(source.as_bytes());
    visitor.visit_node(root_node);

    ir.functions = visitor.functions;
    ir.classes = visitor.classes;
    ir.imports = visitor.imports;
    ir.calls = visitor.calls;

    Ok(ir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extract_ok(source: &str, path: &str) -> CodeIR {
        extract(source, Path::new(path), &ParserConfig::default()).expect("extract should succeed")
    }

    #[test]
    fn test_extract_defn() {
        let ir = extract_ok("(defn hello [] (println \"Hello, world!\"))", "test.clj");
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].name, "hello");
    }

    #[test]
    fn test_extract_ns_require() {
        let ir = extract_ok(
            "(ns my.app (:require [clojure.string :as str]))",
            "test.clj",
        );
        assert!(ir.imports.iter().any(|i| i.imported == "clojure.string"));
    }

    #[test]
    fn test_module_name_from_file_stem() {
        let ir = extract_ok("(defn f [] 1)", "src/core.clj");
        let module = ir.module.expect("module should be set");
        assert_eq!(module.name, "core");
        assert_eq!(module.language, "clojure");
    }

    #[test]
    fn test_module_path_and_line_count() {
        let source = "(defn a [] 1)\n(defn b [] 2)\n(defn c [] 3)";
        let ir = extract_ok(source, "app/util.clj");
        let module = ir.module.expect("module should be set");
        assert_eq!(module.path, "app/util.clj");
        assert_eq!(module.line_count, source.lines().count());
        assert!(module.doc_comment.is_none());
        assert!(module.attributes.is_empty());
    }

    #[test]
    fn test_module_name_unknown_without_stem() {
        // A path with no file stem falls back to "unknown".
        let ir = extract_ok("(defn f [] 1)", "..");
        assert_eq!(ir.module.expect("module").name, "unknown");
    }

    #[test]
    fn test_extract_defprotocol_as_class() {
        let ir = extract_ok("(defprotocol Animal (speak [this]))", "test.clj");
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "Animal");
        assert!(ir.classes[0].is_interface);
    }

    #[test]
    fn test_extract_defrecord_as_class() {
        let ir = extract_ok("(defrecord Dog [name breed])", "test.clj");
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "Dog");
        assert!(!ir.classes[0].is_interface);
    }

    #[test]
    fn test_extract_calls_populated() {
        let ir = extract_ok("(defn f [x] (helper x))", "test.clj");
        assert!(ir
            .calls
            .iter()
            .any(|c| c.caller == "f" && c.callee == "helper"));
    }

    #[test]
    fn test_extract_mixed_entities() {
        let source = "(ns my.app (:require [clojure.set :as set]))\n\
             (defprotocol Shape (area [this]))\n\
             (defn square [s] (* s s))";
        let ir = extract_ok(source, "shapes.clj");
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.classes.len(), 1);
        assert!(ir.imports.iter().any(|i| i.imported == "clojure.set"));
    }

    #[test]
    fn test_empty_source_yields_only_module() {
        let ir = extract_ok("", "empty.clj");
        assert!(ir.functions.is_empty());
        assert!(ir.classes.is_empty());
        assert!(ir.imports.is_empty());
        assert!(ir.module.is_some());
    }

    #[test]
    fn test_comment_only_source_yields_no_entities() {
        let ir = extract_ok("; just a comment\n;; another one", "c.clj");
        assert!(ir.functions.is_empty());
        assert!(ir.classes.is_empty());
        assert!(ir.imports.is_empty());
    }

    #[test]
    fn test_multiple_functions_extracted() {
        let ir = extract_ok("(defn a [] 1)\n(defn b [] 2)", "m.clj");
        let names: Vec<&str> = ir.functions.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b"]);
    }
}
