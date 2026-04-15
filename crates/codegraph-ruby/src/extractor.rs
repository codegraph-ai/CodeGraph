// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST extraction for Ruby source code

use codegraph_parser_api::{CodeIR, ModuleEntity, ParserConfig, ParserError};
use std::path::Path;
use tree_sitter::Parser;

use crate::visitor::RubyVisitor;

/// Extract code entities and relationships from Ruby source code
pub fn extract(
    source: &str,
    file_path: &Path,
    _config: &ParserConfig,
) -> Result<CodeIR, ParserError> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_ruby::LANGUAGE.into())
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
        language: "ruby".to_string(),
        line_count: source.lines().count(),
        doc_comment: None,
        attributes: Vec::new(),
    });

    let mut visitor = RubyVisitor::new(source.as_bytes());
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
    fn test_extract_simple_method() {
        let source = r#"
def hello
  puts "Hello, world!"
end
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.rb"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].name, "hello");
    }

    #[test]
    fn test_extract_class() {
        let source = r#"
class Person
  attr_accessor :name, :age
end
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.rb"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "Person");
    }

    #[test]
    fn test_extract_module() {
        let source = r#"
module Loggable
  def log(message)
    puts message
  end
end
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.rb"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.traits.len(), 1);
        assert_eq!(ir.traits[0].name, "Loggable");
    }

    #[test]
    fn test_extract_class_with_methods() {
        let source = r#"
class Calculator
  def add(a, b)
    a + b
  end

  def subtract(a, b)
    a - b
  end
end
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.rb"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.functions.len(), 2);
    }

    #[test]
    fn test_extract_require() {
        let source = r#"
require 'json'
require_relative './helper'
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.rb"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.imports.len(), 2);
    }

    #[test]
    fn test_extract_inheritance() {
        let source = r#"
class Animal
  def speak
    "..."
  end
end

class Dog < Animal
  def speak
    "Woof!"
  end
end
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.rb"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 2);
        assert!(!ir.inheritance.is_empty());
    }

    #[test]
    fn test_extract_module_inclusion() {
        let source = r#"
module Walkable
  def walk
    "Walking..."
  end
end

class Person
  include Walkable
end
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.rb"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.traits.len(), 1);
        assert_eq!(ir.classes.len(), 1);
        assert!(!ir.implementations.is_empty());
    }

    #[test]
    fn test_extract_module_info() {
        let source = r#"
def test
  puts "test"
end
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("my_module.rb"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert!(ir.module.is_some());
        let module = ir.module.unwrap();
        assert_eq!(module.name, "my_module");
        assert_eq!(module.language, "ruby");
        assert!(module.line_count > 0);
    }

    #[test]
    fn test_extract_calls() {
        let source = r#"
def helper
  42
end

def caller
  helper
  puts "hello"
end

class MyClass
  def process
    validate
    helper
  end

  def validate
    true
  end
end
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.rb"), &config);
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
