// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Basic usage example for codegraph
//!
//! This example demonstrates:
//! - Creating a graph
//! - Adding nodes and edges
//! - Querying relationships

use codegraph::{CodeGraph, Direction, EdgeType, NodeType, PropertyMap};
use std::path::Path;

fn main() -> codegraph::Result<()> {
    // Create a persistent graph (or use in_memory() for testing)
    let mut graph = CodeGraph::open(Path::new("./example.graph"))?;

    println!("Creating a simple code graph...\n");

    // Add a file node
    let file_props = PropertyMap::new()
        .with("path", "src/main.rs")
        .with("language", "rust");
    let file_id = graph.add_node(NodeType::CodeFile, file_props)?;
    println!("✓ Added file: src/main.rs (ID: {file_id})");

    // Add a function node
    let main_props = PropertyMap::new()
        .with("name", "main")
        .with("line_start", 1i64)
        .with("line_end", 10i64)
        .with("visibility", "public");
    let main_id = graph.add_node(NodeType::Function, main_props)?;
    println!("✓ Added function: main (ID: {main_id})");

    // Add another function node
    let helper_props = PropertyMap::new()
        .with("name", "helper")
        .with("line_start", 12i64)
        .with("line_end", 20i64)
        .with("is_async", false);
    let helper_id = graph.add_node(NodeType::Function, helper_props)?;
    println!("✓ Added function: helper (ID: {helper_id})");

    // Add a variable node
    let var_props = PropertyMap::new()
        .with("name", "data")
        .with("type", "String");
    let var_id = graph.add_node(NodeType::Variable, var_props)?;
    println!("✓ Added variable: data (ID: {var_id})");

    // Create relationships
    graph.add_edge(file_id, main_id, EdgeType::Contains, PropertyMap::new())?;
    println!("✓ Added edge: file contains main");

    graph.add_edge(file_id, helper_id, EdgeType::Contains, PropertyMap::new())?;
    println!("✓ Added edge: file contains helper");

    let call_props = PropertyMap::new().with("line", 5i64);
    graph.add_edge(main_id, helper_id, EdgeType::Calls, call_props)?;
    println!("✓ Added edge: main calls helper");

    graph.add_edge(main_id, var_id, EdgeType::References, PropertyMap::new())?;
    println!("✓ Added edge: main references data");

    // Query the graph
    println!("\n--- Querying the graph ---\n");

    let file_node = graph.get_node(file_id)?;
    println!(
        "File node: {}",
        file_node.properties.get_string("path").unwrap_or("unknown")
    );

    let neighbors = graph.get_neighbors(main_id, Direction::Outgoing)?;
    println!(
        "\nMain function calls/references {} entities:",
        neighbors.len()
    );
    for neighbor_id in &neighbors {
        let neighbor = graph.get_node(*neighbor_id)?;
        if let Some(name) = neighbor.properties.get_string("name") {
            println!("  - {} (type: {})", name, neighbor.node_type);
        }
    }

    // Get incoming edges (who calls main?)
    let callers = graph.get_neighbors(main_id, Direction::Incoming)?;
    println!(
        "\nMain function is contained in {} entities:",
        callers.len()
    );
    for caller_id in &callers {
        let caller = graph.get_node(*caller_id)?;
        println!("  - {:?}", caller.node_type);
    }

    // Get edges between specific nodes
    let edges = graph.get_edges_between(main_id, helper_id)?;
    println!("\nEdges from main to helper: {}", edges.len());
    for edge_id in &edges {
        let edge = graph.get_edge(*edge_id)?;
        println!(
            "  - {} (line: {:?})",
            edge.edge_type,
            edge.properties.get_int("line")
        );
    }

    // Graph statistics
    println!("\n--- Graph Statistics ---\n");
    println!("Total nodes: {}", graph.node_count());
    println!("Total edges: {}", graph.edge_count());

    // Persist changes
    graph.flush()?;
    println!("\n✓ Graph persisted to disk");

    Ok(())
}
