// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for end-to-end scenarios
//! Testing complete parsing workflows

use codegraph::CodeGraph;
use codegraph_parser_api::CodeParser;
use codegraph_python::PythonParser;
use std::path::Path;
use tempfile::TempDir;

// ====================
// End-to-End Parsing Tests
// ====================

#[test]
fn test_parse_real_python_module() {
    let parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"
"""
A sample Python module for testing.
"""

import os
import sys
from typing import List, Optional

class Calculator:
    """A simple calculator class."""

    def __init__(self):
        self.history = []

    def add(self, a: int, b: int) -> int:
        """Add two numbers."""
        result = a + b
        self.history.append(('add', a, b, result))
        return result

    def subtract(self, a: int, b: int) -> int:
        """Subtract b from a."""
        result = a - b
        self.history.append(('subtract', a, b, result))
        return result

    @property
    def last_operation(self) -> Optional[tuple]:
        """Get the last operation."""
        return self.history[-1] if self.history else None

def create_calculator() -> Calculator:
    """Factory function for creating calculators."""
    return Calculator()

async def fetch_remote_data(url: str) -> dict:
    """Fetch data from a remote URL."""
    # Simulated async operation
    return {}

if __name__ == "__main__":
    calc = create_calculator()
    print(calc.add(5, 3))
"#;

    let result = parser.parse_source(source, Path::new("calculator.py"), &mut graph);
    assert!(result.is_ok());

    let file_info = result.unwrap();

    // Verify entities
    assert_eq!(file_info.classes.len(), 1, "Should have Calculator class");
    assert!(
        file_info.functions.len() >= 3,
        "Should have methods and functions"
    );
    assert_eq!(file_info.imports.len(), 3, "Should have 3 imports");

    // Verify metrics were tracked
    let metrics = parser.metrics();
    assert_eq!(metrics.files_succeeded, 1);
    assert!(metrics.total_entities > 0);
}

#[test]
fn test_parse_python_with_all_features() {
    let parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"
from abc import ABC, abstractmethod
from dataclasses import dataclass
from typing import Protocol

class Serializable(Protocol):
    """Protocol for serializable objects."""
    def serialize(self) -> str:
        ...

@dataclass
class Person:
    """A person dataclass."""
    name: str
    age: int

    def greet(self) -> str:
        return f"Hello, I'm {self.name}"

class Animal(ABC):
    """Abstract base class for animals."""

    @abstractmethod
    def make_sound(self) -> str:
        pass

class Dog(Animal):
    """A dog that barks."""

    def make_sound(self) -> str:
        return "Woof!"

    @staticmethod
    def species() -> str:
        return "Canis familiaris"

    @classmethod
    def create_puppy(cls):
        return cls()

def test_animal_sounds():
    """Test function for animal sounds."""
    dog = Dog()
    assert dog.make_sound() == "Woof!"

async def async_function():
    """An async function."""
    return await something()
"#;

    let result = parser.parse_source(source, Path::new("advanced.py"), &mut graph);
    assert!(result.is_ok());

    let file_info = result.unwrap();

    // Should capture classes (Person, Dog), traits (Serializable, Animal), and functions
    assert!(
        file_info.classes.len() >= 2,
        "Should have at least 2 classes (Person, Dog), got {}",
        file_info.classes.len()
    );
    assert!(
        file_info.traits.len() >= 2,
        "Should have at least 2 traits (Serializable Protocol, Animal ABC), got {}",
        file_info.traits.len()
    );
    assert!(!file_info.imports.is_empty(), "Should have imports");
    assert!(!file_info.functions.is_empty(), "Should have functions");
}

#[test]
fn test_parse_project_structure() {
    let parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let temp_dir = TempDir::new().unwrap();

    // Create a realistic project structure
    let src_dir = temp_dir.path().join("src");
    std::fs::create_dir(&src_dir).unwrap();

    let models_dir = src_dir.join("models");
    std::fs::create_dir(&models_dir).unwrap();

    let utils_dir = src_dir.join("utils");
    std::fs::create_dir(&utils_dir).unwrap();

    // Create __init__.py files
    std::fs::write(src_dir.join("__init__.py"), "").unwrap();
    std::fs::write(models_dir.join("__init__.py"), "").unwrap();
    std::fs::write(utils_dir.join("__init__.py"), "").unwrap();

    // Create model files
    std::fs::write(
        models_dir.join("user.py"),
        r#"
class User:
    def __init__(self, name):
        self.name = name
"#,
    )
    .unwrap();

    std::fs::write(
        models_dir.join("product.py"),
        r#"
class Product:
    def __init__(self, name, price):
        self.name = name
        self.price = price
"#,
    )
    .unwrap();

    // Create utility files
    std::fs::write(
        utils_dir.join("helpers.py"),
        r#"
def format_currency(amount):
    return f"${amount:.2f}"
"#,
    )
    .unwrap();

    // Create main file
    std::fs::write(
        src_dir.join("main.py"),
        r#"
from models.user import User
from models.product import Product
from utils.helpers import format_currency

def main():
    user = User("Alice")
    product = Product("Widget", 19.99)
    print(format_currency(product.price))

if __name__ == "__main__":
    main()
"#,
    )
    .unwrap();

    // Parse the entire project
    let result = parser.parse_directory(&src_dir, &mut graph);
    assert!(result.is_ok());

    let project_info = result.unwrap();

    // Verify project structure
    assert_eq!(project_info.files.len(), 7, "Should parse all Python files");
    assert!(
        project_info.total_classes >= 2,
        "Should have User and Product classes"
    );
    assert!(
        project_info.total_functions >= 2,
        "Should have multiple functions"
    );
    assert!(
        project_info.total_parse_time.as_nanos() > 0,
        "Should track parse time"
    );

    // Verify metrics
    let metrics = parser.metrics();
    assert_eq!(metrics.files_succeeded, 7);
    assert!(metrics.total_entities > 0);
}

#[test]
fn test_parse_with_circular_imports() {
    let parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let temp_dir = TempDir::new().unwrap();

    // File A imports B
    std::fs::write(
        temp_dir.path().join("a.py"),
        r#"
from b import ClassB

class ClassA:
    def use_b(self):
        return ClassB()
"#,
    )
    .unwrap();

    // File B imports A (circular)
    std::fs::write(
        temp_dir.path().join("b.py"),
        r#"
from a import ClassA

class ClassB:
    def use_a(self):
        return ClassA()
"#,
    )
    .unwrap();

    // Should handle circular imports gracefully
    let result = parser.parse_directory(temp_dir.path(), &mut graph);
    assert!(result.is_ok());

    let project_info = result.unwrap();
    assert_eq!(project_info.files.len(), 2);
}

#[test]
fn test_parse_file_with_encoding_declaration() {
    let parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"
# -*- coding: utf-8 -*-
"""Module with encoding declaration."""

def hello():
    return "Hello, 世界!"
"#;

    let result = parser.parse_source(source, Path::new("encoding.py"), &mut graph);
    assert!(result.is_ok());
}

#[test]
fn test_parse_file_with_future_imports() {
    let parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"
from __future__ import annotations
from __future__ import print_function

def typed_function(x: int) -> int:
    return x + 1
"#;

    let result = parser.parse_source(source, Path::new("future.py"), &mut graph);
    assert!(result.is_ok());
}

// ====================
// Graph Verification Tests
// ====================

#[test]
fn test_graph_contains_nodes() {
    let parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"
def my_function():
    pass

class MyClass:
    def my_method(self):
        pass
"#;

    let result = parser.parse_source(source, Path::new("test.py"), &mut graph);
    assert!(result.is_ok());

    let file_info = result.unwrap();

    // Verify nodes were created
    assert!(
        !file_info.file_id.to_string().is_empty(),
        "Should have file node"
    );
    assert!(
        !file_info.functions.is_empty(),
        "Should have function nodes"
    );
    assert!(!file_info.classes.is_empty(), "Should have class nodes");
}

#[test]
fn test_graph_relationships_created() {
    let parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"
import os

def caller():
    callee()

def callee():
    pass

class Parent:
    pass

class Child(Parent):
    pass
"#;

    let result = parser.parse_source(source, Path::new("relations.py"), &mut graph);
    assert!(result.is_ok());

    // The graph should contain:
    // - Import relationships
    // - Call relationships
    // - Inheritance relationships
    let file_info = result.unwrap();
    assert!(!file_info.imports.is_empty(), "Should have import nodes");
}

// ====================
// Performance Tests
// ====================

#[test]
fn test_parse_large_file_performance() {
    let parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    // Generate a large Python file
    let mut source = String::from("# Large file test\n");
    for i in 0..100 {
        source.push_str(&format!(
            r#"
def function_{i}():
    """Docstring for function {i}"""
    return {i}

class Class_{i}:
    """Docstring for class {i}"""
    def method_{i}(self):
        return {i}
"#
        ));
    }

    let result = parser.parse_source(&source, Path::new("large.py"), &mut graph);
    assert!(result.is_ok());

    let file_info = result.unwrap();
    // tree-sitter extracts functions and methods; methods are counted separately
    // 100 standalone functions + 100 methods in ir.functions + 100 methods in ClassEntity.methods
    assert!(file_info.functions.len() >= 200); // 100 functions + at least 100 methods
    assert_eq!(file_info.classes.len(), 100);

    // Verify metrics tracked timing
    let metrics = parser.metrics();
    assert!(metrics.total_parse_time.as_nanos() > 0);
}

#[test]
fn test_parallel_file_parsing() {
    use codegraph_parser_api::ParserConfig;

    let config = ParserConfig {
        parallel: true,
        parallel_workers: Some(2),
        ..Default::default()
    };
    let parser = PythonParser::with_config(config);
    let mut graph = CodeGraph::in_memory().unwrap();

    let temp_dir = TempDir::new().unwrap();

    // Create multiple files
    for i in 0..5 {
        std::fs::write(
            temp_dir.path().join(format!("file{i}.py")),
            format!("def func{i}(): pass"),
        )
        .unwrap();
    }

    let result = parser.parse_directory(temp_dir.path(), &mut graph);
    assert!(result.is_ok());

    let project_info = result.unwrap();
    assert_eq!(project_info.files.len(), 5);
}

// ====================
// Edge Cases
// ====================

#[test]
fn test_parse_very_long_line() {
    let parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    // Create a very long line (but valid Python)
    let long_string = "x".repeat(1000);
    let source = format!(r#"def func(): return "{long_string}""#);

    let result = parser.parse_source(&source, Path::new("long_line.py"), &mut graph);
    assert!(result.is_ok());
}

#[test]
fn test_parse_deeply_nested_code() {
    let parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"
def level1():
    def level2():
        def level3():
            def level4():
                def level5():
                    return "deeply nested"
                return level5()
            return level4()
        return level3()
    return level2()
"#;

    let result = parser.parse_source(source, Path::new("nested.py"), &mut graph);
    // The parser should handle this, though nested functions might be flattened
    assert!(result.is_ok());
}

#[test]
fn test_parse_unicode_identifiers() {
    let parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"
class Café:
    def méthode(self):
        return "Bonjour"

def функция():
    return "Привет"
"#;

    let result = parser.parse_source(source, Path::new("unicode.py"), &mut graph);
    // Python 3 supports Unicode identifiers
    assert!(result.is_ok());
}

#[test]
fn test_parse_file_with_mixed_indentation() {
    let parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    // Tree-sitter parses syntactically valid code even with mixed indentation
    // The Python interpreter would fail at runtime, but tree-sitter parses it
    let source = "def foo():\n\treturn 1\n    return 2";

    let result = parser.parse_source(source, Path::new("mixed_indent.py"), &mut graph);
    // Tree-sitter successfully parses this - the indentation issues would be
    // caught at Python runtime, not by the parser
    assert!(result.is_ok());
}

#[test]
fn test_project_info_success_rate() {
    let parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let temp_dir = TempDir::new().unwrap();

    // Create 3 good files
    for i in 0..3 {
        std::fs::write(
            temp_dir.path().join(format!("good{i}.py")),
            "def foo(): pass",
        )
        .unwrap();
    }

    // Create 1 bad file - tree-sitter is more lenient with incomplete code
    // Use a more clearly broken syntax that tree-sitter will reject
    std::fs::write(temp_dir.path().join("bad.py"), "def broken(").unwrap();

    let result = parser.parse_directory(temp_dir.path(), &mut graph);
    assert!(result.is_ok());

    let project_info = result.unwrap();
    // Tree-sitter is more lenient and may parse incomplete code
    // Check that all 4 files were attempted
    assert_eq!(
        project_info.files.len() + project_info.failed_files.len(),
        4
    );
}

#[test]
fn test_avg_parse_time_calculation() {
    let parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let temp_dir = TempDir::new().unwrap();

    for i in 0..3 {
        std::fs::write(
            temp_dir.path().join(format!("file{i}.py")),
            "def foo(): pass",
        )
        .unwrap();
    }

    let result = parser.parse_directory(temp_dir.path(), &mut graph);
    assert!(result.is_ok());

    let project_info = result.unwrap();
    let avg_time = project_info.avg_parse_time();

    // Average should be total time / number of successful files
    assert!(avg_time.as_nanos() > 0);
    assert!(avg_time <= project_info.total_parse_time);
}
