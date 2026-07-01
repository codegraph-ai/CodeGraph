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

    #[test]
    fn test_extract_module_metadata() {
        let source = r#"#!/bin/bash
greet() {
    echo "hi"
}
"#;
        let config = ParserConfig::default();
        let ir = extract(source, Path::new("deploy.sh"), &config).unwrap();

        let module = ir.module.expect("module entity present");
        assert_eq!(module.name, "deploy");
        assert_eq!(module.path, "deploy.sh");
        assert_eq!(module.language, "bash");
        assert_eq!(module.line_count, source.lines().count());
        assert!(module.doc_comment.is_none());
        assert!(module.attributes.is_empty());
    }

    #[test]
    fn test_extract_module_name_unknown_fallback() {
        // An empty path has no file_stem, so the module name falls back to "unknown".
        let config = ParserConfig::default();
        let ir = extract("echo hi\n", Path::new(""), &config).unwrap();

        let module = ir.module.expect("module entity present");
        assert_eq!(module.name, "unknown");
    }

    #[test]
    fn test_extract_empty_source() {
        // Empty source parses to a valid empty tree: module name still derives from
        // the file stem, line_count is 0, and no functions are extracted.
        let config = ParserConfig::default();
        let ir = extract("", Path::new("empty.sh"), &config).unwrap();

        let module = ir.module.expect("module entity present");
        assert_eq!(module.name, "empty");
        assert_eq!(module.line_count, 0);
        assert!(ir.functions.is_empty());
    }

    #[test]
    fn test_extract_calls_propagated() {
        // A command invoked inside a function body surfaces as a call relation,
        // propagated from visitor.calls into ir.calls.
        let source = r#"#!/bin/bash
run() {
    deploy
}
"#;
        let config = ParserConfig::default();
        let ir = extract(source, Path::new("test.sh"), &config).unwrap();

        assert!(ir
            .calls
            .iter()
            .any(|c| c.caller == "run" && c.callee == "deploy"));
    }
}
