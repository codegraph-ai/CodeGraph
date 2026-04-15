// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST extraction for C++ source code

use codegraph_parser_api::{CodeIR, ModuleEntity, ParserConfig, ParserError};
use std::path::Path;
use tree_sitter::Parser;

use crate::visitor::CppVisitor;

/// Extract code entities and relationships from C++ source code
pub fn extract(
    source: &str,
    file_path: &Path,
    _config: &ParserConfig,
) -> Result<CodeIR, ParserError> {
    let mut parser = Parser::new();
    let language = tree_sitter_cpp::LANGUAGE.into();
    parser
        .set_language(&language)
        .map_err(|e| ParserError::ParseError(file_path.to_path_buf(), e.to_string()))?;

    let tree = parser.parse(source, None).ok_or_else(|| {
        ParserError::ParseError(file_path.to_path_buf(), "Failed to parse".to_string())
    })?;

    // Note: NOT checking root_node.has_error() — C++ files with complex macros,
    // platform-specific extensions, or missing includes often produce partial
    // error nodes while still containing extractable entities.
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
        language: "cpp".to_string(),
        line_count: source.lines().count(),
        doc_comment: None,
        attributes: Vec::new(),
    });

    let mut visitor = CppVisitor::new(source.as_bytes());
    visitor.visit_node(root_node);

    ir.functions = visitor.functions;
    ir.classes = visitor.classes;
    ir.traits = visitor.traits;
    ir.imports = visitor.imports;
    ir.calls = visitor.calls;
    ir.inheritance = visitor.inheritance;
    ir.implementations = visitor.implementations;

    Ok(ir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_simple_class() {
        let source = r#"
class HelloWorld {
public:
    void greet() {
        // Hello
    }
};
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("HelloWorld.cpp"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "HelloWorld");
    }

    #[test]
    fn test_extract_namespace() {
        let source = r#"
namespace myns {
    class MyClass {};
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.cpp"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "myns::MyClass");
    }

    #[test]
    fn test_extract_function() {
        let source = r#"
void myFunction(int x, double y) {
    return;
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.cpp"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert!(!ir.functions.is_empty());
    }

    #[test]
    fn test_extract_includes() {
        let source = r#"
#include <iostream>
#include "myheader.h"

int main() { return 0; }
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.cpp"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.imports.len(), 2);
    }

    #[test]
    fn test_extract_inheritance() {
        let source = r#"
class Base {};
class Derived : public Base {};
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.cpp"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 2);
        assert!(!ir.inheritance.is_empty());
    }

    #[test]
    fn test_extract_calls() {
        let source = r#"
void bar() {}
void foo() { bar(); }
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.cpp"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert!(!ir.calls.is_empty(), "Should extract at least one call");
    }
}
