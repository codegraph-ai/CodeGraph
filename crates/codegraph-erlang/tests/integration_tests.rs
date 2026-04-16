// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for Erlang parser

use codegraph::CodeGraph;
use codegraph_erlang::ErlangParser;
use codegraph_parser_api::CodeParser;
use std::path::Path;

const SAMPLE_APP: &str = include_str!("fixtures/sample_app.erl");

#[test]
fn test_parse_sample_app_functions() {
    let parser = ErlangParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.erl"), &mut graph)
        .unwrap();

    // Expect exported + internal functions
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

    println!("Functions found: {:?}", func_names);

    // Public API functions must be present
    assert!(
        func_names.iter().any(|n| n == "create_user"),
        "Expected create_user, found: {:?}",
        func_names
    );
    assert!(
        func_names.iter().any(|n| n == "add_role"),
        "Expected add_role, found: {:?}",
        func_names
    );
    assert!(
        func_names.iter().any(|n| n == "get_user"),
        "Expected get_user, found: {:?}",
        func_names
    );
}

#[test]
fn test_parse_sample_app_visibility() {
    let parser = ErlangParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.erl"), &mut graph)
        .unwrap();

    let mut public_count = 0;
    let mut private_count = 0;

    for func_id in &file_info.functions {
        let node = graph.get_node(*func_id).unwrap();
        if let Some(codegraph::PropertyValue::String(vis)) = node.properties.get("visibility") {
            match vis.as_str() {
                "public" => public_count += 1,
                "private" => private_count += 1,
                _ => {}
            }
        }
    }

    println!("Public functions: {}", public_count);
    println!("Private functions: {}", private_count);

    // sample_app exports 7 functions (start_link, stop, create_user, get_user, add_role,
    // plus gen_server callbacks init, handle_call, handle_cast, handle_info, terminate)
    assert!(public_count >= 5, "Expected at least 5 public functions");
    // has_role and filter_users are private
    assert!(private_count >= 1, "Expected at least 1 private function");
}

#[test]
fn test_parse_sample_app_records() {
    let parser = ErlangParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.erl"), &mut graph)
        .unwrap();

    // sample_app defines two records: user and state
    assert!(
        file_info.classes.len() >= 2,
        "Expected at least 2 record types, found {}",
        file_info.classes.len()
    );

    let mut record_names = Vec::new();
    for class_id in &file_info.classes {
        let node = graph.get_node(*class_id).unwrap();
        if let Some(codegraph::PropertyValue::String(name)) = node.properties.get("name") {
            record_names.push(name.clone());
        }
    }

    println!("Records found: {:?}", record_names);
    assert!(
        record_names.iter().any(|n| n == "user"),
        "Expected record 'user', found: {:?}",
        record_names
    );
    assert!(
        record_names.iter().any(|n| n == "state"),
        "Expected record 'state', found: {:?}",
        record_names
    );
}

#[test]
fn test_parse_sample_app_behaviour() {
    let parser = ErlangParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.erl"), &mut graph)
        .unwrap();

    // sample_app uses -behaviour(gen_server)
    assert!(
        !file_info.traits.is_empty(),
        "Expected at least 1 behaviour, found 0"
    );

    let mut behaviour_names = Vec::new();
    for trait_id in &file_info.traits {
        let node = graph.get_node(*trait_id).unwrap();
        if let Some(codegraph::PropertyValue::String(name)) = node.properties.get("name") {
            behaviour_names.push(name.clone());
        }
    }

    println!("Behaviours found: {:?}", behaviour_names);
    assert!(
        behaviour_names.iter().any(|n| n == "gen_server"),
        "Expected behaviour 'gen_server', found: {:?}",
        behaviour_names
    );
}

#[test]
fn test_parse_sample_app_imports() {
    let parser = ErlangParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.erl"), &mut graph)
        .unwrap();

    // -import(lists, [member/2, filter/2])
    assert!(
        !file_info.imports.is_empty(),
        "Expected at least 1 import, found 0"
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
        import_names.iter().any(|n| n == "lists"),
        "Expected import 'lists', found: {:?}",
        import_names
    );
}

#[test]
fn test_parse_sample_app_complexity() {
    let parser = ErlangParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.erl"), &mut graph)
        .unwrap();

    // handle_call has nested case expressions → complexity > 1
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
fn test_parse_sample_app_edges() {
    let parser = ErlangParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let _file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.erl"), &mut graph)
        .unwrap();

    let edge_count = graph.edge_count();
    assert!(
        edge_count >= 1,
        "Expected at least 1 graph edge, found {}",
        edge_count
    );

    println!("Total graph edges: {}", edge_count);
}

#[test]
fn test_parse_sample_app_summary() {
    let parser = ErlangParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.erl"), &mut graph)
        .unwrap();

    println!("\n=== Erlang Parser Sample App Summary ===");
    println!("File: sample_app.erl");
    println!("Lines: {}", file_info.line_count);
    println!("Functions: {}", file_info.functions.len());
    println!("Records (classes): {}", file_info.classes.len());
    println!("Behaviours (traits): {}", file_info.traits.len());
    println!("Imports: {}", file_info.imports.len());
    println!("Parse time: {:?}", file_info.parse_time);
    println!("========================================\n");

    assert!(file_info.line_count > 50);
    assert!(!file_info.functions.is_empty());
}
