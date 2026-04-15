// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST extraction for PHP source code

use codegraph_parser_api::{CodeIR, ModuleEntity, ParserConfig, ParserError};
use std::path::Path;
use tree_sitter::Parser;

use crate::visitor::PhpVisitor;

/// Extract code entities and relationships from PHP source code
pub fn extract(
    source: &str,
    file_path: &Path,
    _config: &ParserConfig,
) -> Result<CodeIR, ParserError> {
    let mut parser = Parser::new();
    let language = tree_sitter_php::LANGUAGE_PHP.into();
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
        language: "php".to_string(),
        line_count: source.lines().count(),
        doc_comment: None,
        attributes: Vec::new(),
    });

    let mut visitor = PhpVisitor::new(source.as_bytes());
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
    fn test_extract_simple_function() {
        let source = r#"<?php
function hello() {
    echo "Hello, world!";
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.php"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].name, "hello");
    }

    #[test]
    fn test_extract_class() {
        let source = r#"<?php
class Person {
    public string $name;
    private int $age;
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.php"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "Person");
    }

    #[test]
    fn test_extract_interface() {
        let source = r#"<?php
interface Readable {
    public function read(): string;
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.php"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.traits.len(), 1);
        assert_eq!(ir.traits[0].name, "Readable");
    }

    #[test]
    fn test_extract_trait() {
        let source = r#"<?php
trait Loggable {
    public function log(string $message): void {
        echo $message;
    }
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.php"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.traits.len(), 1);
        assert_eq!(ir.traits[0].name, "Loggable");
    }

    #[test]
    fn test_extract_method() {
        let source = r#"<?php
class Calculator {
    public function add(int $a, int $b): int {
        return $a + $b;
    }
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.php"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        // Methods are tracked within the class
    }

    #[test]
    fn test_extract_namespace_and_use() {
        let source = r#"<?php
namespace App\Controllers;

use App\Models\User;
use App\Services\AuthService;
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.php"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.imports.len(), 2);
    }

    #[test]
    fn test_extract_multiple_entities() {
        let source = r#"<?php
namespace App;

use Exception;

interface Shape {
    public function area(): float;
}

class Circle implements Shape {
    private float $radius;

    public function __construct(float $radius) {
        $this->radius = $radius;
    }

    public function area(): float {
        return 3.14 * $this->radius * $this->radius;
    }
}

function main(): void {
    echo "Hello";
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.php"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.traits.len(), 1);
        // Namespaced: App\Shape
        assert_eq!(ir.traits[0].name, "App\\Shape");
        assert_eq!(ir.classes.len(), 1);
        // Namespaced: App\Circle
        assert_eq!(ir.classes[0].name, "App\\Circle");
        assert!(!ir.functions.is_empty()); // main function
        assert_eq!(ir.imports.len(), 1);
    }

    #[test]
    fn test_extract_module_info() {
        let source = r#"<?php
function test(): void {
    echo "test";
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("module.php"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert!(ir.module.is_some());
        let module = ir.module.unwrap();
        assert_eq!(module.name, "module");
        assert_eq!(module.language, "php");
        assert!(module.line_count > 0);
    }

    #[test]
    fn test_extract_function_with_return_type() {
        let source = r#"<?php
function add(int $a, int $b): int {
    return $a + $b;
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.php"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].name, "add");
    }

    #[test]
    fn test_extract_enum() {
        let source = r#"<?php
enum Status: string {
    case Pending = 'pending';
    case Active = 'active';
    case Completed = 'completed';
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.php"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        // Enums are mapped to classes
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "Status");
    }

    #[test]
    fn test_extract_inheritance() {
        let source = r#"<?php
class Animal {
    protected string $name;
}

class Dog extends Animal {
    public function bark(): void {
        echo "Woof!";
    }
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.php"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 2);
        assert!(!ir.inheritance.is_empty());
    }

    #[test]
    fn test_extract_calls() {
        let source = r#"<?php
function helper() {
    return 42;
}

function caller() {
    helper();
    strlen("hello");
}

class MyClass {
    public function process() {
        $this->validate();
        helper();
    }

    private function validate() {
        return true;
    }
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.php"), &config);
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
        // caller -> strlen
        assert!(
            ir.calls
                .iter()
                .any(|c| c.caller == "caller" && c.callee == "strlen"),
            "Expected caller -> strlen call"
        );
        // process -> validate (method call via $this->)
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
