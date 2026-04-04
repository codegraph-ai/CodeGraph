// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST extraction for Java source code

use codegraph_parser_api::{CodeIR, ModuleEntity, ParserConfig, ParserError};
use std::path::Path;
use tree_sitter::Parser;

use crate::visitor::JavaVisitor;

/// Extract code entities and relationships from Java source code
pub fn extract(
    source: &str,
    file_path: &Path,
    _config: &ParserConfig,
) -> Result<CodeIR, ParserError> {
    let mut parser = Parser::new();
    let language = tree_sitter_java::language();
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
        language: "java".to_string(),
        line_count: source.lines().count(),
        doc_comment: None,
        attributes: Vec::new(),
    });

    let mut visitor = JavaVisitor::new(source.as_bytes());
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
public class HelloWorld {
    public static void main(String[] args) {
        System.out.println("Hello, World!");
    }
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("HelloWorld.java"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "HelloWorld");
    }

    #[test]
    fn test_extract_interface() {
        let source = r#"
public interface Readable {
    String read();
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("Readable.java"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.traits.len(), 1);
        assert_eq!(ir.traits[0].name, "Readable");
    }

    #[test]
    fn test_extract_method() {
        let source = r#"
public class Calculator {
    public int add(int a, int b) {
        return a + b;
    }
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("Calculator.java"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        // Methods are tracked in functions
        assert!(!ir.functions.is_empty());
    }

    #[test]
    fn test_extract_package_and_imports() {
        let source = r#"
package com.example.app;

import java.util.List;
import java.util.ArrayList;

public class App {
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("App.java"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.imports.len(), 2);
    }

    #[test]
    fn test_extract_multiple_entities() {
        let source = r#"
package com.example;

import java.io.IOException;

public interface Shape {
    double area();
}

public class Circle implements Shape {
    private double radius;

    public Circle(double radius) {
        this.radius = radius;
    }

    @Override
    public double area() {
        return Math.PI * radius * radius;
    }
}

public class Main {
    public static void main(String[] args) {
        System.out.println("Hello");
    }
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("Main.java"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.traits.len(), 1); // Shape interface
        assert_eq!(ir.classes.len(), 2); // Circle, Main
        assert!(!ir.functions.is_empty()); // Methods
        assert_eq!(ir.imports.len(), 1); // java.io.IOException
    }

    #[test]
    fn test_extract_module_info() {
        let source = r#"
public class Test {
    void test() {
        System.out.println("test");
    }
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("Test.java"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert!(ir.module.is_some());
        let module = ir.module.unwrap();
        assert_eq!(module.name, "Test");
        assert_eq!(module.language, "java");
        assert!(module.line_count > 0);
    }

    #[test]
    fn test_extract_enum() {
        let source = r#"
public enum Status {
    PENDING,
    ACTIVE,
    COMPLETED;
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("Status.java"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        // Enums are mapped to classes
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "Status");
    }

    #[test]
    fn test_extract_record() {
        let source = r#"
public record Person(String name, int age) {
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("Person.java"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        // Records are mapped to classes
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "Person");
    }

    #[test]
    fn test_extract_inheritance() {
        let source = r#"
public class Animal {
    protected String name;
}

public class Dog extends Animal {
    public void bark() {
        System.out.println("Woof!");
    }
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("Dog.java"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 2);
        assert!(!ir.inheritance.is_empty());
    }

    #[test]
    fn test_extract_method_calls() {
        let source = r#"
public class Service {
    public void doWork() {
        helper();
        process();
    }

    private void helper() {}
    private void process() {}
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("Service.java"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert!(!ir.calls.is_empty());
        assert!(
            ir.calls
                .iter()
                .any(|c| c.caller == "doWork" && c.callee == "helper"),
            "Expected call doWork -> helper"
        );
        assert!(
            ir.calls
                .iter()
                .any(|c| c.caller == "doWork" && c.callee == "process"),
            "Expected call doWork -> process"
        );
    }

    #[test]
    fn test_extract_static_method_calls() {
        let source = r#"
public class MathHelper {
    public void calculate() {
        Math.abs(-5);
        Helper.format("text");
    }
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("MathHelper.java"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert!(
            ir.calls
                .iter()
                .any(|c| c.caller == "calculate" && c.callee == "Math.abs"),
            "Expected call calculate -> Math.abs"
        );
        assert!(
            ir.calls
                .iter()
                .any(|c| c.caller == "calculate" && c.callee == "Helper.format"),
            "Expected call calculate -> Helper.format"
        );
    }

    #[test]
    fn test_extract_constructor_calls() {
        let source = r#"
public class Factory {
    public void create() {
        new ArrayList();
        new HashMap();
    }
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("Factory.java"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert!(
            ir.calls
                .iter()
                .any(|c| c.caller == "create" && c.callee == "new ArrayList"),
            "Expected call create -> new ArrayList"
        );
        assert!(
            ir.calls
                .iter()
                .any(|c| c.caller == "create" && c.callee == "new HashMap"),
            "Expected call create -> new HashMap"
        );
    }

    #[test]
    fn test_extract_calls_empty_when_no_calls() {
        let source = r#"
public class Pure {
    public int add(int a, int b) {
        return a + b;
    }
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("Pure.java"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert!(
            ir.calls.is_empty(),
            "No calls expected in pure arithmetic method"
        );
    }
}
