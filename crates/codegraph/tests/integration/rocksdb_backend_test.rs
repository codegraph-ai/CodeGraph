// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Integration test for RocksDBBackend persistence across sessions.

use codegraph::{CodeGraph, Direction, EdgeType, NodeType, PropertyMap};
use tempfile::TempDir;

#[test]
fn test_rocksdb_persistence() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.graph");

    let node1_id;
    let node2_id;
    let edge_id;

    // Create graph and add data
    {
        let mut graph = CodeGraph::open(&db_path).unwrap();

        node1_id = graph
            .add_node(
                NodeType::CodeFile,
                PropertyMap::new().with("path", "main.rs"),
            )
            .unwrap();
        node2_id = graph
            .add_node(NodeType::Function, PropertyMap::new().with("name", "main"))
            .unwrap();
        edge_id = graph
            .add_edge(node1_id, node2_id, EdgeType::Contains, PropertyMap::new())
            .unwrap();

        graph.flush().unwrap();
    }

    // Reopen and verify data persisted
    {
        let graph = CodeGraph::open(&db_path).unwrap();

        assert_eq!(graph.node_count(), 2);
        assert_eq!(graph.edge_count(), 1);

        let node1 = graph.get_node(node1_id).unwrap();
        assert_eq!(node1.node_type, NodeType::CodeFile);
        assert_eq!(node1.properties.get_string("path"), Some("main.rs"));

        let node2 = graph.get_node(node2_id).unwrap();
        assert_eq!(node2.node_type, NodeType::Function);
        assert_eq!(node2.properties.get_string("name"), Some("main"));

        let edge = graph.get_edge(edge_id).unwrap();
        assert_eq!(edge.source_id, node1_id);
        assert_eq!(edge.target_id, node2_id);
        assert_eq!(edge.edge_type, EdgeType::Contains);

        // Verify indexes were rebuilt
        let neighbors = graph.get_neighbors(node1_id, Direction::Outgoing).unwrap();
        assert_eq!(neighbors.len(), 1);
        assert!(neighbors.contains(&node2_id));
    }
}

#[test]
fn test_rocksdb_counter_persistence() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.graph");

    let last_node_id;

    // Create graph and add nodes
    {
        let mut graph = CodeGraph::open(&db_path).unwrap();

        graph
            .add_node(NodeType::Function, PropertyMap::new())
            .unwrap();
        graph
            .add_node(NodeType::Function, PropertyMap::new())
            .unwrap();
        last_node_id = graph
            .add_node(NodeType::Function, PropertyMap::new())
            .unwrap();

        assert_eq!(last_node_id, 2); // Third node should have ID 2

        graph.flush().unwrap();
    }

    // Reopen and add more nodes - IDs should continue from where they left off
    {
        let mut graph = CodeGraph::open(&db_path).unwrap();

        let new_node_id = graph
            .add_node(NodeType::Function, PropertyMap::new())
            .unwrap();
        assert_eq!(new_node_id, 3); // Should continue from 3, not restart at 0

        assert_eq!(graph.node_count(), 4);
    }
}

#[test]
fn test_rocksdb_delete_and_reopen() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.graph");

    let node1_id;
    let node2_id;
    let node3_id;

    // Create graph, add nodes, delete one
    {
        let mut graph = CodeGraph::open(&db_path).unwrap();

        node1_id = graph
            .add_node(NodeType::Function, PropertyMap::new())
            .unwrap();
        node2_id = graph
            .add_node(NodeType::Function, PropertyMap::new())
            .unwrap();
        node3_id = graph
            .add_node(NodeType::Function, PropertyMap::new())
            .unwrap();

        graph
            .add_edge(node1_id, node2_id, EdgeType::Calls, PropertyMap::new())
            .unwrap();
        graph
            .add_edge(node2_id, node3_id, EdgeType::Calls, PropertyMap::new())
            .unwrap();

        // Delete middle node
        graph.delete_node(node2_id).unwrap();

        graph.flush().unwrap();
    }

    // Reopen and verify deletion persisted
    {
        let graph = CodeGraph::open(&db_path).unwrap();

        assert_eq!(graph.node_count(), 2);
        assert_eq!(graph.edge_count(), 0); // Both edges should be deleted

        assert!(graph.get_node(node1_id).is_ok());
        assert!(graph.get_node(node2_id).is_err()); // Should be deleted
        assert!(graph.get_node(node3_id).is_ok());
    }
}
