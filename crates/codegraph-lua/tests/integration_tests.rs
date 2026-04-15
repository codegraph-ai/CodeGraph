// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for Lua parser

use codegraph::CodeGraph;
use codegraph_parser_api::CodeParser;
use codegraph_lua::LuaParser;
use std::path::Path;

const SAMPLE_APP: &str = include_str!("fixtures/sample_app.lua");

#[test]
fn test_parse_sample_app_classes() {
    let parser = LuaParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.lua"), &mut graph)
        .unwrap();

    // Lua doesn't have true classes, but the parser may extract table-based "classes"
    // At minimum we should find some structure
    println!("Classes found: {}", file_info.classes.len());

    let mut class_names = Vec::new();
    for class_id in &file_info.classes {
        let node = graph.get_node(*class_id).unwrap();
        if let Some(codegraph::PropertyValue::String(name)) = node.properties.get("name") {
            class_names.push(name.clone());
        }
    }

    println!("Class names: {:?}", class_names);
}

#[test]
fn test_parse_sample_app_functions() {
    let parser = LuaParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.lua"), &mut graph)
        .unwrap();

    // Should find multiple functions (Entity.new, Entity:display, User.new, User:addRole, etc.)
    assert!(
        file_info.functions.len() >= 5,
        "Expected at least 5 functions, found {}",
        file_info.functions.len()
    );

    let mut func_names = Vec::new();
    for func_id in &file_info.functions {
        let node = graph.get_node(*func_id).unwrap();
        if let Some(codegraph::PropertyValue::String(name)) = node.properties.get("name") {
            func_names.push(name.clone());
        }
    }

    // Check for some specific functions
    assert!(
        func_names.iter().any(|n| n.contains("addRole") || n.contains("add_role")),
        "Should contain addRole function, found: {:?}",
        func_names
    );
    assert!(
        func_names.iter().any(|n| n.contains("createUser") || n.contains("create_user")),
        "Should contain createUser function, found: {:?}",
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
    let parser = LuaParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.lua"), &mut graph)
        .unwrap();

    // Should find require("json") and require("utils")
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
    let parser = LuaParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let _file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.lua"), &mut graph)
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
    let parser = LuaParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.lua"), &mut graph)
        .unwrap();

    // At least one function should have complexity > 1 (hasRole has a for+if)
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
fn test_parse_sample_app_summary() {
    let parser = LuaParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.lua"), &mut graph)
        .unwrap();

    println!("\n=== Lua Parser Sample App Summary ===");
    println!("File: sample_app.lua");
    println!("Lines: {}", file_info.line_count);
    println!("Classes: {}", file_info.classes.len());
    println!("Functions: {}", file_info.functions.len());
    println!("Imports: {}", file_info.imports.len());
    println!("Parse time: {:?}", file_info.parse_time);
    println!("=====================================\n");

    assert!(file_info.line_count > 50);
    assert!(!file_info.functions.is_empty());
}
