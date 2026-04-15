// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST extraction for C# source code

use codegraph_parser_api::{CodeIR, ModuleEntity, ParserConfig, ParserError};
use std::path::Path;
use tree_sitter::Parser;

use crate::visitor::CSharpVisitor;

/// Extract code entities and relationships from C# source code
pub fn extract(
    source: &str,
    file_path: &Path,
    _config: &ParserConfig,
) -> Result<CodeIR, ParserError> {
    let mut parser = Parser::new();
    let language = tree_sitter_c_sharp::LANGUAGE.into();
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
        language: "csharp".to_string(),
        line_count: source.lines().count(),
        doc_comment: None,
        attributes: Vec::new(),
    });

    let mut visitor = CSharpVisitor::new(source.as_bytes());
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
public class HelloWorld
{
    public static void Main(string[] args)
    {
        Console.WriteLine("Hello, World!");
    }
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("HelloWorld.cs"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "HelloWorld");
    }

    #[test]
    fn test_extract_interface() {
        let source = r#"
public interface IReadable
{
    string Read();
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("IReadable.cs"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.traits.len(), 1);
        assert_eq!(ir.traits[0].name, "IReadable");
    }

    #[test]
    fn test_extract_method() {
        let source = r#"
public class Calculator
{
    public int Add(int a, int b)
    {
        return a + b;
    }
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("Calculator.cs"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        // Methods are tracked in functions
        assert!(!ir.functions.is_empty());
    }

    #[test]
    fn test_extract_namespace_and_usings() {
        let source = r#"
using System;
using System.Collections.Generic;

namespace MyApp.Models
{
    public class User
    {
    }
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("User.cs"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.imports.len(), 2);
    }

    #[test]
    fn test_extract_multiple_entities() {
        let source = r#"
using System;

namespace Shapes
{
    public interface IShape
    {
        double Area();
    }

    public class Circle : IShape
    {
        private double radius;

        public Circle(double radius)
        {
            this.radius = radius;
        }

        public double Area()
        {
            return Math.PI * radius * radius;
        }
    }

    public class Program
    {
        public static void Main(string[] args)
        {
            Console.WriteLine("Hello");
        }
    }
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("Program.cs"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.traits.len(), 1); // IShape interface
        assert_eq!(ir.classes.len(), 2); // Circle, Program
        assert!(!ir.functions.is_empty()); // Methods
        assert_eq!(ir.imports.len(), 1); // System
    }

    #[test]
    fn test_extract_module_info() {
        let source = r#"
public class Test
{
    void TestMethod()
    {
        Console.WriteLine("test");
    }
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("Test.cs"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert!(ir.module.is_some());
        let module = ir.module.unwrap();
        assert_eq!(module.name, "Test");
        assert_eq!(module.language, "csharp");
        assert!(module.line_count > 0);
    }

    #[test]
    fn test_extract_enum() {
        let source = r#"
public enum Status
{
    Pending,
    Active,
    Completed
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("Status.cs"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        // Enums are mapped to classes
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "Status");
    }

    #[test]
    fn test_extract_struct() {
        let source = r#"
public struct Point
{
    public int X;
    public int Y;
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("Point.cs"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        // Structs are mapped to classes
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "Point");
    }

    #[test]
    fn test_extract_record() {
        let source = r#"
public record Person(string Name, int Age);
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("Person.cs"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        // Records are mapped to classes
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "Person");
    }

    #[test]
    fn test_extract_inheritance() {
        let source = r#"
public class Animal
{
    protected string Name;
}

public class Dog : Animal
{
    public void Bark()
    {
        Console.WriteLine("Woof!");
    }
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("Dog.cs"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 2);
        assert!(!ir.inheritance.is_empty());
    }

    #[test]
    fn test_extract_calls() {
        let source = r#"
public class MyClass
{
    public void Caller()
    {
        Helper();
    }

    public void Helper() {}
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.cs"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert!(!ir.calls.is_empty(), "Should extract at least one call");
    }
}
