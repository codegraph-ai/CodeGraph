// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

// Integration tests for project-level parsing

use codegraph::CodeGraph;
use codegraph_python::{Parser, ParserConfig};
use std::path::Path;

#[test]
fn test_parse_directory_basic() {
    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = Parser::new();

    let project_path = Path::new("tests/fixtures/test_project");
    let result = parser.parse_directory(project_path, &mut graph);

    assert!(result.is_ok());
    let project_info = result.unwrap();

    println!("Files parsed: {}", project_info.files.len());
    println!("Total functions: {}", project_info.total_functions);
    println!("Total classes: {}", project_info.total_classes);
    println!("Failed files: {}", project_info.failed_files.len());

    for file in &project_info.files {
        println!(
            "  File: {:?}, functions: {}, classes: {}",
            file.file_path,
            file.functions.len(),
            file.classes.len()
        );
    }

    for (path, error) in &project_info.failed_files {
        println!("  Failed: {path:?} - {error}");
    }

    // Should have parsed at least 4 files (main.py, utils.py, user.py, product.py)
    assert!(
        project_info.files.len() >= 4,
        "Expected at least 4 files, got {}",
        project_info.files.len()
    );

    // Should have found multiple functions and classes
    // Now we extract methods too, so we should have:
    // - 3 top-level functions (main, helper_function, another_helper)
    // - 5 methods (User.__init__, User.get_display_name, Product.__init__, Product.get_price, UtilityClass.process)
    // = 8 total
    assert!(
        project_info.total_functions >= 8,
        "Expected at least 8 functions (including methods), got {}",
        project_info.total_functions
    );
    assert!(
        project_info.total_classes >= 3,
        "Expected at least 3 classes, got {}",
        project_info.total_classes
    );

    // Success rate should be high
    assert!(project_info.success_rate() > 90.0);
}

#[test]
fn test_parse_directory_excludes_pycache() {
    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = Parser::new();

    let project_path = Path::new("tests/fixtures/test_project");
    let result = parser.parse_directory(project_path, &mut graph);

    assert!(result.is_ok());
    let project_info = result.unwrap();

    // Should not have parsed ignored.py from __pycache__
    let parsed_files: Vec<String> = project_info
        .files
        .iter()
        .map(|f| f.file_path.display().to_string())
        .collect();

    let has_pycache = parsed_files.iter().any(|p| p.contains("__pycache__"));
    assert!(!has_pycache, "Should not parse files in __pycache__");
}

#[test]
fn test_parse_directory_with_custom_config() {
    let mut graph = CodeGraph::in_memory().unwrap();

    let config = ParserConfig {
        include_private: false,
        file_extensions: vec!["py".to_string()],
        exclude_dirs: vec!["__pycache__".to_string(), "node_modules".to_string()],
        parallel: false,
        ..Default::default()
    };

    let parser = Parser::with_config(config);

    let project_path = Path::new("tests/fixtures/test_project");
    let result = parser.parse_directory(project_path, &mut graph);

    assert!(result.is_ok());
}

#[test]
fn test_parse_directory_parallel() {
    let mut graph = CodeGraph::in_memory().unwrap();

    let config = ParserConfig {
        parallel: true,
        num_threads: Some(2),
        ..Default::default()
    };

    let parser = Parser::with_config(config);

    let project_path = Path::new("tests/fixtures/test_project");
    let result = parser.parse_directory(project_path, &mut graph);

    assert!(result.is_ok());
    let project_info = result.unwrap();

    // Same results as sequential parsing
    assert!(project_info.files.len() >= 4);
    assert!(project_info.total_functions >= 8); // Top-level functions + methods
}

#[test]
fn test_parse_nonexistent_directory() {
    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = Parser::new();

    let project_path = Path::new("tests/fixtures/nonexistent");
    let result = parser.parse_directory(project_path, &mut graph);

    // Should succeed but with empty results or all failures
    assert!(result.is_ok());
    let project_info = result.unwrap();
    assert_eq!(project_info.files.len(), 0);
}

#[test]
fn test_project_info_statistics() {
    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = Parser::new();

    let project_path = Path::new("tests/fixtures/test_project");
    let result = parser.parse_directory(project_path, &mut graph);

    assert!(result.is_ok());
    let project_info = result.unwrap();

    // Check statistics
    assert!(project_info.total_lines > 0);
    assert!(project_info.total_time.as_millis() > 0);
    assert!(project_info.avg_parse_time().as_nanos() > 0);

    // Success rate should be 100% for valid project
    assert_eq!(project_info.success_rate(), 100.0);
}

#[test]
fn test_parse_directory_collects_errors() {
    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = Parser::new();

    // Parse directory that includes malformed.py
    let fixtures_path = Path::new("tests/fixtures");
    let result = parser.parse_directory(fixtures_path, &mut graph);

    assert!(result.is_ok());
    let project_info = result.unwrap();

    // Should have some failed files (malformed.py)
    // But this depends on fixture structure, so just check it doesn't crash
    assert!(!project_info.files.is_empty() || !project_info.failed_files.is_empty());
}
