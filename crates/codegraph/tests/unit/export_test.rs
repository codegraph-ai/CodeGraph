// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for export functionality (TDD - written FIRST)
//!
//! Tests cover:
//! - T106: export_dot() output validity
//! - T107: export_dot_styled() with options
//! - T108: export_json() D3.js compatibility
//! - T109: export_json_filtered()
//! - T110: export_csv_nodes()
//! - T111: export_csv_edges()
//! - T112: export_triples() RDF format
//! - T113: Size limit warnings (>10K nodes)

use codegraph::{helpers, CodeGraph, EdgeType, NodeType};
use std::fs;
use tempfile::TempDir;

// Helper to create test graph
fn create_test_graph() -> codegraph::Result<CodeGraph> {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.graph");
    let mut graph = CodeGraph::open(&db_path)?;

    // Create a small code graph
    let main_file = helpers::add_file(&mut graph, "src/main.rs", "rust")?;
    let lib_file = helpers::add_file(&mut graph, "src/lib.rs", "rust")?;

    let main_fn = helpers::add_function_with_metadata(
        &mut graph,
        main_file,
        helpers::FunctionMetadata {
            name: "main",
            line_start: 1,
            line_end: 10,
            visibility: "public",
            signature: "fn main()",
            is_async: false,
            is_test: false,
        },
    )?;

    let helper_fn = helpers::add_function_with_metadata(
        &mut graph,
        lib_file,
        helpers::FunctionMetadata {
            name: "helper",
            line_start: 5,
            line_end: 15,
            visibility: "private",
            signature: "fn helper()",
            is_async: false,
            is_test: false,
        },
    )?;

    helpers::add_call(&mut graph, main_fn, helper_fn, 5)?;
    helpers::add_import(&mut graph, main_file, lib_file, vec!["helper"])?;

    Ok(graph)
}

// T106: Test export_dot() produces valid Graphviz format
#[test]
fn test_export_dot_validity() {
    let graph = create_test_graph().unwrap();

    let dot = graph.export_dot().unwrap();

    // Verify DOT format structure
    assert!(dot.starts_with("digraph code_graph {"));
    assert!(dot.contains("rankdir="));
    assert!(dot.contains("[label="));
    assert!(dot.contains("->"));
    assert!(dot.ends_with("}\n"));

    // Verify node labels
    assert!(dot.contains("src/main.rs") || dot.contains("main.rs"));
    assert!(dot.contains("src/lib.rs") || dot.contains("lib.rs"));
    assert!(dot.contains("main"));
    assert!(dot.contains("helper"));

    // Verify edge labels
    assert!(dot.contains("Calls") || dot.contains("calls"));
    assert!(dot.contains("Contains") || dot.contains("contains"));
    // Check for import edges (can be Imports or ImportsFrom)
    assert!(dot.contains("Imports") || dot.contains("ImportsFrom") || dot.contains("imports"));
}

// T107: Test export_dot_styled() with custom options
#[test]
fn test_export_dot_styled() {
    let graph = create_test_graph().unwrap();

    let mut node_colors = std::collections::HashMap::new();
    node_colors.insert(NodeType::CodeFile, "#E0E0E0".to_string());
    node_colors.insert(NodeType::Function, "#90CAF9".to_string());

    let mut edge_colors = std::collections::HashMap::new();
    edge_colors.insert(EdgeType::Calls, "#FF5252".to_string());

    let mut node_shapes = std::collections::HashMap::new();
    node_shapes.insert(NodeType::CodeFile, "folder".to_string());
    node_shapes.insert(NodeType::Function, "box".to_string());

    use codegraph::export::DotOptions;
    let options = DotOptions {
        node_colors,
        edge_colors,
        node_shapes,
        rankdir: "TB".to_string(),
        show_properties: vec!["visibility".to_string()],
    };

    let dot = graph.export_dot_styled(options).unwrap();

    // Verify styling applied
    assert!(dot.contains("rankdir=TB"));
    assert!(dot.contains("fillcolor=") || dot.contains("color="));
    assert!(dot.contains("shape="));
    assert!(dot.contains("visibility") || dot.contains("public") || dot.contains("private"));
}

// T108: Test export_json() produces D3.js compatible format
#[test]
fn test_export_json_d3_compatibility() {
    let graph = create_test_graph().unwrap();

    let json = graph.export_json().unwrap();

    // Parse JSON
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();

    // Verify structure
    assert!(value.is_object());
    assert!(value["nodes"].is_array());
    assert!(value["links"].is_array());

    let nodes = value["nodes"].as_array().unwrap();
    let links = value["links"].as_array().unwrap();

    // Verify nodes have required fields
    assert!(nodes.len() >= 4); // 2 files + 2 functions
    for node in nodes {
        assert!(node["id"].is_number());
        assert!(node["type"].is_string());
        assert!(node["properties"].is_object());
    }

    // Verify links have required fields for D3.js
    assert!(links.len() >= 3); // Contains, Contains, Calls, ImportsFrom
    for link in links {
        assert!(link["source"].is_number());
        assert!(link["target"].is_number());
        assert!(link["type"].is_string());
    }
}

// T109: Test export_json_filtered() exports subset
#[test]
fn test_export_json_filtered() {
    let graph = create_test_graph().unwrap();

    // Export only functions
    let json = graph
        .export_json_filtered(|node| node.node_type == NodeType::Function, true)
        .unwrap();

    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let nodes = value["nodes"].as_array().unwrap();

    // Verify only functions exported
    assert_eq!(nodes.len(), 2); // main and helper
    for node in nodes {
        assert_eq!(node["type"].as_str().unwrap(), "Function");
    }

    // Verify edges between functions included
    let links = value["links"].as_array().unwrap();
    assert!(!links.is_empty()); // At least the Calls edge
}

// T110: Test export_csv_nodes() creates valid CSV
#[test]
fn test_export_csv_nodes() {
    let graph = create_test_graph().unwrap();

    let temp_dir = TempDir::new().unwrap();
    let csv_path = temp_dir.path().join("nodes.csv");

    graph.export_csv_nodes(&csv_path).unwrap();

    // Read and verify CSV
    let content = fs::read_to_string(&csv_path).unwrap();
    let lines: Vec<&str> = content.lines().collect();

    // Verify header
    assert!(lines[0].contains("id"));
    assert!(lines[0].contains("type"));

    // Verify at least 4 nodes (2 files + 2 functions)
    assert!(lines.len() >= 5); // header + 4 rows

    // Verify CSV format
    for line in &lines[1..] {
        let fields: Vec<&str> = line.split(',').collect();
        assert!(fields.len() >= 2); // At least id and type
    }
}

// T111: Test export_csv_edges() creates valid CSV
#[test]
fn test_export_csv_edges() {
    let graph = create_test_graph().unwrap();

    let temp_dir = TempDir::new().unwrap();
    let csv_path = temp_dir.path().join("edges.csv");

    graph.export_csv_edges(&csv_path).unwrap();

    // Read and verify CSV
    let content = fs::read_to_string(&csv_path).unwrap();
    let lines: Vec<&str> = content.lines().collect();

    // Verify header
    assert!(lines[0].contains("id"));
    assert!(lines[0].contains("source"));
    assert!(lines[0].contains("target"));
    assert!(lines[0].contains("type"));

    // Verify at least 3 edges (2 Contains + 1 Calls + 1 ImportsFrom)
    assert!(lines.len() >= 4); // header + 3+ rows

    // Verify CSV format
    for line in &lines[1..] {
        let fields: Vec<&str> = line.split(',').collect();
        assert!(fields.len() >= 4); // id, source, target, type
    }
}

// T112: Test export_triples() produces valid RDF N-Triples
#[test]
fn test_export_triples_rdf_format() {
    let graph = create_test_graph().unwrap();

    let triples = graph.export_triples().unwrap();

    // Verify N-Triples format (each line ends with " .")
    let lines: Vec<&str> = triples.lines().collect();
    assert!(!lines.is_empty());

    for line in lines {
        if !line.trim().is_empty() {
            assert!(line.trim().ends_with(" ."));
            // Basic triple structure: <subject> <predicate> <object> .
            let parts: Vec<&str> = line.split_whitespace().collect();
            assert!(parts.len() >= 4); // subject, predicate, object, dot
            assert!(parts[0].starts_with('<'));
            assert!(parts[1].starts_with('<'));
        }
    }

    // Verify contains node types
    assert!(triples.contains("<rdf:type>"));
    assert!(triples.contains("CodeFile") || triples.contains("type:CodeFile"));
    assert!(triples.contains("Function") || triples.contains("type:Function"));

    // Verify contains edges
    assert!(triples.contains("Calls") || triples.contains("edge:Calls"));
    assert!(triples.contains("Contains") || triples.contains("edge:Contains"));
}

// T113: Test size limit warnings for large graphs
#[test]
fn test_size_limit_warnings() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("large.graph");
    let mut graph = CodeGraph::open(&db_path).unwrap();

    // Create a graph with >10K nodes
    let file = helpers::add_file(&mut graph, "test.rs", "rust").unwrap();
    for i in 0..10_100 {
        let name = format!("func_{i}");
        let signature = format!("fn func_{i}()");
        let _ = helpers::add_function_with_metadata(
            &mut graph,
            file,
            helpers::FunctionMetadata {
                name: &name,
                line_start: i * 10,
                line_end: i * 10 + 5,
                visibility: "public",
                signature: &signature,
                is_async: false,
                is_test: false,
            },
        );
    }

    // Should succeed but potentially with warnings
    let result = graph.export_dot();
    assert!(result.is_ok());

    let result_json = graph.export_json();
    assert!(result_json.is_ok());
}

// Additional test: export_csv() convenience method
#[test]
fn test_export_csv_convenience() {
    let graph = create_test_graph().unwrap();

    let temp_dir = TempDir::new().unwrap();
    let nodes_path = temp_dir.path().join("nodes.csv");
    let edges_path = temp_dir.path().join("edges.csv");

    graph.export_csv(&nodes_path, &edges_path).unwrap();

    // Verify both files exist
    assert!(nodes_path.exists());
    assert!(edges_path.exists());

    // Verify content
    let nodes_content = fs::read_to_string(&nodes_path).unwrap();
    let edges_content = fs::read_to_string(&edges_path).unwrap();

    assert!(!nodes_content.is_empty());
    assert!(!edges_content.is_empty());
}
