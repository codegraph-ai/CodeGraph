// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Integration test for large graph handling (100K nodes, 500K edges).

use codegraph::{CodeGraph, EdgeType, NodeType, PropertyMap};

#[test]
#[ignore] // This test is slow, run with --ignored flag
fn test_large_graph_100k_nodes_500k_edges() {
    let mut graph = CodeGraph::in_memory().unwrap();

    let num_nodes = 100_000;
    let edges_per_node = 5;

    // Add 100K nodes
    let node_ids: Vec<_> = (0..num_nodes)
        .map(|i| {
            graph
                .add_node(
                    NodeType::Function,
                    PropertyMap::new().with("name", format!("func_{i}")),
                )
                .unwrap()
        })
        .collect();

    assert_eq!(graph.node_count(), num_nodes);

    // Add ~500K edges (5 edges per node on average)
    let mut edge_count = 0;
    for i in 0..num_nodes {
        for j in 1..=edges_per_node {
            let target_idx = (i + j) % num_nodes;
            graph
                .add_edge(
                    node_ids[i],
                    node_ids[target_idx],
                    EdgeType::Calls,
                    PropertyMap::new(),
                )
                .unwrap();
            edge_count += 1;
        }
    }

    assert_eq!(graph.edge_count(), edge_count);

    // Test random access performance
    let mid_node = node_ids[num_nodes / 2];
    let node = graph.get_node(mid_node).unwrap();
    assert_eq!(node.id, mid_node);
}

#[test]
fn test_medium_graph_10k_nodes() {
    let mut graph = CodeGraph::in_memory().unwrap();

    let num_nodes = 10_000;

    // Batch add 10K nodes
    let nodes: Vec<_> = (0..num_nodes)
        .map(|i| {
            (
                NodeType::Function,
                PropertyMap::new().with("index", i as i64),
            )
        })
        .collect();

    let node_ids = graph.add_nodes_batch(nodes).unwrap();

    assert_eq!(node_ids.len(), num_nodes);
    assert_eq!(graph.node_count(), num_nodes);

    // Verify random access
    let sample_id = node_ids[5000];
    let node = graph.get_node(sample_id).unwrap();
    assert_eq!(node.properties.get_int("index"), Some(5000));
}
