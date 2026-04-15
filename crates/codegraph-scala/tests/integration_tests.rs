// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for Scala parser

use codegraph::CodeGraph;
use codegraph_parser_api::CodeParser;
use codegraph_scala::ScalaParser;
use std::path::Path;

const SAMPLE_APP: &str = include_str!("fixtures/sample_app.scala");

#[test]
fn test_parse_sample_app_classes() {
    let parser = ScalaParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.scala"), &mut graph)
        .unwrap();

    // Should find Entity, User, Product classes and UserService object
    assert!(
        file_info.classes.len() >= 3,
        "Expected at least 3 classes, found {}",
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
        "Should contain Entity class, found: {:?}",
        class_names
    );
    assert!(
        class_names.iter().any(|n| n.contains("User") && !n.contains("UserService")),
        "Should contain User class, found: {:?}",
        class_names
    );
    assert!(
        class_names.iter().any(|n| n.contains("Product")),
        "Should contain Product class, found: {:?}",
        class_names
    );

    println!("Classes found: {:?}", class_names);
}

#[test]
fn test_parse_sample_app_functions() {
    let parser = ScalaParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.scala"), &mut graph)
        .unwrap();

    // Should find multiple methods (display, addRole, isAdmin, validateRole, etc.)
    assert!(
        file_info.functions.len() >= 5,
        "Expected at least 5 functions/methods, found {}",
        file_info.functions.len()
    );

    let mut func_names = Vec::new();
    for func_id in &file_info.functions {
        let node = graph.get_node(*func_id).unwrap();
        if let Some(codegraph::PropertyValue::String(name)) = node.properties.get("name") {
            func_names.push(name.clone());
        }
    }

    // Check for specific methods
    assert!(
        func_names.iter().any(|n| n.contains("addRole")),
        "Should contain addRole method, found: {:?}",
        func_names
    );
    assert!(
        func_names.iter().any(|n| n.contains("createUser")),
        "Should contain createUser method, found: {:?}",
        func_names
    );
    assert!(
        func_names.iter().any(|n| n.contains("display")),
        "Should contain display method, found: {:?}",
        func_names
    );

    println!("Functions found: {} total", func_names.len());
    println!(
        "Sample functions: {:?}",
        &func_names[..func_names.len().min(15)]
    );
}

#[test]
fn test_parse_sample_app_imports() {
    let parser = ScalaParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.scala"), &mut graph)
        .unwrap();

    // Should find scala.collection.mutable, scala.concurrent.Future, etc.
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

    println!("Imports found: {:?}", import_names);
}

#[test]
fn test_parse_sample_app_calls() {
    let parser = ScalaParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let _file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.scala"), &mut graph)
        .unwrap();

    let edge_count = graph.edge_count();
    assert!(
        edge_count >= 1,
        "Expected at least 1 edge (call relationships), found {}",
        edge_count
    );

    println!("Total edges found: {}", edge_count);
}

#[test]
fn test_parse_sample_app_complexity() {
    let parser = ScalaParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.scala"), &mut graph)
        .unwrap();

    // At least one function should have complexity > 1 (addRole, validateRole have if-statements)
    let mut found_complex = false;
    for func_id in &file_info.functions {
        let node = graph.get_node(*func_id).unwrap();
        if let Some(codegraph::PropertyValue::Int(complexity)) =
            node.properties.get("complexity")
        {
            if *complexity > 1 {
                found_complex = true;
                let name = node
                    .properties
                    .get("name")
                    .and_then(|v| if let codegraph::PropertyValue::String(s) = v { Some(s.as_str()) } else { None })
                    .unwrap_or("?");
                println!("Complex function: {} (complexity={})", name, complexity);
            }
        }
    }

    assert!(found_complex, "Expected at least one function with complexity > 1");
}

#[test]
fn test_parse_sample_app_traits() {
    let parser = ScalaParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.scala"), &mut graph)
        .unwrap();

    // Should find the Serializable trait
    println!("Traits found: {}", file_info.traits.len());

    let mut trait_names = Vec::new();
    for trait_id in &file_info.traits {
        let node = graph.get_node(*trait_id).unwrap();
        if let Some(codegraph::PropertyValue::String(name)) = node.properties.get("name") {
            trait_names.push(name.clone());
        }
    }

    println!("Trait names: {:?}", trait_names);
}

#[test]
fn test_parse_sample_app_summary() {
    let parser = ScalaParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.scala"), &mut graph)
        .unwrap();

    println!("\n=== Scala Parser Sample App Summary ===");
    println!("File: sample_app.scala");
    println!("Lines: {}", file_info.line_count);
    println!("Classes: {}", file_info.classes.len());
    println!("Traits: {}", file_info.traits.len());
    println!("Functions: {}", file_info.functions.len());
    println!("Imports: {}", file_info.imports.len());
    println!("Parse time: {:?}", file_info.parse_time);
    println!("=======================================\n");

    assert!(file_info.line_count > 50);
    assert!(!file_info.classes.is_empty());
    assert!(!file_info.functions.is_empty());
}
