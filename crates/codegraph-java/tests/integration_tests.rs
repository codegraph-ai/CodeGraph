// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for Java parser

use codegraph::CodeGraph;
use codegraph_java::JavaParser;
use codegraph_parser_api::CodeParser;
use std::path::Path;

const SAMPLE_APP: &str = include_str!("fixtures/sample_app.java");

#[test]
fn test_parse_sample_app_classes() {
    let parser = JavaParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.java"), &mut graph)
        .unwrap();

    // Should find BaseEntity, User, Product, UserService, EmailService, OrderStatus (enum), AppConfig (record)
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
    let parser = JavaParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.java"), &mut graph)
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
fn test_parse_sample_app_methods() {
    let parser = JavaParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.java"), &mut graph)
        .unwrap();

    // Should find multiple methods
    assert!(
        file_info.functions.len() >= 15,
        "Expected at least 15 methods, found {}",
        file_info.functions.len()
    );

    let mut method_names = Vec::new();
    let mut static_count = 0;

    for func_id in &file_info.functions {
        let node = graph.get_node(*func_id).unwrap();
        if let Some(codegraph::PropertyValue::String(name)) = node.properties.get("name") {
            method_names.push(name.clone());
        }
        if node.properties.get_bool("is_static") == Some(true) {
            static_count += 1;
        }
    }

    // Check for some specific methods
    assert!(
        method_names
            .iter()
            .any(|n| n.contains("addRole") || n.contains("AddRole")),
        "Should contain addRole method"
    );
    assert!(
        method_names
            .iter()
            .any(|n| n.contains("createUser") || n.contains("CreateUser")),
        "Should contain createUser method"
    );

    // Should have static methods
    assert!(
        static_count >= 2,
        "Expected at least 2 static methods, found {}",
        static_count
    );

    println!("Methods found: {} total", method_names.len());
    println!("Static methods: {}", static_count);
}

#[test]
fn test_parse_sample_app_imports() {
    let parser = JavaParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.java"), &mut graph)
        .unwrap();

    // Should find import statements
    assert!(
        file_info.imports.len() >= 3,
        "Expected at least 3 imports, found {}",
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
        import_names.iter().any(|n| n.contains("java.util")),
        "Should contain java.util import"
    );

    println!("Imports found: {:?}", import_names);
}

#[test]
fn test_parse_sample_app_relationships() {
    let parser = JavaParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let _file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.java"), &mut graph)
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
    let parser = JavaParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.java"), &mut graph)
        .unwrap();

    println!("\n=== Java Parser Sample App Summary ===");
    println!("File: sample_app.java");
    println!("Lines: {}", file_info.line_count);
    println!(
        "Classes (incl. enums, records): {}",
        file_info.classes.len()
    );
    println!("Interfaces (traits): {}", file_info.traits.len());
    println!("Methods (functions): {}", file_info.functions.len());
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
public class Broken {
    public void method( {
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = JavaParser::new();

    let result = parser.parse_source(source, Path::new("Test.java"), &mut graph);
    assert!(result.is_err());
}
