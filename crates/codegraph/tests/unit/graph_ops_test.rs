// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for core graph operations (add_node, get_node, add_edge, etc.).

use codegraph::{CodeGraph, Direction, EdgeType, NodeType, PropertyMap};

#[test]
fn test_add_node() {
    let mut graph = CodeGraph::in_memory().unwrap();

    let props = PropertyMap::new().with("name", "test_func");
    let node_id = graph.add_node(NodeType::Function, props).unwrap();

    assert_eq!(node_id, 0); // First node should have ID 0

    let node = graph.get_node(node_id).unwrap();
    assert_eq!(node.node_type, NodeType::Function);
    assert_eq!(node.properties.get_string("name"), Some("test_func"));
}

#[test]
fn test_get_node_and_get_node_mut() {
    let mut graph = CodeGraph::in_memory().unwrap();

    let node_id = graph
        .add_node(NodeType::Function, PropertyMap::new())
        .unwrap();

    // Test immutable get
    let node = graph.get_node(node_id).unwrap();
    assert_eq!(node.id, node_id);

    // Test mutable get
    let node_mut = graph.get_node_mut(node_id).unwrap();
    node_mut.set_property("test", "value");

    let node = graph.get_node(node_id).unwrap();
    assert_eq!(node.properties.get_string("test"), Some("value"));
}

#[test]
fn test_get_nonexistent_node() {
    let graph = CodeGraph::in_memory().unwrap();

    let result = graph.get_node(999);
    assert!(result.is_err());
}

#[test]
fn test_add_edge_with_validation() {
    let mut graph = CodeGraph::in_memory().unwrap();

    let source_id = graph
        .add_node(NodeType::Function, PropertyMap::new())
        .unwrap();
    let target_id = graph
        .add_node(NodeType::Function, PropertyMap::new())
        .unwrap();

    let props = PropertyMap::new().with("line", 42i64);
    let edge_id = graph
        .add_edge(source_id, target_id, EdgeType::Calls, props)
        .unwrap();

    assert_eq!(edge_id, 0); // First edge should have ID 0

    let edge = graph.get_edge(edge_id).unwrap();
    assert_eq!(edge.source_id, source_id);
    assert_eq!(edge.target_id, target_id);
    assert_eq!(edge.edge_type, EdgeType::Calls);
}

#[test]
fn test_add_edge_with_missing_nodes() {
    let mut graph = CodeGraph::in_memory().unwrap();

    // Try to add edge with non-existent nodes
    let result = graph.add_edge(100, 200, EdgeType::Calls, PropertyMap::new());
    assert!(result.is_err());
}

#[test]
fn test_get_neighbors_outgoing() {
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = graph
        .add_node(NodeType::Function, PropertyMap::new())
        .unwrap();
    let target1 = graph
        .add_node(NodeType::Function, PropertyMap::new())
        .unwrap();
    let target2 = graph
        .add_node(NodeType::Function, PropertyMap::new())
        .unwrap();

    graph
        .add_edge(source, target1, EdgeType::Calls, PropertyMap::new())
        .unwrap();
    graph
        .add_edge(source, target2, EdgeType::Calls, PropertyMap::new())
        .unwrap();

    let neighbors = graph.get_neighbors(source, Direction::Outgoing).unwrap();
    assert_eq!(neighbors.len(), 2);
    assert!(neighbors.contains(&target1));
    assert!(neighbors.contains(&target2));
}

#[test]
fn test_get_neighbors_incoming() {
    let mut graph = CodeGraph::in_memory().unwrap();

    let target = graph
        .add_node(NodeType::Function, PropertyMap::new())
        .unwrap();
    let source1 = graph
        .add_node(NodeType::Function, PropertyMap::new())
        .unwrap();
    let source2 = graph
        .add_node(NodeType::Function, PropertyMap::new())
        .unwrap();

    graph
        .add_edge(source1, target, EdgeType::Calls, PropertyMap::new())
        .unwrap();
    graph
        .add_edge(source2, target, EdgeType::Calls, PropertyMap::new())
        .unwrap();

    let neighbors = graph.get_neighbors(target, Direction::Incoming).unwrap();
    assert_eq!(neighbors.len(), 2);
    assert!(neighbors.contains(&source1));
    assert!(neighbors.contains(&source2));
}

#[test]
fn test_get_neighbors_both_directions() {
    let mut graph = CodeGraph::in_memory().unwrap();

    let node_a = graph
        .add_node(NodeType::Function, PropertyMap::new())
        .unwrap();
    let node_b = graph
        .add_node(NodeType::Function, PropertyMap::new())
        .unwrap();
    let node_c = graph
        .add_node(NodeType::Function, PropertyMap::new())
        .unwrap();

    graph
        .add_edge(node_a, node_b, EdgeType::Calls, PropertyMap::new())
        .unwrap();
    graph
        .add_edge(node_c, node_a, EdgeType::Calls, PropertyMap::new())
        .unwrap();

    let neighbors = graph.get_neighbors(node_a, Direction::Both).unwrap();
    assert_eq!(neighbors.len(), 2);
    assert!(neighbors.contains(&node_b));
    assert!(neighbors.contains(&node_c));
}

#[test]
fn test_delete_node_cascades_to_edges() {
    let mut graph = CodeGraph::in_memory().unwrap();

    let node_a = graph
        .add_node(NodeType::Function, PropertyMap::new())
        .unwrap();
    let node_b = graph
        .add_node(NodeType::Function, PropertyMap::new())
        .unwrap();
    let node_c = graph
        .add_node(NodeType::Function, PropertyMap::new())
        .unwrap();

    let edge1 = graph
        .add_edge(node_a, node_b, EdgeType::Calls, PropertyMap::new())
        .unwrap();
    let edge2 = graph
        .add_edge(node_c, node_a, EdgeType::Calls, PropertyMap::new())
        .unwrap();

    // Delete node_a should cascade to edges
    graph.delete_node(node_a).unwrap();

    // Node should be gone
    assert!(graph.get_node(node_a).is_err());

    // Edges should be gone
    assert!(graph.get_edge(edge1).is_err());
    assert!(graph.get_edge(edge2).is_err());
}

#[test]
fn test_batch_add_nodes() {
    let mut graph = CodeGraph::in_memory().unwrap();

    let nodes = vec![
        (NodeType::Function, PropertyMap::new().with("name", "func1")),
        (NodeType::Function, PropertyMap::new().with("name", "func2")),
        (NodeType::Function, PropertyMap::new().with("name", "func3")),
    ];

    let node_ids = graph.add_nodes_batch(nodes).unwrap();

    assert_eq!(node_ids.len(), 3);
    assert_eq!(graph.node_count(), 3);

    // Verify nodes were created
    for node_id in node_ids {
        assert!(graph.get_node(node_id).is_ok());
    }
}

#[test]
fn test_batch_add_edges() {
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = graph
        .add_node(NodeType::Function, PropertyMap::new())
        .unwrap();
    let target1 = graph
        .add_node(NodeType::Function, PropertyMap::new())
        .unwrap();
    let target2 = graph
        .add_node(NodeType::Function, PropertyMap::new())
        .unwrap();

    let edges = vec![
        (source, target1, EdgeType::Calls, PropertyMap::new()),
        (source, target2, EdgeType::Calls, PropertyMap::new()),
    ];

    let edge_ids = graph.add_edges_batch(edges).unwrap();

    assert_eq!(edge_ids.len(), 2);
    assert_eq!(graph.edge_count(), 2);

    // Verify edges were created
    for edge_id in edge_ids {
        assert!(graph.get_edge(edge_id).is_ok());
    }
}
