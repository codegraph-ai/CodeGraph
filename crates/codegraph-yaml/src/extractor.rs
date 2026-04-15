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
}
