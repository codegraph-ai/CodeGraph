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

    #[test]
    fn test_module_metadata_fields() {
        let source = "class A {};\nclass B {};\n";
        let config = ParserConfig::default();
        let ir = extract(source, Path::new("widget.cpp"), &config).unwrap();

        let module = ir.module.expect("module entity should be set");
        assert_eq!(module.name, "widget");
        assert_eq!(module.path, "widget.cpp");
        assert_eq!(module.language, "cpp");
        assert_eq!(module.line_count, source.lines().count());
        assert!(module.doc_comment.is_none());
        assert!(module.attributes.is_empty());
    }

    #[test]
    fn test_module_name_unknown_fallback() {
        // An empty path has no file_stem, so name falls back to "unknown".
        let config = ParserConfig::default();
        let ir = extract("int main() { return 0; }", Path::new(""), &config).unwrap();

        let module = ir.module.expect("module entity should be set");
        assert_eq!(module.name, "unknown");
    }

    #[test]
    fn test_empty_source_zero_lines() {
        let config = ParserConfig::default();
        let ir = extract("", Path::new("empty.cpp"), &config).unwrap();

        let module = ir.module.expect("module entity should be set");
        assert_eq!(module.name, "empty");
        assert_eq!(module.line_count, 0);
        assert!(ir.functions.is_empty());
        assert!(ir.classes.is_empty());
        assert!(ir.imports.is_empty());
        assert!(ir.calls.is_empty());
    }

    #[test]
    fn test_line_count_tracks_blank_lines() {
        // line_count follows source.lines().count(), independent of entity count.
        let source = "\n\n\nvoid solo() {}\n\n\n";
        let config = ParserConfig::default();
        let ir = extract(source, Path::new("blanks.cpp"), &config).unwrap();

        let module = ir.module.expect("module entity should be set");
        assert_eq!(module.line_count, source.lines().count());
        assert!(!ir.functions.is_empty());
    }

    #[test]
    fn test_imports_empty_without_includes() {
        // A plain function with no #include leaves ir.imports empty.
        let config = ParserConfig::default();
        let ir = extract(
            "int add(int a, int b) { return a + b; }",
            Path::new("m.cpp"),
            &config,
        )
        .unwrap();

        assert!(ir.imports.is_empty());
    }
}
