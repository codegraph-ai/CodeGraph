// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST extraction for Kotlin source code

use codegraph_parser_api::{CodeIR, ModuleEntity, ParserConfig, ParserError};
use std::path::Path;
use tree_sitter::Parser;

use crate::visitor::KotlinVisitor;

/// Extract code entities and relationships from Kotlin source code
pub fn extract(
    source: &str,
    file_path: &Path,
    _config: &ParserConfig,
) -> Result<CodeIR, ParserError> {
    let mut parser = Parser::new();
    let language = tree_sitter_kotlin::language();
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
        language: "kotlin".to_string(),
        line_count: source.lines().count(),
        doc_comment: None,
        attributes: Vec::new(),
    });

    let mut visitor = KotlinVisitor::new(source.as_bytes());
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
    fun main() {
        println("Hello, World!")
    }
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("HelloWorld.kt"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "HelloWorld");
    }

    #[test]
    fn test_extract_interface() {
        let source = r#"
interface Readable {
    fun read(): String
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("Readable.kt"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.traits.len(), 1);
        assert_eq!(ir.traits[0].name, "Readable");
    }

    #[test]
    fn test_extract_function() {
        let source = r#"
fun add(a: Int, b: Int): Int {
    return a + b
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("Math.kt"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert!(!ir.functions.is_empty());
    }

    #[test]
    fn test_extract_package_and_imports() {
        let source = r#"
package com.example.app

import java.util.List
import java.util.ArrayList

class App
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("App.kt"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.imports.len(), 2);
    }

    #[test]
    fn test_extract_data_class() {
        let source = r#"
data class Person(val name: String, val age: Int)
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("Person.kt"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "Person");
    }

    #[test]
    fn test_extract_object() {
        let source = r#"
object Singleton {
    fun getInstance(): Singleton = this
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("Singleton.kt"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "Singleton");
    }

    #[test]
    fn test_extract_inheritance() {
        let source = r#"
open class Animal {
    open fun sound() {}
}

class Dog : Animal() {
    override fun sound() {
        println("Woof!")
    }
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("Animal.kt"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 2);
        assert!(!ir.inheritance.is_empty());
    }

    #[test]
    fn test_extract_enum_class() {
        let source = r#"
enum class Status {
    PENDING,
    ACTIVE,
    COMPLETED
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("Status.kt"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "Status");
    }

    #[test]
    fn test_extract_module_info() {
        let source = r#"
fun test() {
    println("test")
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("Test.kt"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert!(ir.module.is_some());
        let module = ir.module.unwrap();
        assert_eq!(module.name, "Test");
        assert_eq!(module.language, "kotlin");
        assert!(module.line_count > 0);
    }

    #[test]
    fn test_extract_calls() {
        let source = r#"
class Foo {
    fun bar() {
        baz()
    }
    fun baz() {}
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("Foo.kt"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert!(
            ir.calls
                .iter()
                .any(|c| c.caller == "bar" && c.callee == "baz"),
            "Expected call bar -> baz"
        );
    }

    #[test]
    fn test_extract_top_level_function_calls() {
        let source = r#"
fun caller() {
    helper()
    process()
}

fun helper() {}
fun process() {}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("TopLevel.kt"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert!(
            ir.calls
                .iter()
                .any(|c| c.caller == "caller" && c.callee == "helper"),
            "Expected call caller -> helper"
        );
        assert!(
            ir.calls
                .iter()
                .any(|c| c.caller == "caller" && c.callee == "process"),
            "Expected call caller -> process"
        );
    }

    #[test]
    fn test_extract_calls_empty_when_no_calls() {
        let source = r#"
fun add(a: Int, b: Int): Int {
    return a + b
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("Pure.kt"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert!(
            ir.calls.is_empty(),
            "No calls expected in pure arithmetic function"
        );
    }
}
