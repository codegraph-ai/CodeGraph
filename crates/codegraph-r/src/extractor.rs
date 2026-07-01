// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST extraction for R source code

use codegraph_parser_api::{CodeIR, ModuleEntity, ParserConfig, ParserError};
use std::path::Path;
use tree_sitter::Parser;

use crate::ts_r;
use crate::visitor::RVisitor;

/// Extract code entities and relationships from R source code
pub(crate) fn extract(
    source: &str,
    file_path: &Path,
    _config: &ParserConfig,
) -> Result<CodeIR, ParserError> {
    let mut parser = Parser::new();
    parser
        .set_language(&ts_r::language())
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
        language: "r".to_string(),
        line_count: source.lines().count(),
        doc_comment: None,
        attributes: Vec::new(),
    });

    let mut visitor = RVisitor::new(source.as_bytes());
    visitor.visit_node(root_node);

    ir.functions = visitor.functions;
    ir.imports = visitor.imports;
    ir.calls = visitor.calls;

    Ok(ir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extract_ok(source: &str, path: &str) -> CodeIR {
        let config = ParserConfig::default();
        extract(source, Path::new(path), &config).expect("extract should succeed")
    }

    #[test]
    fn test_extract_simple_function() {
        let source = r#"
add <- function(a, b) {
    a + b
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.R"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].name, "add");
    }

    #[test]
    fn test_extract_library_call() {
        let source = r#"
library(ggplot2)
require(dplyr)
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.R"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.imports.len(), 2);
    }

    #[test]
    fn test_module_name_from_file_stem() {
        let ir = extract_ok("x <- 1\n", "analysis.R");
        let module = ir.module.expect("module should be set");
        assert_eq!(module.name, "analysis");
    }

    #[test]
    fn test_module_name_unknown_fallback() {
        // A path of ".." has no file_stem, exercising the "unknown" fallback.
        let ir = extract_ok("x <- 1\n", "..");
        let module = ir.module.expect("module should be set");
        assert_eq!(module.name, "unknown");
    }

    #[test]
    fn test_module_path_and_language() {
        let ir = extract_ok("x <- 1\n", "pkg/util.R");
        let module = ir.module.expect("module should be set");
        assert_eq!(module.path, Path::new("pkg/util.R").display().to_string());
        assert_eq!(module.language, "r");
    }

    #[test]
    fn test_module_line_count() {
        let source = "a <- 1\nb <- 2\nc <- 3\n";
        let ir = extract_ok(source, "test.R");
        let module = ir.module.expect("module should be set");
        assert_eq!(module.line_count, source.lines().count());
    }

    #[test]
    fn test_module_doc_comment_and_attributes_empty() {
        let ir = extract_ok("x <- 1\n", "test.R");
        let module = ir.module.expect("module should be set");
        assert_eq!(module.doc_comment, None);
        assert!(module.attributes.is_empty());
    }

    #[test]
    fn test_empty_source_yields_only_module() {
        let ir = extract_ok("", "empty.R");
        assert!(ir.module.is_some());
        assert!(ir.functions.is_empty());
        assert!(ir.imports.is_empty());
        assert!(ir.calls.is_empty());
    }

    #[test]
    fn test_comment_only_source_yields_no_entities() {
        let ir = extract_ok("# just a comment\n", "test.R");
        assert!(ir.module.is_some());
        assert!(ir.functions.is_empty());
        assert!(ir.imports.is_empty());
        assert!(ir.calls.is_empty());
    }

    #[test]
    fn test_calls_populated_via_caller_callee() {
        let source = r#"
helper <- function() {
    42
}

run <- function() {
    helper()
}
"#;
        let ir = extract_ok(source, "test.R");
        assert_eq!(ir.functions.len(), 2);
        assert!(
            ir.calls
                .iter()
                .any(|c| c.caller == "run" && c.callee == "helper"),
            "expected a run -> helper call relation, got {:?}",
            ir.calls
        );
    }

    #[test]
    fn test_mixed_import_and_function() {
        let source = r#"
library(dplyr)

transform_data <- function(df) {
    df
}
"#;
        let ir = extract_ok(source, "test.R");
        assert_eq!(ir.imports.len(), 1);
        assert_eq!(ir.imports[0].imported, "dplyr");
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].name, "transform_data");
    }

    #[test]
    fn test_multiple_functions_extracted() {
        let source = r#"
f <- function() 1
g <- function() 2
h <- function() 3
"#;
        let ir = extract_ok(source, "test.R");
        assert_eq!(ir.functions.len(), 3);
        let names: Vec<&str> = ir.functions.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"f"));
        assert!(names.contains(&"g"));
        assert!(names.contains(&"h"));
    }
}
