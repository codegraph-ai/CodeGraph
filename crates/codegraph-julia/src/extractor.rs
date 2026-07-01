// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST extraction for Julia source code

use codegraph_parser_api::{CodeIR, ModuleEntity, ParserConfig, ParserError};
use std::path::Path;
use tree_sitter::Parser;

use crate::visitor::JuliaVisitor;

/// Extract code entities and relationships from Julia source code
pub(crate) fn extract(
    source: &str,
    file_path: &Path,
    _config: &ParserConfig,
) -> Result<CodeIR, ParserError> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_julia::LANGUAGE.into())
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
        language: "julia".to_string(),
        line_count: source.lines().count(),
        doc_comment: None,
        attributes: Vec::new(),
    });

    let mut visitor = JuliaVisitor::new(source.as_bytes());
    visitor.visit_node(root_node);

    ir.functions = visitor.functions;
    ir.imports = visitor.imports;
    ir.calls = visitor.calls;
    ir.classes = visitor.classes;
    ir.traits = visitor.traits;

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
function hello()
    println("Hello, world!")
end
"#;
        let ir = extract_ok(source, "test.jl");
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].name, "hello");
    }

    #[test]
    fn test_extract_using() {
        let ir = extract_ok("using DataFrames\n", "test.jl");
        assert_eq!(ir.imports.len(), 1);
    }

    #[test]
    fn test_extract_struct() {
        let source = r#"
struct User
    name::String
    email::String
end
"#;
        let ir = extract_ok(source, "test.jl");
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "User");
    }

    #[test]
    fn test_module_name_from_file_stem() {
        let ir = extract_ok("", "widgets.jl");
        let module = ir.module.expect("module should be set");
        assert_eq!(module.name, "widgets");
    }

    #[test]
    fn test_module_name_fallback_when_no_stem() {
        let ir = extract_ok("", "..");
        let module = ir.module.expect("module should be set");
        assert_eq!(module.name, "unknown");
    }

    #[test]
    fn test_module_metadata() {
        let source = "function f()\nend\nfunction g()\nend\n";
        let ir = extract_ok(source, "meta.jl");
        let module = ir.module.expect("module should be set");
        assert_eq!(module.path, "meta.jl");
        assert_eq!(module.language, "julia");
        assert_eq!(module.line_count, source.lines().count());
        assert!(module.doc_comment.is_none());
        assert!(module.attributes.is_empty());
    }

    #[test]
    fn test_empty_source_yields_only_module() {
        let ir = extract_ok("", "empty.jl");
        assert!(ir.module.is_some());
        assert!(ir.functions.is_empty());
        assert!(ir.imports.is_empty());
        assert!(ir.calls.is_empty());
        assert!(ir.classes.is_empty());
        assert!(ir.traits.is_empty());
    }

    #[test]
    fn test_comment_only_source() {
        let ir = extract_ok("# just a comment\n", "comment.jl");
        assert!(ir.module.is_some());
        assert!(ir.functions.is_empty());
        assert!(ir.classes.is_empty());
        assert!(ir.calls.is_empty());
    }

    #[test]
    fn test_abstract_type_flows_into_traits() {
        let ir = extract_ok("abstract type Animal end\n", "types.jl");
        assert_eq!(ir.traits.len(), 1);
        assert_eq!(ir.traits[0].name, "Animal");
        assert!(ir.classes.is_empty());
    }

    #[test]
    fn test_calls_populated_via_caller_callee() {
        let source = r#"
function callee()
end

function caller()
    callee()
end
"#;
        let ir = extract_ok(source, "calls.jl");
        assert_eq!(ir.functions.len(), 2);
        assert!(
            ir.calls.iter().any(|c| c.callee == "callee"),
            "expected a call relation to callee, got {:?}",
            ir.calls
        );
    }

    #[test]
    fn test_mixed_source_populates_every_kind() {
        let source = r#"
using DataFrames

abstract type Shape end

struct Circle
    radius::Float64
end

function area(c)
    compute(c)
end
"#;
        let ir = extract_ok(source, "mixed.jl");
        assert!(!ir.imports.is_empty(), "expected imports");
        assert!(!ir.traits.is_empty(), "expected traits");
        assert!(!ir.classes.is_empty(), "expected classes");
        assert!(!ir.functions.is_empty(), "expected functions");
        assert!(!ir.calls.is_empty(), "expected calls");
    }

    #[test]
    fn test_multiple_functions() {
        let source = r#"
function one()
end

function two()
end

function three()
end
"#;
        let ir = extract_ok(source, "multi.jl");
        assert_eq!(ir.functions.len(), 3);
    }
}
