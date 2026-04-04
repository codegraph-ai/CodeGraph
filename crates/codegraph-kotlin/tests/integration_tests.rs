// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for Kotlin parser

use codegraph::CodeGraph;
use codegraph_kotlin::KotlinParser;
use codegraph_parser_api::CodeParser;
use std::path::Path;

const SAMPLE_APP: &str = include_str!("fixtures/sample_app.kt");

#[test]
fn test_parse_sample_app_classes() {
    let parser = KotlinParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.kt"), &mut graph)
        .unwrap();

    // Should find BaseEntity, User, Product, UserService, AppConfig, Result, OrderStatus, EmailService
    assert!(
        file_info.classes.len() >= 6,
        "Expected at least 6 classes, found {}",
        file_info.classes.len()
    );

    let mut class_names = Vec::new();
    for class_id in &file_info.classes {
        let node = graph.get_node(*class_id).unwrap();
        if let Some(codegraph::PropertyValue::String(name)) = node.properties.get("name") {
            class_names.push(name.clone());
        }
    }

    assert!(
        class_names.iter().any(|n| n.contains("BaseEntity")),
        "Should contain BaseEntity class"
    );
    assert!(
        class_names.iter().any(|n| n.contains("User")),
        "Should contain User class"
    );
    assert!(
        class_names.iter().any(|n| n.contains("Product")),
        "Should contain Product class"
    );
    assert!(
        class_names.iter().any(|n| n.contains("UserService")),
        "Should contain UserService class"
    );

    println!("Classes found: {:?}", class_names);
}

#[test]
fn test_parse_sample_app_interfaces() {
    let parser = KotlinParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.kt"), &mut graph)
        .unwrap();

    // Should find Entity, Serializable, Repository interfaces
    assert!(
        file_info.traits.len() >= 3,
        "Expected at least 3 interfaces, found {}",
        file_info.traits.len()
    );

    let mut interface_names = Vec::new();
    for trait_id in &file_info.traits {
        let node = graph.get_node(*trait_id).unwrap();
        if let Some(codegraph::PropertyValue::String(name)) = node.properties.get("name") {
            interface_names.push(name.clone());
        }
    }

    assert!(
        interface_names.iter().any(|n| n.contains("Entity")),
        "Should contain Entity interface"
    );
    assert!(
        interface_names.iter().any(|n| n.contains("Serializable")),
        "Should contain Serializable interface"
    );
    assert!(
        interface_names.iter().any(|n| n.contains("Repository")),
        "Should contain Repository interface"
    );

    println!("Interfaces found: {:?}", interface_names);
}

#[test]
fn test_parse_sample_app_functions() {
    let parser = KotlinParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.kt"), &mut graph)
        .unwrap();

    // Should find multiple functions including top-level and extension functions
    assert!(
        file_info.functions.len() >= 10,
        "Expected at least 10 functions, found {}",
        file_info.functions.len()
    );

    let mut function_names = Vec::new();
    for func_id in &file_info.functions {
        let node = graph.get_node(*func_id).unwrap();
        if let Some(codegraph::PropertyValue::String(name)) = node.properties.get("name") {
            function_names.push(name.clone());
        }
    }

    // Check for some specific functions
    assert!(
        function_names.iter().any(|n| n.contains("addRole")),
        "Should contain addRole function"
    );
    assert!(
        function_names.iter().any(|n| n.contains("createUser")),
        "Should contain createUser function"
    );

    println!("Functions found: {} total", function_names.len());
    println!(
        "Sample functions: {:?}",
        &function_names[..function_names.len().min(15)]
    );
}

#[test]
fn test_parse_sample_app_imports() {
    let parser = KotlinParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.kt"), &mut graph)
        .unwrap();

    // Should find import statements
    assert!(
        file_info.imports.len() >= 2,
        "Expected at least 2 imports, found {}",
        file_info.imports.len()
    );

    let mut import_names = Vec::new();
    for import_id in &file_info.imports {
        let node = graph.get_node(*import_id).unwrap();
        if let Some(codegraph::PropertyValue::String(name)) = node.properties.get("name") {
            import_names.push(name.clone());
        }
    }

    assert!(
        import_names.iter().any(|n| n.contains("kotlinx")),
        "Should contain kotlinx import"
    );

    println!("Imports found: {:?}", import_names);
}

#[test]
fn test_parse_sample_app_relationships() {
    let parser = KotlinParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let _file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.kt"), &mut graph)
        .unwrap();

    // Check that graph has edges (inheritance and implementation relationships should exist)
    let edge_count = graph.edge_count();
    assert!(
        edge_count >= 4,
        "Expected at least 4 edges (inheritance/implementation), found {}",
        edge_count
    );

    println!("Total edges found: {}", edge_count);
}

#[test]
fn test_parse_sample_app_summary() {
    let parser = KotlinParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.kt"), &mut graph)
        .unwrap();

    println!("\n=== Kotlin Parser Sample App Summary ===");
    println!("File: sample_app.kt");
    println!("Lines: {}", file_info.line_count);
    println!(
        "Classes (incl. data, sealed, object, enum): {}",
        file_info.classes.len()
    );
    println!("Interfaces (traits): {}", file_info.traits.len());
    println!("Functions: {}", file_info.functions.len());
    println!("Imports: {}", file_info.imports.len());
    println!("Parse time: {:?}", file_info.parse_time);
    println!("=====================================\n");

    // Basic sanity checks
    assert!(file_info.line_count > 100);
    assert!(!file_info.classes.is_empty());
    assert!(!file_info.traits.is_empty());
    assert!(!file_info.functions.is_empty());
}

#[test]
fn test_syntax_error() {
    let source = r#"
fun broken( {
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = KotlinParser::new();

    let result = parser.parse_source(source, Path::new("test.kt"), &mut graph);
    assert!(result.is_err());
}
