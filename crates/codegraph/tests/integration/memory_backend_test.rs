// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Integration test for MemoryBackend save/load operations.

use codegraph::{CodeGraph, EdgeType, NodeType, PropertyMap};

#[test]
fn test_memory_backend_in_memory_only() {
    let mut graph = CodeGraph::in_memory().unwrap();

    // Add some nodes and edges
    let node1 = graph
        .add_node(NodeType::Function, PropertyMap::new().with("name", "func1"))
        .unwrap();
    let node2 = graph
        .add_node(NodeType::Function, PropertyMap::new().with("name", "func2"))
        .unwrap();
    graph
        .add_edge(node1, node2, EdgeType::Calls, PropertyMap::new())
        .unwrap();

    assert_eq!(graph.node_count(), 2);
    assert_eq!(graph.edge_count(), 1);

    // Memory backend doesn't persist - just verify operations work
    graph.flush().unwrap();
}

#[test]
fn test_memory_backend_clear() {
    let mut graph = CodeGraph::in_memory().unwrap();

    // Add data
    let node1 = graph
        .add_node(NodeType::Function, PropertyMap::new())
        .unwrap();
    let node2 = graph
        .add_node(NodeType::Function, PropertyMap::new())
        .unwrap();
    graph
        .add_edge(node1, node2, EdgeType::Calls, PropertyMap::new())
        .unwrap();

    assert_eq!(graph.node_count(), 2);
    assert_eq!(graph.edge_count(), 1);

    // Clear
    graph.clear().unwrap();

    assert_eq!(graph.node_count(), 0);
    assert_eq!(graph.edge_count(), 0);
}
