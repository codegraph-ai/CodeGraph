// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST extraction for YAML source files

use codegraph_parser_api::{CodeIR, ModuleEntity, ParserConfig, ParserError};
use std::path::Path;
use tree_sitter::Parser;

use crate::visitor::YamlVisitor;

/// Extract code entities and relationships from YAML source
pub(crate) fn extract(
    source: &str,
    file_path: &Path,
    _config: &ParserConfig,
) -> Result<CodeIR, ParserError> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_yaml::LANGUAGE.into())
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
        language: "yaml".to_string(),
        line_count: source.lines().count(),
        doc_comment: None,
        attributes: Vec::new(),
    });

    let mut visitor = YamlVisitor::new(source.as_bytes());
    visitor.visit_node(root_node);

    ir.functions = visitor.functions;
    // YAML has no imports or calls in the traditional sense
    ir.imports = Vec::new();
    ir.calls = Vec::new();

    Ok(ir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extract_ok(source: &str, file: &str) -> CodeIR {
        let config = ParserConfig::default();
        extract(source, Path::new(file), &config).expect("extract should succeed")
    }

    #[test]
    fn test_extract_top_level_keys() {
        let source = "apiVersion: apps/v1\nkind: Deployment\n";
        let config = ParserConfig::default();
        let result = extract(source, Path::new("deploy.yaml"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.functions.len(), 2);
        assert_eq!(ir.functions[0].name, "apiVersion");
        assert_eq!(ir.functions[1].name, "kind");
    }

    #[test]
    fn test_extract_nested_keys() {
        let source = "metadata:\n  name: my-app\nspec:\n  replicas: 3\n";
        let config = ParserConfig::default();
        let result = extract(source, Path::new("deploy.yaml"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        // Only top-level keys extracted: metadata, spec
        assert_eq!(ir.functions.len(), 2);
    }

    #[test]
    fn test_module_language() {
        let source = "key: value\n";
        let config = ParserConfig::default();
        let ir = extract(source, Path::new("config.yaml"), &config).unwrap();
        assert_eq!(ir.module.unwrap().language, "yaml");
    }

    #[test]
    fn module_name_from_file_stem() {
        let ir = extract_ok("key: value\n", "config.yaml");
        assert_eq!(ir.module.unwrap().name, "config");
    }

    #[test]
    fn module_name_unknown_stem_fallback() {
        // A path with no usable file_stem falls back to the "unknown" literal.
        let ir = extract_ok("key: value\n", "..");
        assert_eq!(ir.module.unwrap().name, "unknown");
    }

    #[test]
    fn module_path_and_line_count_reflect_input() {
        let source = "a: 1\nb: 2\nc: 3\n";
        let ir = extract_ok(source, "values.yaml");
        let module = ir.module.unwrap();
        assert_eq!(module.path, "values.yaml");
        assert_eq!(module.line_count, 3);
    }

    #[test]
    fn module_doc_comment_and_attributes_are_empty() {
        let ir = extract_ok("key: value\n", "config.yaml");
        let module = ir.module.unwrap();
        assert!(module.doc_comment.is_none());
        assert!(module.attributes.is_empty());
    }

    #[test]
    fn empty_source_yields_only_module() {
        let ir = extract_ok("", "empty.yaml");
        assert!(ir.module.is_some());
        assert!(ir.functions.is_empty());
        assert!(ir.imports.is_empty());
        assert!(ir.calls.is_empty());
        assert!(ir.classes.is_empty());
        assert!(ir.traits.is_empty());
    }

    #[test]
    fn comment_only_source_yields_no_functions() {
        let ir = extract_ok("# just a comment\n", "config.yaml");
        assert!(ir.module.is_some());
        assert!(ir.functions.is_empty());
    }

    #[test]
    fn imports_and_calls_are_always_empty() {
        // YAML has no import or call concept; extract() hard-clears both.
        let ir = extract_ok("apiVersion: apps/v1\nkind: Deployment\n", "deploy.yaml");
        assert_eq!(ir.functions.len(), 2);
        assert!(ir.imports.is_empty());
        assert!(ir.calls.is_empty());
    }

    #[test]
    fn classes_and_traits_stay_empty() {
        // YAML has no class or trait concept.
        let ir = extract_ok("metadata:\n  name: my-app\n", "deploy.yaml");
        assert!(ir.classes.is_empty());
        assert!(ir.traits.is_empty());
        assert!(ir.inheritance.is_empty());
        assert!(ir.implementations.is_empty());
    }

    #[test]
    fn multi_document_stream_extracts_keys_from_each_document() {
        let ir = extract_ok("kind: A\n---\nkind: B\n", "multi.yaml");
        assert_eq!(ir.functions.len(), 2);
        assert_eq!(ir.functions[0].signature, "kind: A");
        assert_eq!(ir.functions[1].signature, "kind: B");
    }

    #[test]
    fn multi_key_extraction_preserves_order() {
        let ir = extract_ok("first: 1\nsecond: 2\nthird: 3\n", "config.yaml");
        let names: Vec<_> = ir.functions.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["first", "second", "third"]);
    }
}
