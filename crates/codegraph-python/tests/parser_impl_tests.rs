// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Unit tests specifically for parser_impl.rs
//! Testing the PythonParser implementation details

use codegraph::CodeGraph;
use codegraph_parser_api::{CodeParser, ParserConfig, ParserError};
use codegraph_python::PythonParser;
use std::path::Path;
use tempfile::TempDir;

// ====================
// Configuration Tests
// ====================

#[test]
fn test_parser_default_config() {
    let parser = PythonParser::new();
    let config = parser.config();
    assert!(!config.skip_private);
    assert!(!config.skip_tests);
}

#[test]
fn test_parser_custom_config() {
    let config = ParserConfig {
        skip_private: true,
        skip_tests: true,
        max_file_size: 5000,
        ..Default::default()
    };
    let parser = PythonParser::with_config(config.clone());

    assert!(parser.config().skip_private);
    assert!(parser.config().skip_tests);
    assert_eq!(parser.config().max_file_size, 5000);
}

// ====================
// Metrics Tests
// ====================

#[test]
fn test_metrics_initial_state() {
    let parser = PythonParser::new();
    let metrics = parser.metrics();

    assert_eq!(metrics.files_attempted, 0);
    assert_eq!(metrics.files_succeeded, 0);
    assert_eq!(metrics.files_failed, 0);
    assert_eq!(metrics.total_entities, 0);
    assert_eq!(metrics.total_relationships, 0);
}

#[test]
fn test_metrics_after_successful_parse() {
    let parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = "def foo(): pass";
    parser
        .parse_source(source, Path::new("test.py"), &mut graph)
        .unwrap();

    let metrics = parser.metrics();
    assert_eq!(metrics.files_attempted, 1);
    assert_eq!(metrics.files_succeeded, 1);
    assert_eq!(metrics.files_failed, 0);
    assert!(metrics.total_entities > 0);
}

#[test]
fn test_metrics_after_failed_parse() {
    let parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = "def broken(\n    incomplete";
    let _ = parser.parse_source(source, Path::new("test.py"), &mut graph);

    let metrics = parser.metrics();
    assert_eq!(metrics.files_attempted, 1);
    assert_eq!(metrics.files_failed, 1);
}

#[test]
fn test_metrics_accumulation() {
    let parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    // Parse multiple files
    parser
        .parse_source("def foo(): pass", Path::new("test1.py"), &mut graph)
        .unwrap();
    parser
        .parse_source("def bar(): pass", Path::new("test2.py"), &mut graph)
        .unwrap();
    parser
        .parse_source("class Baz: pass", Path::new("test3.py"), &mut graph)
        .unwrap();

    let metrics = parser.metrics();
    assert_eq!(metrics.files_attempted, 3);
    assert_eq!(metrics.files_succeeded, 3);
    assert!(metrics.total_entities >= 3);
}

#[test]
fn test_metrics_reset() {
    let mut parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    parser
        .parse_source("def foo(): pass", Path::new("test.py"), &mut graph)
        .unwrap();

    let metrics_before = parser.metrics();
    assert!(metrics_before.files_succeeded > 0);

    parser.reset_metrics();

    let metrics_after = parser.metrics();
    assert_eq!(metrics_after.files_attempted, 0);
    assert_eq!(metrics_after.files_succeeded, 0);
    assert_eq!(metrics_after.total_entities, 0);
}

// ====================
// Parsing Tests
// ====================

#[test]
fn test_parse_empty_source() {
    let parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let result = parser.parse_source("", Path::new("empty.py"), &mut graph);
    assert!(result.is_ok());

    let file_info = result.unwrap();
    assert_eq!(file_info.functions.len(), 0);
    assert_eq!(file_info.classes.len(), 0);
}

#[test]
fn test_parse_comments_only() {
    let parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"
# This is a comment
# Another comment
    "#;

    let result = parser.parse_source(source, Path::new("comments.py"), &mut graph);
    assert!(result.is_ok());

    let file_info = result.unwrap();
    assert_eq!(file_info.functions.len(), 0);
    assert_eq!(file_info.classes.len(), 0);
}

#[test]
fn test_parse_docstring_only() {
    let parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"
"""
This is a module docstring.
"""
    "#;

    let result = parser.parse_source(source, Path::new("docstring.py"), &mut graph);
    assert!(result.is_ok());
}

#[test]
fn test_parse_simple_function() {
    let parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"
def greet(name):
    return f"Hello, {name}!"
    "#;

    let result = parser.parse_source(source, Path::new("greet.py"), &mut graph);
    assert!(result.is_ok());

    let file_info = result.unwrap();
    assert_eq!(file_info.functions.len(), 1);
    assert_eq!(file_info.classes.len(), 0);
}

#[test]
fn test_parse_async_function() {
    let parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"
async def fetch_data():
    return await some_api()
    "#;

    let result = parser.parse_source(source, Path::new("async.py"), &mut graph);
    assert!(result.is_ok());

    let file_info = result.unwrap();
    assert_eq!(file_info.functions.len(), 1);
}

#[test]
fn test_parse_class_simple() {
    let parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"
class Person:
    def __init__(self, name):
        self.name = name
    "#;

    let result = parser.parse_source(source, Path::new("person.py"), &mut graph);
    assert!(result.is_ok());

    let file_info = result.unwrap();
    assert_eq!(file_info.classes.len(), 1);
    assert_eq!(file_info.functions.len(), 1); // __init__ method
}

#[test]
fn test_parse_class_with_inheritance() {
    let parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"
class Animal:
    pass

class Dog(Animal):
    def bark(self):
        print("Woof!")
    "#;

    let result = parser.parse_source(source, Path::new("animals.py"), &mut graph);
    assert!(result.is_ok());

    let file_info = result.unwrap();
    assert_eq!(file_info.classes.len(), 2);
}

#[test]
fn test_parse_multiple_imports() {
    let parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"
import os
import sys
from pathlib import Path
from typing import List, Dict
import numpy as np
    "#;

    let result = parser.parse_source(source, Path::new("imports.py"), &mut graph);
    assert!(result.is_ok());

    let file_info = result.unwrap();
    assert_eq!(file_info.imports.len(), 5);
}

#[test]
fn test_parse_decorators() {
    let parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"
@decorator
def func1():
    pass

class MyClass:
    @property
    def prop(self):
        return self._value

    @staticmethod
    def static_method():
        pass

    @classmethod
    def class_method(cls):
        pass
    "#;

    let result = parser.parse_source(source, Path::new("decorators.py"), &mut graph);
    assert!(result.is_ok());

    let file_info = result.unwrap();
    assert!(!file_info.functions.is_empty());
}

// ====================
// Error Handling Tests
// ====================

#[test]
fn test_syntax_error_handling() {
    let parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"
def broken(
    # Missing closing paren
    "#;

    let result = parser.parse_source(source, Path::new("broken.py"), &mut graph);
    assert!(result.is_err());

    match result {
        Err(ParserError::ParseError(path, msg)) => {
            assert_eq!(path, Path::new("broken.py"));
            assert!(!msg.is_empty());
        }
        _ => panic!("Expected ParseError"),
    }
}

#[test]
fn test_file_size_limit_in_parse_source() {
    let config = ParserConfig::default().with_max_file_size(50);
    let parser = PythonParser::with_config(config);
    let mut graph = CodeGraph::in_memory().unwrap();

    // Create source larger than 50 bytes
    let source = "# ".repeat(100); // 200 bytes

    let result = parser.parse_source(&source, Path::new("large.py"), &mut graph);
    assert!(result.is_err());

    match result {
        Err(ParserError::FileTooLarge(path, size)) => {
            assert_eq!(path, Path::new("large.py"));
            assert!(size > 50);
        }
        _ => panic!("Expected FileTooLarge error"),
    }
}

#[test]
fn test_invalid_file_extension() {
    let parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test.txt");
    std::fs::write(&file_path, "def foo(): pass").unwrap();

    let result = parser.parse_file(&file_path, &mut graph);
    assert!(result.is_err());
}

// ====================
// File Operations Tests
// ====================

#[test]
fn test_parse_file_success() {
    let parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test.py");
    std::fs::write(&file_path, "def foo(): pass").unwrap();

    let result = parser.parse_file(&file_path, &mut graph);
    assert!(result.is_ok());

    let file_info = result.unwrap();
    assert_eq!(file_info.functions.len(), 1);
    assert!(file_info.byte_count > 0);
}

#[test]
fn test_parse_file_not_found() {
    let parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let result = parser.parse_file(Path::new("/nonexistent/file.py"), &mut graph);
    assert!(result.is_err());

    match result {
        Err(ParserError::IoError(_, _)) => (),
        _ => panic!("Expected IoError"),
    }
}

#[test]
fn test_parse_file_too_large() {
    let config = ParserConfig::default().with_max_file_size(10);
    let parser = PythonParser::with_config(config);
    let mut graph = CodeGraph::in_memory().unwrap();

    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("large.py");
    std::fs::write(&file_path, "# This is more than 10 bytes").unwrap();

    let result = parser.parse_file(&file_path, &mut graph);
    assert!(result.is_err());

    match result {
        Err(ParserError::FileTooLarge(_, _)) => (),
        _ => panic!("Expected FileTooLarge error"),
    }
}

// ====================
// Multiple Files Tests
// ====================

#[test]
fn test_parse_files_all_success() {
    let parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let temp_dir = TempDir::new().unwrap();
    let file1 = temp_dir.path().join("file1.py");
    let file2 = temp_dir.path().join("file2.py");
    let file3 = temp_dir.path().join("file3.py");

    std::fs::write(&file1, "def foo(): pass").unwrap();
    std::fs::write(&file2, "class Bar: pass").unwrap();
    std::fs::write(&file3, "import os").unwrap();

    let paths = vec![file1, file2, file3];
    let result = parser.parse_files(&paths, &mut graph);

    assert!(result.is_ok());
    let project_info = result.unwrap();
    assert_eq!(project_info.files.len(), 3);
    assert_eq!(project_info.failed_files.len(), 0);
    assert_eq!(project_info.total_functions, 1);
    assert_eq!(project_info.total_classes, 1);
}

#[test]
fn test_parse_files_partial_failure() {
    let parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let temp_dir = TempDir::new().unwrap();
    let file1 = temp_dir.path().join("good.py");
    let file2 = temp_dir.path().join("bad.py");

    std::fs::write(&file1, "def foo(): pass").unwrap();
    std::fs::write(&file2, "def broken(\n    incomplete").unwrap();

    let paths = vec![file1, file2];
    let result = parser.parse_files(&paths, &mut graph);

    assert!(result.is_ok());
    let project_info = result.unwrap();
    assert_eq!(project_info.files.len(), 1);
    assert_eq!(project_info.failed_files.len(), 1);
}

#[test]
fn test_parse_directory_recursive() {
    let parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let temp_dir = TempDir::new().unwrap();

    // Create files in root
    std::fs::write(temp_dir.path().join("file1.py"), "def foo(): pass").unwrap();

    // Create subdirectory with files
    let subdir = temp_dir.path().join("subdir");
    std::fs::create_dir(&subdir).unwrap();
    std::fs::write(subdir.join("file2.py"), "def bar(): pass").unwrap();

    // Create nested subdirectory
    let nested = subdir.join("nested");
    std::fs::create_dir(&nested).unwrap();
    std::fs::write(nested.join("file3.py"), "def baz(): pass").unwrap();

    let result = parser.parse_directory(temp_dir.path(), &mut graph);
    assert!(result.is_ok());

    let project_info = result.unwrap();
    assert_eq!(project_info.files.len(), 3);
}

#[test]
fn test_parse_directory_empty() {
    let parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let temp_dir = TempDir::new().unwrap();

    let result = parser.parse_directory(temp_dir.path(), &mut graph);
    assert!(result.is_ok());

    let project_info = result.unwrap();
    assert_eq!(project_info.files.len(), 0);
}

// ====================
// Special Cases Tests
// ====================

#[test]
fn test_parse_private_functions_included() {
    let parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"
def public_func():
    pass

def _private_func():
    pass

def __very_private():
    pass
    "#;

    let result = parser.parse_source(source, Path::new("test.py"), &mut graph);
    assert!(result.is_ok());

    let file_info = result.unwrap();
    assert_eq!(file_info.functions.len(), 3); // All functions included by default
}

#[test]
fn test_parse_private_functions_excluded() {
    let config = ParserConfig {
        skip_private: true,
        ..Default::default()
    };
    let parser = PythonParser::with_config(config);
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"
def public_func():
    pass

def _private_func():
    pass
    "#;

    let result = parser.parse_source(source, Path::new("test.py"), &mut graph);
    assert!(result.is_ok());

    let file_info = result.unwrap();
    assert_eq!(file_info.functions.len(), 1); // Only public function
}

#[test]
fn test_parse_test_functions_included() {
    let parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"
def test_something():
    assert True

def regular_function():
    pass
    "#;

    let result = parser.parse_source(source, Path::new("test.py"), &mut graph);
    assert!(result.is_ok());

    let file_info = result.unwrap();
    assert_eq!(file_info.functions.len(), 2); // Both included by default
}

#[test]
fn test_parse_test_functions_excluded() {
    let config = ParserConfig {
        skip_tests: true,
        ..Default::default()
    };
    let parser = PythonParser::with_config(config);
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"
def test_something():
    assert True

def regular_function():
    pass
    "#;

    let result = parser.parse_source(source, Path::new("test.py"), &mut graph);
    assert!(result.is_ok());

    let file_info = result.unwrap();
    assert_eq!(file_info.functions.len(), 1); // Test function excluded
}

#[test]
fn test_complex_nested_structures() {
    let parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"
class Outer:
    class Inner:
        def inner_method(self):
            def nested_function():
                pass
            return nested_function

    def outer_method(self):
        pass
    "#;

    let result = parser.parse_source(source, Path::new("nested.py"), &mut graph);
    assert!(result.is_ok());

    // The parser should handle nested structures
    let file_info = result.unwrap();
    assert!(!file_info.classes.is_empty());
}
