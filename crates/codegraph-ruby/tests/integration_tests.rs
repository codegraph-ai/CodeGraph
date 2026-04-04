// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for Ruby parser

use codegraph::{CodeGraph, NodeType};
use codegraph_parser_api::CodeParser;
use codegraph_ruby::RubyParser;
use std::path::Path;

const SAMPLE_APP: &str = include_str!("fixtures/sample_app.rb");

#[test]
fn test_parse_sample_app_classes() {
    let parser = RubyParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.rb"), &mut graph)
        .unwrap();

    // Should find Entity, User, Product, UserService classes
    assert!(
        file_info.classes.len() >= 4,
        "Expected at least 4 classes, found {}",
        file_info.classes.len()
    );

    // Verify class names exist in graph
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

    println!("Classes found: {:?}", class_names);
}

#[test]
fn test_parse_sample_app_modules() {
    let parser = RubyParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.rb"), &mut graph)
        .unwrap();

    // Should find MyApp, Models, Services modules
    assert!(
        file_info.traits.len() >= 3,
        "Expected at least 3 modules, found {}",
        file_info.traits.len()
    );

    let mut module_names = Vec::new();
    for trait_id in &file_info.traits {
        let node = graph.get_node(*trait_id).unwrap();
        if let Some(codegraph::PropertyValue::String(name)) = node.properties.get("name") {
            module_names.push(name.clone());
        }
    }

    assert!(
        module_names.iter().any(|n| n.contains("MyApp")),
        "Should contain MyApp module"
    );
    assert!(
        module_names.iter().any(|n| n.contains("Models")),
        "Should contain Models module"
    );
    assert!(
        module_names.iter().any(|n| n.contains("Services")),
        "Should contain Services module"
    );

    println!("Modules found: {:?}", module_names);
}

#[test]
fn test_parse_sample_app_methods() {
    let parser = RubyParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.rb"), &mut graph)
        .unwrap();

    // Should find multiple methods
    assert!(
        file_info.functions.len() >= 10,
        "Expected at least 10 methods, found {}",
        file_info.functions.len()
    );

    let mut method_names = Vec::new();
    for func_id in &file_info.functions {
        let node = graph.get_node(*func_id).unwrap();
        if let Some(codegraph::PropertyValue::String(name)) = node.properties.get("name") {
            method_names.push(name.clone());
        }
    }

    // Check for some specific methods
    assert!(
        method_names.iter().any(|n| n.contains("initialize")),
        "Should contain initialize method"
    );
    assert!(
        method_names.iter().any(|n| n.contains("add_role")),
        "Should contain add_role method"
    );
    assert!(
        method_names.iter().any(|n| n.contains("create_user")),
        "Should contain create_user method"
    );

    println!("Methods found: {} total", method_names.len());
    println!(
        "Sample methods: {:?}",
        &method_names[..method_names.len().min(10)]
    );
}

#[test]
fn test_parse_sample_app_imports() {
    let parser = RubyParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.rb"), &mut graph)
        .unwrap();

    // Should find require 'json' and require_relative './helper'
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
        import_names.iter().any(|n| n.contains("json")),
        "Should contain json import"
    );
    assert!(
        import_names.iter().any(|n| n.contains("helper")),
        "Should contain helper import"
    );

    println!("Imports found: {:?}", import_names);
}

#[test]
fn test_parse_sample_app_inheritance() {
    let parser = RubyParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let _file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.rb"), &mut graph)
        .unwrap();

    // Check that graph has edges (inheritance relationships should exist)
    let edge_count = graph.edge_count();
    assert!(
        edge_count >= 2,
        "Expected at least 2 edges (inheritance relationships), found {}",
        edge_count
    );

    println!("Total edges found: {}", edge_count);
}

#[test]
fn test_parse_sample_app_summary() {
    let parser = RubyParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.rb"), &mut graph)
        .unwrap();

    println!("\n=== Ruby Parser Sample App Summary ===");
    println!("File: sample_app.rb");
    println!("Lines: {}", file_info.line_count);
    println!("Classes: {}", file_info.classes.len());
    println!("Modules (traits): {}", file_info.traits.len());
    println!("Methods (functions): {}", file_info.functions.len());
    println!("Imports: {}", file_info.imports.len());
    println!("Parse time: {:?}", file_info.parse_time);
    println!("=====================================\n");

    // Basic sanity checks
    assert!(file_info.line_count > 50);
    assert!(!file_info.classes.is_empty());
    assert!(!file_info.traits.is_empty());
    assert!(!file_info.functions.is_empty());
}

#[test]
fn test_require_vs_require_relative_is_external() {
    let source = r#"
require 'json'
require 'net/http'
require_relative './helper'
require_relative '../lib/utils'
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = RubyParser::new();
    let result = parser.parse_source(source, Path::new("test.rb"), &mut graph);
    assert!(result.is_ok());

    let file_info = result.unwrap();
    assert_eq!(file_info.imports.len(), 4);

    // Check Module nodes for is_external property
    let module_ids = graph.query().node_type(NodeType::Module).execute().unwrap();
    assert_eq!(module_ids.len(), 4);

    let mut external_count = 0;
    let mut relative_count = 0;

    for id in &module_ids {
        let node = graph.get_node(*id).unwrap();
        let name = node.properties.get_string("name").unwrap();
        let is_external = node.properties.get_string("is_external") == Some("true");

        match name {
            "json" | "net/http" => {
                assert!(is_external, "'{}' should be external", name);
                external_count += 1;
            }
            "./helper" | "../lib/utils" => {
                assert!(!is_external, "'{}' should not be external", name);
                relative_count += 1;
            }
            _ => panic!("Unexpected module: {}", name),
        }
    }

    assert_eq!(external_count, 2, "Expected 2 external requires");
    assert_eq!(relative_count, 2, "Expected 2 require_relative");
}

#[test]
fn test_syntax_error() {
    let source = r#"
def broken(
    puts "missing closing paren"
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = RubyParser::new();

    let result = parser.parse_source(source, Path::new("test.rb"), &mut graph);
    assert!(result.is_err());
}

#[test]
fn test_calls_edges() {
    use codegraph::{Direction, EdgeType};

    let parser = RubyParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"
class Calculator
  def caller
    helper
  end

  def helper
    42
  end
end
"#;

    let result = parser.parse_source(source, Path::new("test.rb"), &mut graph);
    assert!(result.is_ok());

    let caller_id = graph
        .query()
        .node_type(codegraph::NodeType::Function)
        .execute()
        .unwrap()
        .into_iter()
        .find(|&id| {
            graph
                .get_node(id)
                .map(|n| n.properties.get_string("name") == Some("caller"))
                .unwrap_or(false)
        })
        .expect("Should find 'caller' function");

    let callees: Vec<String> = graph
        .get_neighbors(caller_id, Direction::Outgoing)
        .unwrap_or_default()
        .iter()
        .filter(|&&neighbor_id| {
            graph
                .get_edges_between(caller_id, neighbor_id)
                .unwrap_or_default()
                .iter()
                .any(|&e| {
                    graph
                        .get_edge(e)
                        .map(|edge| edge.edge_type == EdgeType::Calls)
                        .unwrap_or(false)
                })
        })
        .map(|&id| {
            graph
                .get_node(id)
                .map(|n| n.properties.get_string("name").unwrap_or("?").to_string())
                .unwrap_or_default()
        })
        .collect();

    assert!(
        callees.contains(&"helper".to_string()),
        "Should have Calls edge to 'helper', got: {:?}",
        callees
    );
}
