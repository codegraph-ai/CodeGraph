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

    /// Run `extract` with a default config, asserting success.
    fn extract_ok(source: &str, path: &str) -> CodeIR {
        extract(source, Path::new(path), &ParserConfig::default()).expect("extract should succeed")
    }

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

    #[test]
    fn test_extract_module_name_from_file_name() {
        // Unlike most parsers (file_stem), the Dockerfile extractor uses the
        // full file_name, so a dotted name keeps its suffix.
        let ir = extract_ok("FROM alpine\n", "app.Dockerfile");
        assert_eq!(ir.module.unwrap().name, "app.Dockerfile");
    }

    #[test]
    fn test_extract_missing_file_name_fallback() {
        // `..` has no file_name, exercising the "Dockerfile" fallback branch.
        let ir = extract_ok("FROM alpine\n", "..");
        assert_eq!(ir.module.unwrap().name, "Dockerfile");
    }

    #[test]
    fn test_extract_module_path_and_line_count() {
        let source = "FROM alpine\nUSER root\nEXPOSE 8080\n";
        let ir = extract_ok(source, "build/Dockerfile");
        let module = ir.module.expect("module should be set");
        assert_eq!(
            module.path,
            Path::new("build/Dockerfile").display().to_string()
        );
        assert_eq!(module.line_count, source.lines().count());
    }

    #[test]
    fn test_extract_module_has_no_doc_or_attributes() {
        let ir = extract_ok("FROM alpine\n", "Dockerfile");
        let module = ir.module.expect("module should be set");
        assert!(module.doc_comment.is_none());
        assert!(module.attributes.is_empty());
    }

    #[test]
    fn test_extract_empty_source_yields_only_module() {
        let ir = extract_ok("", "Dockerfile");
        assert!(ir.module.is_some());
        assert!(ir.functions.is_empty());
        assert_eq!(ir.module.unwrap().line_count, 0);
    }

    #[test]
    fn test_extract_comment_only_yields_no_directives() {
        let ir = extract_ok("# base image\n# nothing else\n", "Dockerfile");
        assert!(ir.functions.is_empty());
        assert!(ir.module.is_some());
    }

    #[test]
    fn test_extract_directives_flow_into_ir_functions() {
        let source = "FROM alpine:3\nRUN apk add curl\nCMD [\"sh\"]\n";
        let ir = extract_ok(source, "Dockerfile");
        let names: Vec<&str> = ir.functions.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"FROM"), "missing FROM in {names:?}");
        assert!(names.contains(&"RUN"), "missing RUN in {names:?}");
        assert!(names.contains(&"CMD"), "missing CMD in {names:?}");
    }

    #[test]
    fn test_extract_body_prefix_flows_through() {
        let ir = extract_ok("USER root\n", "Dockerfile");
        let user = ir
            .functions
            .iter()
            .find(|f| f.name == "USER")
            .expect("USER directive missing");
        assert!(user.body_prefix.as_deref().unwrap_or("").contains("root"));
    }

    #[test]
    fn test_extract_calls_stay_empty() {
        // The Dockerfile visitor never records call relations.
        let ir = extract_ok("FROM alpine\nRUN echo hi\n", "Dockerfile");
        assert!(ir.calls.is_empty());
    }
}
