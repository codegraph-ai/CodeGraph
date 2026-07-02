// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST extraction for TOML source code

use codegraph_parser_api::{CodeIR, ModuleEntity, ParserConfig, ParserError};
use std::path::Path;
use tree_sitter::Parser;

use crate::visitor::TomlVisitor;

/// Extract code entities from TOML source code
pub fn extract(
    source: &str,
    file_path: &Path,
    _config: &ParserConfig,
) -> Result<CodeIR, ParserError> {
    let mut parser = Parser::new();
    let language = crate::ts_toml::language();
    parser
        .set_language(&language)
        .map_err(|e| ParserError::ParseError(file_path.to_path_buf(), e.to_string()))?;

    let tree = parser.parse(source, None).ok_or_else(|| {
        ParserError::ParseError(file_path.to_path_buf(), "Failed to parse".to_string())
    })?;

    let root_node = tree.root_node();

    // TOML files often have minor syntax differences; we tolerate errors rather
    // than bailing out entirely — partial extraction is better than nothing.
    let mut ir = CodeIR::new(file_path.to_path_buf());

    let module_name = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    ir.module = Some(ModuleEntity {
        name: module_name,
        path: file_path.display().to_string(),
        language: "toml".to_string(),
        line_count: source.lines().count(),
        doc_comment: None,
        attributes: Vec::new(),
    });

    let mut visitor = TomlVisitor::new(source.as_bytes());
    visitor.visit_document(root_node);

    ir.classes = visitor.classes;
    ir.functions = visitor.functions;

    Ok(ir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> ParserConfig {
        ParserConfig::default()
    }

    #[test]
    fn test_extract_empty_file() {
        let result = extract("", Path::new("empty.toml"), &cfg());
        assert!(result.is_ok());
    }

    #[test]
    fn test_extract_top_level_keys() {
        let source = r#"
name = "my-project"
version = "0.1.0"
edition = "2021"
"#;
        let ir = extract(source, Path::new("Cargo.toml"), &cfg()).unwrap();
        assert!(!ir.functions.is_empty(), "Expected key-value properties");
        let names: Vec<&str> = ir.functions.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"name"), "Expected 'name' key");
        assert!(names.contains(&"version"), "Expected 'version' key");
    }

    #[test]
    fn test_extract_table_as_class() {
        let source = r#"
[package]
name = "codegraph"
version = "0.1.0"
"#;
        let ir = extract(source, Path::new("Cargo.toml"), &cfg()).unwrap();
        assert_eq!(ir.classes.len(), 1, "Expected 1 table section");
        assert_eq!(ir.classes[0].name, "package");
    }

    #[test]
    fn test_extract_multiple_tables() {
        let source = r#"
[package]
name = "foo"

[dependencies]
serde = "1.0"

[dev-dependencies]
tempfile = "3"
"#;
        let ir = extract(source, Path::new("Cargo.toml"), &cfg()).unwrap();
        assert_eq!(ir.classes.len(), 3, "Expected 3 table sections");
        let names: Vec<&str> = ir.classes.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"package"));
        assert!(names.contains(&"dependencies"));
        assert!(names.contains(&"dev-dependencies"));
    }

    #[test]
    fn test_extract_module_info() {
        let source = r#"name = "test""#;
        let ir = extract(source, Path::new("config.toml"), &cfg()).unwrap();
        let module = ir.module.unwrap();
        assert_eq!(module.name, "config");
        assert_eq!(module.language, "toml");
        assert!(module.line_count > 0);
    }

    #[test]
    fn test_extract_array_of_tables() {
        let source = r#"
[[bin]]
name = "server"
path = "src/main.rs"

[[bin]]
name = "client"
path = "src/client.rs"
"#;
        let ir = extract(source, Path::new("Cargo.toml"), &cfg()).unwrap();
        // Two [[bin]] sections become two class nodes
        assert_eq!(ir.classes.len(), 2);
        assert!(ir.classes.iter().all(|c| c.name == "bin"));
    }

    #[test]
    fn test_module_metadata_fields_full() {
        // test_extract_module_info only asserts name/language/line_count; pin the
        // remaining ModuleEntity fields extract() assembles directly.
        let source = r#"name = "x""#;
        let ir = extract(source, Path::new("dir/settings.toml"), &cfg()).unwrap();
        let module = ir.module.unwrap();
        assert_eq!(module.name, "settings");
        assert_eq!(
            module.path,
            Path::new("dir/settings.toml").display().to_string()
        );
        assert_eq!(module.language, "toml");
        assert_eq!(module.line_count, 1);
        assert!(module.doc_comment.is_none());
        assert!(module.attributes.is_empty());
    }

    #[test]
    fn test_unknown_module_name_fallback() {
        // An empty path has no file_stem, so the two-arm resolution falls through
        // to the innermost unwrap_or("unknown").
        let ir = extract(r#"a = 1"#, Path::new(""), &cfg()).unwrap();
        assert_eq!(ir.module.unwrap().name, "unknown");
    }

    #[test]
    fn test_empty_source_zero_line_count() {
        // Empty source parses to a valid tree: line_count 0, no entities.
        let ir = extract("", Path::new("empty.toml"), &cfg()).unwrap();
        let module = ir.module.unwrap();
        assert_eq!(module.line_count, 0);
        assert!(ir.classes.is_empty());
        assert!(ir.functions.is_empty());
    }

    #[test]
    fn test_line_count_tracks_blank_lines() {
        // line_count follows source.lines().count() independent of entity count.
        let source = "\n\n\nkey = 1\n\n\n";
        let ir = extract(source, Path::new("blanks.toml"), &cfg()).unwrap();
        assert_eq!(ir.module.unwrap().line_count, source.lines().count());
    }

    #[test]
    fn test_top_level_keys_produce_no_classes() {
        // A keys-only document (no [table] headers) leaves ir.classes empty,
        // the mirror of the table-as-class assertions.
        let source = r#"
name = "flat"
version = "1"
"#;
        let ir = extract(source, Path::new("flat.toml"), &cfg()).unwrap();
        assert!(
            ir.classes.is_empty(),
            "Top-level keys should produce no table classes"
        );
        assert!(!ir.functions.is_empty());
    }

    #[test]
    fn test_extract_pair_signature() {
        let source = r#"
[package]
name = "my-crate"
"#;
        let ir = extract(source, Path::new("Cargo.toml"), &cfg()).unwrap();
        let name_prop = ir
            .functions
            .iter()
            .find(|f| f.name == "package.name")
            .expect("Expected package.name property");
        assert!(
            name_prop.signature.contains("name"),
            "Signature should contain key name"
        );
    }
}
