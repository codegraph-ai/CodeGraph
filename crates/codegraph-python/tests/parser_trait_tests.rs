// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

// Tests for CodeParser trait implementation
// Following TDD approach

use codegraph::{CodeGraph, NodeType};
use codegraph_parser_api::{CodeParser, ParserConfig, ParserError};
use codegraph_python::PythonParser;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

#[test]
fn test_python_parser_language() {
    let parser = PythonParser::new();
    assert_eq!(parser.language(), "python");
}

#[test]
fn test_python_parser_file_extensions() {
    let parser = PythonParser::new();
    let extensions = parser.file_extensions();
    assert!(extensions.contains(&".py"));
    assert!(extensions.contains(&".pyw"));
}

#[test]
fn test_python_parser_can_parse() {
    let parser = PythonParser::new();

    // Should accept .py files
    assert!(parser.can_parse(Path::new("test.py")));
    assert!(parser.can_parse(Path::new("test.pyw")));

    // Should reject other files
    assert!(!parser.can_parse(Path::new("test.rs")));
    assert!(!parser.can_parse(Path::new("test.txt")));
    assert!(!parser.can_parse(Path::new("test")));
}

#[test]
fn test_parse_simple_function() {
    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = PythonParser::new();

    let source = r#"
def hello():
    print("Hello, world!")
"#;

    let result = parser.parse_source(source, Path::new("test.py"), &mut graph);
    assert!(result.is_ok());

    let file_info = result.unwrap();
    assert_eq!(file_info.functions.len(), 1);
    assert_eq!(file_info.classes.len(), 0);
    assert!(file_info.line_count > 0);
}

#[test]
fn test_parse_class_with_methods() {
    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = PythonParser::new();

    let source = r#"
class Calculator:
    def add(self, a, b):
        return a + b

    def subtract(self, a, b):
        return a - b
"#;

    let result = parser.parse_source(source, Path::new("test.py"), &mut graph);
    assert!(result.is_ok());

    let file_info = result.unwrap();
    assert_eq!(file_info.classes.len(), 1);
    assert_eq!(file_info.functions.len(), 2); // Two methods
}

#[test]
fn test_parse_with_imports() {
    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = PythonParser::new();

    let source = r#"
import os
from pathlib import Path
import sys as system

def get_cwd():
    return os.getcwd()
"#;

    let result = parser.parse_source(source, Path::new("test.py"), &mut graph);
    assert!(result.is_ok());

    let file_info = result.unwrap();
    assert_eq!(file_info.imports.len(), 3);
    assert_eq!(file_info.functions.len(), 1);
}

#[test]
fn test_parse_file_with_syntax_error() {
    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = PythonParser::new();

    let source = r#"
def broken(
    print("missing closing paren")
"#;

    let result = parser.parse_source(source, Path::new("test.py"), &mut graph);
    assert!(result.is_err());

    match result {
        Err(ParserError::ParseError(path, msg)) => {
            assert_eq!(path, PathBuf::from("test.py"));
            assert!(!msg.is_empty());
        }
        _ => panic!("Expected ParseError"),
    }
}

#[test]
fn test_parse_file_too_large() {
    let mut graph = CodeGraph::in_memory().unwrap();
    let config = ParserConfig::default().with_max_file_size(100); // 100 bytes max
    let parser = PythonParser::with_config(config);

    // Create a source that's larger than 100 bytes
    let source = "# ".repeat(100); // 200 bytes

    let result = parser.parse_source(&source, Path::new("test.py"), &mut graph);
    assert!(result.is_err());

    match result {
        Err(ParserError::FileTooLarge(path, size)) => {
            assert_eq!(path, PathBuf::from("test.py"));
            assert!(size > 100);
        }
        _ => panic!("Expected FileTooLarge error"),
    }
}

#[test]
fn test_parse_multiple_files() {
    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = PythonParser::new();

    let temp_dir = TempDir::new().unwrap();
    let file1 = temp_dir.path().join("file1.py");
    let file2 = temp_dir.path().join("file2.py");

    std::fs::write(&file1, "def func1(): pass").unwrap();
    std::fs::write(&file2, "def func2(): pass").unwrap();

    let paths = vec![file1, file2];
    let result = parser.parse_files(&paths, &mut graph);

    assert!(result.is_ok());
    let project_info = result.unwrap();
    assert_eq!(project_info.files.len(), 2);
    assert_eq!(project_info.total_functions, 2);
    assert_eq!(project_info.failed_files.len(), 0);
}

#[test]
fn test_parse_directory() {
    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = PythonParser::new();

    let temp_dir = TempDir::new().unwrap();
    std::fs::write(temp_dir.path().join("file1.py"), "def func1(): pass").unwrap();
    std::fs::write(temp_dir.path().join("file2.py"), "class MyClass: pass").unwrap();

    // Create a subdirectory
    let subdir = temp_dir.path().join("subdir");
    std::fs::create_dir(&subdir).unwrap();
    std::fs::write(subdir.join("file3.py"), "def func3(): pass").unwrap();

    let result = parser.parse_directory(temp_dir.path(), &mut graph);
    assert!(result.is_ok());

    let project_info = result.unwrap();
    assert_eq!(project_info.files.len(), 3);
    assert_eq!(project_info.total_functions, 2);
    assert_eq!(project_info.total_classes, 1);
}

#[test]
fn test_parser_metrics() {
    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = PythonParser::new();

    let source1 = "def func1(): pass";
    let source2 = "def func2(): pass";

    parser
        .parse_source(source1, Path::new("test1.py"), &mut graph)
        .unwrap();
    parser
        .parse_source(source2, Path::new("test2.py"), &mut graph)
        .unwrap();

    let metrics = parser.metrics();
    assert_eq!(metrics.files_attempted, 2);
    assert_eq!(metrics.files_succeeded, 2);
    assert_eq!(metrics.files_failed, 0);
    assert!(metrics.total_entities > 0);
}

#[test]
fn test_parser_reset_metrics() {
    let mut parser = PythonParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    parser
        .parse_source("def func(): pass", Path::new("test.py"), &mut graph)
        .unwrap();

    let metrics_before = parser.metrics();
    assert_eq!(metrics_before.files_succeeded, 1);

    parser.reset_metrics();

    let metrics_after = parser.metrics();
    assert_eq!(metrics_after.files_succeeded, 0);
}

#[test]
fn test_skip_private_functions() {
    let mut graph = CodeGraph::in_memory().unwrap();
    let config = ParserConfig {
        skip_private: true,
        ..Default::default()
    };
    let parser = PythonParser::with_config(config);

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
    // Should only have the public function
    assert_eq!(file_info.functions.len(), 1);
}

#[test]
fn test_async_function_detection() {
    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = PythonParser::new();

    let source = r#"
async def fetch_data():
    return "data"
"#;

    let result = parser.parse_source(source, Path::new("test.py"), &mut graph);
    assert!(result.is_ok());

    let file_info = result.unwrap();
    assert_eq!(file_info.functions.len(), 1);

    // Verify the function is marked as async in the graph
    let func_ids = graph
        .query()
        .node_type(NodeType::Function)
        .execute()
        .unwrap();
    assert_eq!(func_ids.len(), 1);
    let node = graph.get_node(func_ids[0]).unwrap();
    assert_eq!(node.properties.get_string("name"), Some("fetch_data"));
    assert_eq!(node.properties.get_bool("is_async"), Some(true));
}

#[test]
fn test_decorator_extraction() {
    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = PythonParser::new();

    let source = r#"
@property
def decorated_func():
    pass
"#;

    let result = parser.parse_source(source, Path::new("test.py"), &mut graph);
    assert!(result.is_ok());

    // Verify decorators are captured
    let func_ids = graph
        .query()
        .node_type(NodeType::Function)
        .execute()
        .unwrap();
    assert_eq!(func_ids.len(), 1);
    let node = graph.get_node(func_ids[0]).unwrap();
    assert_eq!(node.properties.get_string("name"), Some("decorated_func"));
    let attrs = node
        .properties
        .get_string_list("attributes")
        .expect("attributes should be a StringList");
    assert!(
        attrs.iter().any(|a| a.contains("property")),
        "Expected @property decorator in attributes, got: {:?}",
        attrs
    );
}

#[test]
fn test_empty_file() {
    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = PythonParser::new();

    let source = "";

    let result = parser.parse_source(source, Path::new("test.py"), &mut graph);
    assert!(result.is_ok());

    let file_info = result.unwrap();
    assert_eq!(file_info.functions.len(), 0);
    assert_eq!(file_info.classes.len(), 0);
}

#[test]
fn test_multiple_classes_and_functions() {
    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = PythonParser::new();

    let source = r#"
def standalone_func():
    pass

class First:
    def method1(self):
        pass

class Second:
    def method2(self):
        pass

def another_func():
    pass
"#;

    let result = parser.parse_source(source, Path::new("test.py"), &mut graph);
    assert!(result.is_ok());

    let file_info = result.unwrap();
    assert_eq!(file_info.classes.len(), 2);
    assert_eq!(file_info.functions.len(), 4); // 2 standalone + 2 methods
}
