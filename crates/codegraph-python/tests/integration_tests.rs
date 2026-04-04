// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

// Integration tests for basic parsing functionality

use codegraph::CodeGraph;
use codegraph_python::{ParseError, Parser, ParserConfig};
use std::io::Write;
use std::path::Path;
use tempfile::NamedTempFile;

#[test]
fn test_parse_simple_file() {
    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = Parser::new();

    let file_path = Path::new("tests/fixtures/simple.py");
    let result = parser.parse_file(file_path, &mut graph);

    assert!(result.is_ok());
    let file_info = result.unwrap();

    // Should have found the function and class
    assert!(
        !file_info.functions.is_empty(),
        "Should find at least 1 function"
    );
    assert!(
        !file_info.classes.is_empty(),
        "Should find at least 1 class"
    );
}

#[test]
fn test_parse_empty_file() {
    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = Parser::new();

    let file_path = Path::new("tests/fixtures/empty.py");
    let result = parser.parse_file(file_path, &mut graph);

    assert!(result.is_ok());
    let file_info = result.unwrap();

    // Empty file should have no entities
    assert_eq!(file_info.functions.len(), 0);
    assert_eq!(file_info.classes.len(), 0);
}

#[test]
fn test_parse_comments_only() {
    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = Parser::new();

    let file_path = Path::new("tests/fixtures/only_comments.py");
    let result = parser.parse_file(file_path, &mut graph);

    assert!(result.is_ok());
    let file_info = result.unwrap();

    // Comments-only file should have no entities
    assert_eq!(file_info.functions.len(), 0);
    assert_eq!(file_info.classes.len(), 0);
}

#[test]
fn test_parse_malformed_file() {
    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = Parser::new();

    let file_path = Path::new("tests/fixtures/malformed.py");
    let result = parser.parse_file(file_path, &mut graph);

    // Malformed file should return an error
    assert!(result.is_err());
}

#[test]
fn test_parse_with_custom_config() {
    let mut graph = CodeGraph::in_memory().unwrap();

    let config = ParserConfig {
        include_private: false,
        include_tests: true,
        parse_docs: true,
        max_file_size: 10 * 1024 * 1024, // 10MB
        file_extensions: vec!["py".to_string()],
        exclude_dirs: vec!["__pycache__".to_string()],
        parallel: false,
        num_threads: Some(1),
    };

    let parser = Parser::with_config(config);

    let file_path = Path::new("tests/fixtures/simple.py");
    let result = parser.parse_file(file_path, &mut graph);

    assert!(result.is_ok());
}

#[test]
fn test_parse_source_directly() {
    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = Parser::new();

    let source = r#"
def hello():
    print("Hello, world!")

class Greeter:
    def greet(self):
        return "Hi"
"#;

    let file_path = Path::new("test.py");
    let result = parser.parse_source(source, file_path, &mut graph);

    assert!(result.is_ok());
    let file_info = result.unwrap();

    assert!(!file_info.functions.is_empty());
    assert!(!file_info.classes.is_empty());
}

// Error handling tests

#[test]
fn test_file_too_large() {
    let mut graph = CodeGraph::in_memory().unwrap();

    // Create config with very small max file size
    let config = ParserConfig {
        max_file_size: 10, // 10 bytes
        ..Default::default()
    };

    let parser = Parser::with_config(config);

    // Create a temporary file larger than limit
    let mut temp_file = NamedTempFile::new().unwrap();
    writeln!(temp_file, "# This is a file larger than 10 bytes").unwrap();
    temp_file.flush().unwrap();

    let result = parser.parse_file(temp_file.path(), &mut graph);

    assert!(result.is_err());
    if let Err(ParseError::FileTooLarge {
        path,
        max_size,
        actual_size,
    }) = result
    {
        assert_eq!(max_size, 10);
        assert!(actual_size > 10);
        let _ = path; // Suppress unused warning
    } else {
        panic!("Expected FileTooLarge error");
    }
}

#[test]
fn test_file_not_found() {
    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = Parser::new();

    let file_path = Path::new("nonexistent_file.py");
    let result = parser.parse_file(file_path, &mut graph);

    assert!(result.is_err());
    match result {
        Err(ParseError::IoError { .. }) => {
            // Expected
        }
        _ => panic!("Expected IoError"),
    }
}

#[test]
fn test_syntax_error() {
    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = Parser::new();

    let source = r#"
def broken_function(
    # Missing closing parenthesis and colon
"#;

    let file_path = Path::new("test.py");
    let result = parser.parse_source(source, file_path, &mut graph);

    assert!(result.is_err());
    match result {
        Err(ParseError::SyntaxError { .. }) => {
            // Expected
        }
        _ => panic!("Expected SyntaxError"),
    }
}

#[test]
fn test_invalid_extension() {
    let mut graph = CodeGraph::in_memory().unwrap();

    // Create config that only allows .py files
    let config = ParserConfig {
        file_extensions: vec!["py".to_string()],
        ..Default::default()
    };

    let parser = Parser::with_config(config);

    // Create a temporary file with wrong extension
    let mut temp_file = NamedTempFile::with_suffix(".txt").unwrap();
    writeln!(temp_file, "def foo(): pass").unwrap();
    temp_file.flush().unwrap();

    let result = parser.parse_file(temp_file.path(), &mut graph);

    assert!(result.is_err());
    match result {
        Err(ParseError::InvalidConfig(_)) => {
            // Expected
        }
        _ => panic!("Expected InvalidConfig error"),
    }
}

#[test]
fn test_error_message_format() {
    let error = ParseError::SyntaxError {
        file: "test.py".to_string(),
        line: 10,
        column: 5,
        message: "unexpected token".to_string(),
    };

    let error_string = format!("{error}");
    assert!(error_string.contains("test.py"));
    assert!(error_string.contains("10"));
    assert!(error_string.contains("5"));
    assert!(error_string.contains("unexpected token"));
}

#[test]
fn test_graph_error() {
    let error = ParseError::GraphError("database connection failed".to_string());
    let error_string = format!("{error}");
    assert!(error_string.contains("Graph operation failed"));
    assert!(error_string.contains("database connection failed"));
}

#[test]
fn test_unsupported_feature() {
    let error = ParseError::UnsupportedFeature {
        file: "advanced.py".to_string(),
        feature: "pattern matching".to_string(),
    };

    let error_string = format!("{error}");
    assert!(error_string.contains("advanced.py"));
    assert!(error_string.contains("pattern matching"));
}

#[test]
fn test_call_extraction() {
    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = Parser::new();

    let file_path = Path::new("tests/fixtures/calls.py");
    let result = parser.parse_file(file_path, &mut graph);

    assert!(result.is_ok(), "Should parse file with calls successfully");
    let file_info = result.unwrap();

    // Should have found functions and methods
    // greet, main, Calculator.add, Calculator.multiply = 4 total
    assert_eq!(
        file_info.functions.len(),
        4,
        "Should find 2 functions + 2 methods"
    );
    assert_eq!(file_info.classes.len(), 1, "Should find Calculator class");

    // The IR should have extracted call relationships
    // We expect calls like: main->greet, Calculator.multiply->Calculator.add
    // Note: The actual verification of calls in the graph would require
    // querying the graph database, which is tested in builder tests
}

#[test]
fn test_comprehensive_relationships() {
    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = Parser::new();

    let file_path = Path::new("tests/fixtures/comprehensive.py");
    let result = parser.parse_file(file_path, &mut graph);

    assert!(
        result.is_ok(),
        "Should parse comprehensive file successfully"
    );
    let file_info = result.unwrap();

    // Should extract all entities
    // Animal(ABC) is now extracted as a TraitEntity, Dog and Cat remain as classes
    assert!(
        file_info.classes.len() >= 2,
        "Should find Dog, Cat classes, got {}",
        file_info.classes.len()
    );
    assert!(
        !file_info.traits.is_empty(),
        "Should find Animal ABC as trait, got {}",
        file_info.traits.len()
    );
    assert!(
        file_info.functions.len() >= 2,
        "Should find create_animal, main functions"
    );

    // Parser successfully processes files with:
    // - Imports (os, typing, abc)
    // - Inheritance (Dog/Cat inherit from Animal)
    // - Methods (make_sound, move, fetch, scratch)
    // - Function calls (create_animal, make_sound, fetch, scratch, print)
    // All relationships are extracted and stored in the graph
}
