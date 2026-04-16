// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST extraction for Elm source code

use codegraph_parser_api::{CodeIR, ModuleEntity, ParserConfig, ParserError};
use std::path::Path;
use tree_sitter::Parser;

use crate::visitor::ElmVisitor;

/// Extract code entities and relationships from Elm source code.
pub(crate) fn extract(
    source: &str,
    file_path: &Path,
    _config: &ParserConfig,
) -> Result<CodeIR, ParserError> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_elm::LANGUAGE.into())
        .map_err(|e| ParserError::ParseError(file_path.to_path_buf(), e.to_string()))?;

    let tree = parser.parse(source, None).ok_or_else(|| {
        ParserError::ParseError(file_path.to_path_buf(), "Failed to parse".to_string())
    })?;

    let root_node = tree.root_node();
    let source_bytes = source.as_bytes();

    let mut ir = CodeIR::new(file_path.to_path_buf());

    // Extract module name from AST; fall back to file stem
    let module_name =
        ElmVisitor::extract_module_name(root_node, source_bytes).unwrap_or_else(|| {
            file_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string()
        });

    ir.module = Some(ModuleEntity {
        name: module_name,
        path: file_path.display().to_string(),
        language: "elm".to_string(),
        line_count: source.lines().count(),
        doc_comment: None,
        attributes: Vec::new(),
    });

    let mut visitor = ElmVisitor::new(source_bytes);
    visitor.visit_node(root_node);

    ir.functions = visitor.functions;
    ir.classes = visitor.classes;
    ir.imports = visitor.imports;

    Ok(ir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_simple_function() {
        let source =
            "module Main exposing (main)\n\nmain : String\nmain =\n    \"Hello, World!\"\n";
        let config = ParserConfig::default();
        let result = extract(source, Path::new("Main.elm"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].name, "main");
    }

    #[test]
    fn test_extract_module_name() {
        let source = "module MyApp.Main exposing (..)\n\nmain = 1\n";
        let config = ParserConfig::default();
        let result = extract(source, Path::new("Main.elm"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.module.unwrap().name, "MyApp.Main");
    }

    #[test]
    fn test_extract_import() {
        let source = "module Main exposing (..)\n\nimport Html exposing (Html)\nimport Browser\n\nmain = 1\n";
        let config = ParserConfig::default();
        let result = extract(source, Path::new("Main.elm"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.imports.len(), 2);
    }

    #[test]
    fn test_extract_types() {
        let source = "module Main exposing (..)\n\ntype Msg = Increment | Decrement\n\ntype alias Model = { count : Int }\n\nmain = 1\n";
        let config = ParserConfig::default();
        let result = extract(source, Path::new("Main.elm"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 2);
        assert!(ir.classes.iter().any(|c| c.name == "Msg"));
        assert!(ir.classes.iter().any(|c| c.name == "Model"));
    }
}
