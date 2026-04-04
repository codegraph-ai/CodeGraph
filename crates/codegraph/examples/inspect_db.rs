// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use codegraph::{CodeGraph, Direction, NodeType};
use std::env;

fn main() {
    let path = env::args().nth(1).unwrap_or_else(|| {
        "/home/administrator/projects/mcp-context-server/.stellarion/graph".to_string()
    });
    let graph = CodeGraph::open(&path).expect("Failed to open graph");
    println!("Node count: {}", graph.node_count());
    println!("Edge count: {}", graph.edge_count());

    // Find src/index.ts node
    println!("\n--- src/index.ts edges ---");
    for (id, node) in graph.iter_nodes() {
        if node.node_type != NodeType::CodeFile {
            continue;
        }
        let rel_path = node.properties.get_string("relative_path").unwrap_or("");
        if rel_path == "src/index.ts" {
            println!(
                "Found: Node {} path={}",
                id,
                node.properties.get_string("path").unwrap_or("?")
            );
            if let Ok(neighbors) = graph.get_neighbors(id, Direction::Outgoing) {
                for n_id in &neighbors {
                    if let Ok(edges) = graph.get_edges_between(id, *n_id) {
                        for e_id in &edges {
                            if let Ok(edge) = graph.get_edge(*e_id) {
                                let target = graph.get_node(edge.target_id).ok();
                                let target_name = target
                                    .as_ref()
                                    .and_then(|n| {
                                        n.properties.get_string("name").map(|s| s.to_string())
                                    })
                                    .unwrap_or_default();
                                let target_type = target
                                    .as_ref()
                                    .map(|n| format!("{}", n.node_type))
                                    .unwrap_or_default();
                                println!(
                                    "  --{}--> [{}:{}]",
                                    edge.edge_type, target_type, target_name
                                );
                            }
                        }
                    }
                }
            }
            break;
        }
    }

    // Show all Module nodes with relative path names (first 20)
    println!("\n--- Module nodes starting with './' ---");
    let mut count = 0;
    for (id, node) in graph.iter_nodes() {
        if node.node_type == NodeType::Module {
            let name = node.properties.get_string("name").unwrap_or("");
            if name.starts_with("./") || name.starts_with("../") {
                println!("  Module {}: name='{}'", id, name);
                count += 1;
                if count >= 20 {
                    break;
                }
            }
        }
    }
}
