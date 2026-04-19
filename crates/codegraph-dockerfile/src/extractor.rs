// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST extraction for Dockerfile source code.

use codegraph_parser_api::{CodeIR, ModuleEntity, ParserConfig, ParserError};
use std::path::Path;
use tree_sitter::Parser;

use crate::visitor::DockerfileVisitor;

/// Parse a Dockerfile source string and produce a `CodeIR` containing one
/// `FunctionEntity` per recognised directive (FROM, USER, EXPOSE, ...).
pub(crate) fn extract(
    source: &str,
    file_path: &Path,
    _config: &ParserConfig,
) -> Result<CodeIR, ParserError> {
    let mut parser = Parser::new();
    let language = crate::ts_dockerfile::language();
    parser
        .set_language(&language)
        .map_err(|e| ParserError::ParseError(file_path.to_path_buf(), e.to_string()))?;

    let tree = parser.parse(source, None).ok_or_else(|| {
        ParserError::ParseError(file_path.to_path_buf(), "Failed to parse".to_string())
    })?;

    let root_node = tree.root_node();

    let mut ir = CodeIR::new(file_path.to_path_buf());

    let module_name = file_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("Dockerfile")
        .to_string();
    ir.module = Some(ModuleEntity {
        name: module_name,
        path: file_path.display().to_string(),
        language: "dockerfile".to_string(),
        line_count: source.lines().count(),
        doc_comment: None,
        attributes: Vec::new(),
    });

    let mut visitor = DockerfileVisitor::new(source.as_bytes());
    visitor.visit_node(root_node);

    ir.functions = visitor.functions;

    Ok(ir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_basic_dockerfile() {
        let source = "FROM python:3.11\nUSER root\nEXPOSE 8080\n";
        let config = ParserConfig::default();
        let ir = extract(source, Path::new("Dockerfile"), &config).expect("parse should succeed");
        assert!(ir.functions.iter().any(|f| f.name == "FROM"));
        assert!(ir.functions.iter().any(|f| f.name == "USER"));
        assert!(ir.functions.iter().any(|f| f.name == "EXPOSE"));
    }

    #[test]
    fn test_extract_module_metadata() {
        let source = "FROM alpine\n";
        let config = ParserConfig::default();
        let ir = extract(source, Path::new("Containerfile"), &config).unwrap();
        let module = ir.module.expect("module should be set");
        assert_eq!(module.language, "dockerfile");
        assert_eq!(module.name, "Containerfile");
    }
}
