// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST extraction for Bash/Shell source code

use codegraph_parser_api::{CodeIR, ModuleEntity, ParserConfig, ParserError};
use std::path::Path;
use tree_sitter::Parser;

use crate::visitor::BashVisitor;

/// Extract code entities and relationships from Bash source code
pub(crate) fn extract(
    source: &str,
    file_path: &Path,
    _config: &ParserConfig,
) -> Result<CodeIR, ParserError> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_bash::LANGUAGE.into())
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
        language: "bash".to_string(),
        line_count: source.lines().count(),
        doc_comment: None,
        attributes: Vec::new(),
    });

    let mut visitor = BashVisitor::new(source.as_bytes());
    visitor.visit_node(root_node);

    ir.functions = visitor.functions;
    ir.imports = visitor.imports;
    ir.calls = visitor.calls;

    Ok(ir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_simple_function() {
        let source = r#"#!/bin/bash
greet() {
    echo "Hello, world!"
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.sh"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].name, "greet");
    }

    #[test]
    fn test_extract_function_keyword() {
        let source = r#"#!/bin/bash
function deploy() {
    echo "deploying"
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.sh"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].name, "deploy");
    }

    #[test]
    fn test_extract_source_import() {
        let source = r#"source ./lib.sh
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.sh"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.imports.len(), 1);
    }

    #[test]
    fn test_extract_dot_import() {
        let source = r#". ./utils.sh
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.sh"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.imports.len(), 1);
        assert_eq!(ir.imports[0].imported, "./utils.sh");
    }
}
