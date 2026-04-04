// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Crash recovery integration test
//!
//! Tests that graph can recover from simulated process interruption

use codegraph::{CodeGraph, EdgeType, NodeType, PropertyMap};
use tempfile::TempDir;

#[test]
fn test_crash_recovery_with_wal() {
    let temp_dir = TempDir::new().unwrap();
    let graph_path = temp_dir.path().join("crash_recovery.db");

    // Phase 1: Create graph and add data WITHOUT calling flush()
    // This simulates a process that crashes before explicitly flushing
    let node_ids = {
        let mut graph = CodeGraph::open(&graph_path).unwrap();

        // Add nodes
        let mut node_ids = Vec::new();
        for i in 0..100 {
            let props = PropertyMap::new()
                .with("name", format!("node_{i}"))
                .with("index", i as i64);
            let id = graph.add_node(NodeType::Function, props).unwrap();
            node_ids.push(id);
        }

        // Add edges between consecutive nodes
        for i in 0..99 {
            graph
                .add_edge(
                    node_ids[i],
                    node_ids[i + 1],
                    EdgeType::Calls,
                    PropertyMap::new(),
                )
                .unwrap();
        }

        // Don't call flush() - simulate crash
        // Drop graph without clean shutdown
        node_ids
    };

    // Phase 2: Reopen graph and verify data is recovered
    // RocksDB's WAL should ensure all committed writes are recovered
    let graph = CodeGraph::open(&graph_path).unwrap();

    // Verify all nodes exist
    assert_eq!(graph.node_count(), 100, "All nodes should be recovered");

    for (i, node_id) in node_ids.iter().enumerate() {
        let node = graph.get_node(*node_id).unwrap();
        assert_eq!(node.node_type, NodeType::Function);
        assert_eq!(
            node.properties.get_string("name"),
            Some(&format!("node_{i}")[..])
        );
        assert_eq!(node.properties.get_int("index"), Some(i as i64));
    }

    // Verify all edges exist
    assert_eq!(graph.edge_count(), 99, "All edges should be recovered");

    for i in 0..99 {
        let edges = graph
            .get_edges_between(node_ids[i], node_ids[i + 1])
            .unwrap();
        assert_eq!(edges.len(), 1, "Edge {} -> {} should exist", i, i + 1);

        let edge = graph.get_edge(edges[0]).unwrap();
        assert_eq!(edge.edge_type, EdgeType::Calls);
        assert_eq!(edge.source_id, node_ids[i]);
        assert_eq!(edge.target_id, node_ids[i + 1]);
    }
}

#[test]
fn test_crash_recovery_with_batch_operations() {
    let temp_dir = TempDir::new().unwrap();
    let graph_path = temp_dir.path().join("crash_recovery_batch.db");

    // Phase 1: Use batch operations and simulate crash
    let (node_ids, edge_ids) = {
        let mut graph = CodeGraph::open(&graph_path).unwrap();

        // Create batch of nodes
        let nodes: Vec<_> = (0..50)
            .map(|i| {
                let props = PropertyMap::new().with("batch_id", i as i64);
                (NodeType::Class, props)
            })
            .collect();

        let node_ids = graph.add_nodes_batch(nodes).unwrap();

        // Create batch of edges
        let edges: Vec<_> = (0..49)
            .map(|i| {
                (
                    node_ids[i],
                    node_ids[i + 1],
                    EdgeType::Contains,
                    PropertyMap::new(),
                )
            })
            .collect();

        let edge_ids = graph.add_edges_batch(edges).unwrap();

        // Simulate crash without flush
        (node_ids, edge_ids)
    };

    // Phase 2: Verify batch operations are durable
    let graph = CodeGraph::open(&graph_path).unwrap();

    assert_eq!(graph.node_count(), 50);
    assert_eq!(graph.edge_count(), 49);

    for (i, node_id) in node_ids.iter().enumerate() {
        let node = graph.get_node(*node_id).unwrap();
        assert_eq!(node.properties.get_int("batch_id"), Some(i as i64));
    }

    for edge_id in edge_ids.iter() {
        let edge = graph.get_edge(*edge_id).unwrap();
        assert_eq!(edge.edge_type, EdgeType::Contains);
    }
}

#[test]
fn test_partial_transaction_recovery() {
    let temp_dir = TempDir::new().unwrap();
    let graph_path = temp_dir.path().join("partial_transaction.db");

    // Phase 1: Create initial state
    let file_id = {
        let mut graph = CodeGraph::open(&graph_path).unwrap();
        let props = PropertyMap::new().with("path", "src/main.rs");
        let id = graph.add_node(NodeType::CodeFile, props).unwrap();
        graph.flush().unwrap();
        id
    };

    // Phase 2: Add more data, simulate crash
    let func_id = {
        let mut graph = CodeGraph::open(&graph_path).unwrap();

        // This should succeed and be durable
        let props = PropertyMap::new().with("name", "main");
        let func_id = graph.add_node(NodeType::Function, props).unwrap();

        // This should also be durable (RocksDB commits writes immediately)
        graph
            .add_edge(file_id, func_id, EdgeType::Contains, PropertyMap::new())
            .unwrap();

        // Simulate crash
        func_id
    };

    // Phase 3: Verify partial work is recovered
    let graph = CodeGraph::open(&graph_path).unwrap();

    assert_eq!(graph.node_count(), 2);
    assert_eq!(graph.edge_count(), 1);

    let file = graph.get_node(file_id).unwrap();
    assert_eq!(file.properties.get_string("path"), Some("src/main.rs"));

    let func = graph.get_node(func_id).unwrap();
    assert_eq!(func.properties.get_string("name"), Some("main"));

    let edges = graph.get_edges_between(file_id, func_id).unwrap();
    assert_eq!(edges.len(), 1);
}
