// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for Elm parser

use codegraph::CodeGraph;
use codegraph_elm::ElmParser;
use codegraph_parser_api::CodeParser;
use std::path::Path;

const SAMPLE_APP: &str = include_str!("fixtures/sample_app.elm");

#[test]
fn test_parse_sample_app_functions() {
    let parser = ElmParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.elm"), &mut graph)
        .unwrap();

    // Should find: init, update, view, viewStatus, fetchUsers, usersDecoder,
    // userDecoder, formatCount, clampCount, main + 2 port annotations = 12
    assert!(
        file_info.functions.len() >= 8,
        "Expected at least 8 functions/ports, found {}",
        file_info.functions.len()
    );

    let mut func_names = Vec::new();
    for func_id in &file_info.functions {
        let node = graph.get_node(*func_id).unwrap();
        if let Some(codegraph::PropertyValue::String(name)) = node.properties.get("name") {
            func_names.push(name.clone());
        }
    }

    println!("Functions found: {} total", func_names.len());
    println!(
        "All functions: {:?}",
        &func_names[..func_names.len().min(20)]
    );

    // Core functions must be present
    assert!(
        func_names.iter().any(|n| n == "init"),
        "init not found in {:?}",
        func_names
    );
    assert!(
        func_names.iter().any(|n| n == "update"),
        "update not found in {:?}",
        func_names
    );
    assert!(
        func_names.iter().any(|n| n == "view"),
        "view not found in {:?}",
        func_names
    );
    assert!(
        func_names.iter().any(|n| n == "main"),
        "main not found in {:?}",
        func_names
    );
}

#[test]
fn test_parse_sample_app_imports() {
    let parser = ElmParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.elm"), &mut graph)
        .unwrap();

    // Should find: Browser, Html, Html.Attributes, Html.Events, Http, Json.Decode
    assert!(
        file_info.imports.len() >= 5,
        "Expected at least 5 imports, found {}",
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

    assert!(
        import_names.iter().any(|n| n == "Browser"),
        "Browser not found in {:?}",
        import_names
    );
    assert!(
        import_names.iter().any(|n| n == "Html"),
        "Html not found in {:?}",
        import_names
    );
    assert!(
        import_names.iter().any(|n| n == "Http"),
        "Http not found in {:?}",
        import_names
    );
}

#[test]
fn test_parse_sample_app_types() {
    let parser = ElmParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.elm"), &mut graph)
        .unwrap();

    // Types: Msg, Status (type declarations)
    // Aliases: Model, UserData (type alias declarations)
    assert!(
        file_info.classes.len() >= 4,
        "Expected at least 4 types/aliases, found {}",
        file_info.classes.len()
    );

    let mut type_names = Vec::new();
    for class_id in &file_info.classes {
        let node = graph.get_node(*class_id).unwrap();
        if let Some(codegraph::PropertyValue::String(name)) = node.properties.get("name") {
            type_names.push(name.clone());
        }
    }

    println!("Types found: {:?}", type_names);

    assert!(
        type_names.iter().any(|n| n == "Msg"),
        "Msg type not found in {:?}",
        type_names
    );
    assert!(
        type_names.iter().any(|n| n == "Model"),
        "Model type not found in {:?}",
        type_names
    );
    assert!(
        type_names.iter().any(|n| n == "Status"),
        "Status type not found in {:?}",
        type_names
    );
    assert!(
        type_names.iter().any(|n| n == "UserData"),
        "UserData type not found in {:?}",
        type_names
    );
}

#[test]
fn test_parse_sample_app_ports() {
    let parser = ElmParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.elm"), &mut graph)
        .unwrap();

    let mut func_names = Vec::new();
    for func_id in &file_info.functions {
        let node = graph.get_node(*func_id).unwrap();
        if let Some(codegraph::PropertyValue::String(name)) = node.properties.get("name") {
            func_names.push(name.clone());
        }
    }

    // Port declarations should appear as functions with "port" attribute
    assert!(
        func_names.iter().any(|n| n == "sendMessage"),
        "sendMessage port not found in {:?}",
        func_names
    );
    assert!(
        func_names.iter().any(|n| n == "receiveMessage"),
        "receiveMessage port not found in {:?}",
        func_names
    );
}

#[test]
fn test_parse_sample_app_complexity() {
    let parser = ElmParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.elm"), &mut graph)
        .unwrap();

    // update and formatCount have branches so complexity > 1
    let mut found_complex = false;
    for func_id in &file_info.functions {
        let node = graph.get_node(*func_id).unwrap();
        if let Some(codegraph::PropertyValue::Int(complexity)) = node.properties.get("complexity") {
            if *complexity > 1 {
                found_complex = true;
                let name = node
                    .properties
                    .get("name")
                    .and_then(|v| {
                        if let codegraph::PropertyValue::String(s) = v {
                            Some(s.as_str())
                        } else {
                            None
                        }
                    })
                    .unwrap_or("?");
                println!("Complex function: {} (complexity={})", name, complexity);
            }
        }
    }

    assert!(
        found_complex,
        "Expected at least one function with complexity > 1"
    );
}

#[test]
fn test_parse_sample_app_module_name() {
    let parser = ElmParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.elm"), &mut graph)
        .unwrap();

    let file_node = graph.get_node(file_info.file_id).unwrap();
    let name = file_node
        .properties
        .get("name")
        .and_then(|v| {
            if let codegraph::PropertyValue::String(s) = v {
                Some(s.as_str())
            } else {
                None
            }
        })
        .unwrap_or("");

    assert_eq!(
        name, "SampleApp",
        "Expected module name 'SampleApp', got '{}'",
        name
    );
}

#[test]
fn test_parse_sample_app_summary() {
    let parser = ElmParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.elm"), &mut graph)
        .unwrap();

    println!("\n=== Elm Parser Sample App Summary ===");
    println!("File: sample_app.elm");
    println!("Lines: {}", file_info.line_count);
    println!("Types: {}", file_info.classes.len());
    println!("Functions/Ports: {}", file_info.functions.len());
    println!("Imports: {}", file_info.imports.len());
    println!("Parse time: {:?}", file_info.parse_time);
    println!("=====================================\n");

    assert!(file_info.line_count > 50);
    assert!(!file_info.functions.is_empty());
    assert!(!file_info.imports.is_empty());
}
