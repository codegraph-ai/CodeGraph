// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST extraction for Swift source code

use codegraph_parser_api::{CodeIR, ModuleEntity, ParserConfig, ParserError};
use std::path::Path;
use tree_sitter::Parser;

use crate::visitor::SwiftVisitor;

/// Extract code entities and relationships from Swift source code
pub fn extract(
    source: &str,
    file_path: &Path,
    _config: &ParserConfig,
) -> Result<CodeIR, ParserError> {
    let mut parser = Parser::new();
    let language = tree_sitter_swift::LANGUAGE.into();
    parser
        .set_language(&language)
        .map_err(|e| ParserError::ParseError(file_path.to_path_buf(), e.to_string()))?;

    let tree = parser.parse(source, None).ok_or_else(|| {
        ParserError::ParseError(file_path.to_path_buf(), "Failed to parse".to_string())
    })?;

    let root_node = tree.root_node();

    if root_node.has_error() {
        return Err(ParserError::SyntaxError(
            file_path.to_path_buf(),
            0,
            0,
            "Syntax error".to_string(),
        ));
    }

    let mut ir = CodeIR::new(file_path.to_path_buf());

    let module_name = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();
    ir.module = Some(ModuleEntity {
        name: module_name,
        path: file_path.display().to_string(),
        language: "swift".to_string(),
        line_count: source.lines().count(),
        doc_comment: None,
        attributes: Vec::new(),
    });

    let mut visitor = SwiftVisitor::new(source.as_bytes());
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
class Person {
    var name: String
    init(name: String) {
        self.name = name
    }
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("Person.swift"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "Person");
    }

    #[test]
    fn test_extract_struct() {
        let source = r#"
struct Point {
    var x: Int
    var y: Int
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("Point.swift"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "Point");
    }

    #[test]
    fn test_extract_protocol() {
        let source = r#"
protocol Drawable {
    func draw()
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("Drawable.swift"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.traits.len(), 1);
        assert_eq!(ir.traits[0].name, "Drawable");
    }

    #[test]
    fn test_extract_function() {
        let source = r#"
func greet(name: String) -> String {
    return "Hello, \(name)!"
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("greet.swift"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert!(!ir.functions.is_empty());
    }

    #[test]
    fn test_extract_import() {
        let source = r#"
import Foundation
import UIKit

class ViewController {}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("ViewController.swift"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.imports.len(), 2);
    }

    #[test]
    fn test_extract_inheritance() {
        let source = r#"
class Animal {}
class Dog: Animal {}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("Animal.swift"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 2);
        assert!(!ir.inheritance.is_empty());
    }

    #[test]
    fn test_extract_calls() {
        let source = r#"
func helper() -> Int {
    return 42
}

func caller() {
    helper()
    print("hello")
}

class MyClass {
    func process() {
        validate()
        helper()
    }

    func validate() -> Bool {
        return true
    }
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.swift"), &config);
        assert!(result.is_ok());
        let ir = result.unwrap();
        assert!(!ir.calls.is_empty(), "Expected calls to be extracted");

        // caller -> helper
        assert!(
            ir.calls
                .iter()
                .any(|c| c.caller == "caller" && c.callee == "helper"),
            "Expected caller -> helper call, got: {:?}",
            ir.calls
                .iter()
                .map(|c| format!("{}->{}", c.caller, c.callee))
                .collect::<Vec<_>>()
        );
        // caller -> print
        assert!(
            ir.calls
                .iter()
                .any(|c| c.caller == "caller" && c.callee == "print"),
            "Expected caller -> print call"
        );
        // process -> validate
        assert!(
            ir.calls
                .iter()
                .any(|c| c.caller == "process" && c.callee == "validate"),
            "Expected process -> validate call"
        );
        // process -> helper
        assert!(
            ir.calls
                .iter()
                .any(|c| c.caller == "process" && c.callee == "helper"),
            "Expected process -> helper call"
        );
    }
}
