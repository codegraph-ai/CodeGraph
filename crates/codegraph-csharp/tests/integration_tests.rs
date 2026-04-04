// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for C# parser

use codegraph::CodeGraph;
use codegraph_csharp::CSharpParser;
use codegraph_parser_api::CodeParser;
use std::path::Path;

const SAMPLE_APP: &str = include_str!("fixtures/sample_app.cs");

#[test]
fn test_parse_sample_app_classes() {
    let parser = CSharpParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.cs"), &mut graph)
        .unwrap();

    // Should find Entity, User, Product, UserService, EmailService, Point (struct), AppConfig (record), OrderStatus (enum)
    assert!(
        file_info.classes.len() >= 7,
        "Expected at least 7 classes, found {}",
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
        class_names.iter().any(|n| n.contains("Entity")),
        "Should contain Entity class"
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
    assert!(
        class_names.iter().any(|n| n.contains("Point")),
        "Should contain Point struct"
    );
    assert!(
        class_names.iter().any(|n| n.contains("OrderStatus")),
        "Should contain OrderStatus enum"
    );

    println!("Classes found: {:?}", class_names);
}

#[test]
fn test_parse_sample_app_interfaces() {
    let parser = CSharpParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.cs"), &mut graph)
        .unwrap();

    // Should find IEntity, ISerializable, IRepository interfaces
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
        interface_names.iter().any(|n| n.contains("IEntity")),
        "Should contain IEntity interface"
    );
    assert!(
        interface_names.iter().any(|n| n.contains("ISerializable")),
        "Should contain ISerializable interface"
    );
    assert!(
        interface_names.iter().any(|n| n.contains("IRepository")),
        "Should contain IRepository interface"
    );

    println!("Interfaces found: {:?}", interface_names);
}

#[test]
fn test_parse_sample_app_methods() {
    let parser = CSharpParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.cs"), &mut graph)
        .unwrap();

    // Should find multiple methods
    assert!(
        file_info.functions.len() >= 15,
        "Expected at least 15 methods, found {}",
        file_info.functions.len()
    );

    let mut method_names = Vec::new();
    let mut async_count = 0;
    let mut static_count = 0;

    for func_id in &file_info.functions {
        let node = graph.get_node(*func_id).unwrap();
        if let Some(codegraph::PropertyValue::String(name)) = node.properties.get("name") {
            method_names.push(name.clone());
        }
        if node.properties.get_bool("is_async") == Some(true) {
            async_count += 1;
        }
        if node.properties.get_bool("is_static") == Some(true) {
            static_count += 1;
        }
    }

    // Check for some specific methods
    assert!(
        method_names.iter().any(|n| n.contains("AddRole")),
        "Should contain AddRole method"
    );
    assert!(
        method_names.iter().any(|n| n.contains("CreateUser")),
        "Should contain CreateUser method"
    );
    assert!(
        method_names.iter().any(|n| n.contains("Serialize")),
        "Should contain Serialize method"
    );

    // Should have async methods
    assert!(
        async_count >= 2,
        "Expected at least 2 async methods, found {}",
        async_count
    );

    // Should have static methods
    assert!(
        static_count >= 2,
        "Expected at least 2 static methods, found {}",
        static_count
    );

    println!("Methods found: {} total", method_names.len());
    println!("Async methods: {}", async_count);
    println!("Static methods: {}", static_count);
    println!(
        "Sample methods: {:?}",
        &method_names[..method_names.len().min(10)]
    );
}

#[test]
fn test_parse_sample_app_imports() {
    let parser = CSharpParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.cs"), &mut graph)
        .unwrap();

    // Should find using statements
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
        import_names.iter().any(|n| n.contains("System")),
        "Should contain System import"
    );
    assert!(
        import_names.iter().any(|n| n.contains("Collections")),
        "Should contain Collections import"
    );
    assert!(
        import_names.iter().any(|n| n.contains("Threading")),
        "Should contain Threading import"
    );

    println!("Imports found: {:?}", import_names);
}

#[test]
fn test_parse_sample_app_inheritance() {
    let parser = CSharpParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let _file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.cs"), &mut graph)
        .unwrap();

    // Check that graph has edges (inheritance and implementation relationships should exist)
    let edge_count = graph.edge_count();
    assert!(
        edge_count >= 4,
        "Expected at least 4 edges (inheritance/implementation relationships), found {}",
        edge_count
    );

    println!("Total edges found: {}", edge_count);
}

#[test]
fn test_parse_sample_app_summary() {
    let parser = CSharpParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.cs"), &mut graph)
        .unwrap();

    println!("\n=== C# Parser Sample App Summary ===");
    println!("File: sample_app.cs");
    println!("Lines: {}", file_info.line_count);
    println!(
        "Classes (incl. structs, enums, records): {}",
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
class Broken {
    void Method( {
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = CSharpParser::new();

    let result = parser.parse_source(source, Path::new("test.cs"), &mut graph);
    assert!(result.is_err());
}
