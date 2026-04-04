// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! JSON format export for D3.js and web visualization tools.
//!
//! Generates JSON with "nodes" and "links" arrays compatible with D3.js force-directed layouts.

use crate::{CodeGraph, Node, Result};
use serde_json::{json, Value};
use std::collections::HashSet;

/// Export graph to D3.js-compatible JSON format
pub fn export_json(graph: &CodeGraph) -> Result<String> {
    let mut nodes_array = Vec::new();
    let mut links_array = Vec::new();

    // Export all nodes
    for node_id in 0..graph.node_count() as u64 {
        if let Ok(node) = graph.get_node(node_id) {
            nodes_array.push(node_to_json(node_id, node));
        }
    }

    // Export all edges
    for edge_id in 0..graph.edge_count() as u64 {
        if let Ok(edge) = graph.get_edge(edge_id) {
            links_array.push(json!({
                "id": edge_id,
                "source": edge.source_id,
                "target": edge.target_id,
                "type": format!("{:?}", edge.edge_type),
                "properties": properties_to_json(&edge.properties),
            }));
        }
    }

    let result = json!({
        "nodes": nodes_array,
        "links": links_array,
    });

    // serde_json::to_string_pretty should never fail for our data structures
    Ok(serde_json::to_string_pretty(&result).expect("Failed to serialize JSON"))
}

/// Export filtered subset of graph to JSON
pub fn export_json_filtered(
    graph: &CodeGraph,
    node_filter: impl Fn(&Node) -> bool,
    include_edges: bool,
) -> Result<String> {
    let mut nodes_array = Vec::new();
    let mut filtered_ids = HashSet::new();

    // Export filtered nodes
    for node_id in 0..graph.node_count() as u64 {
        if let Ok(node) = graph.get_node(node_id) {
            if node_filter(node) {
                nodes_array.push(node_to_json(node_id, node));
                filtered_ids.insert(node_id);
            }
        }
    }

    // Export edges if requested
    let mut links_array = Vec::new();
    if include_edges {
        for edge_id in 0..graph.edge_count() as u64 {
            if let Ok(edge) = graph.get_edge(edge_id) {
                // Only include edges between filtered nodes
                if filtered_ids.contains(&edge.source_id) && filtered_ids.contains(&edge.target_id)
                {
                    links_array.push(json!({
                        "id": edge_id,
                        "source": edge.source_id,
                        "target": edge.target_id,
                        "type": format!("{:?}", edge.edge_type),
                        "properties": properties_to_json(&edge.properties),
                    }));
                }
            }
        }
    }

    let result = json!({
        "nodes": nodes_array,
        "links": links_array,
    });

    // serde_json::to_string_pretty should never fail for our data structures
    Ok(serde_json::to_string_pretty(&result).expect("Failed to serialize JSON"))
}

/// Convert node to JSON object
fn node_to_json(node_id: u64, node: &Node) -> Value {
    json!({
        "id": node_id,
        "type": format!("{:?}", node.node_type),
        "properties": properties_to_json(&node.properties),
    })
}

/// Convert PropertyMap to JSON object
fn properties_to_json(props: &crate::PropertyMap) -> Value {
    let mut obj = serde_json::Map::new();

    for (key, value) in props.iter() {
        let json_value = match value {
            crate::PropertyValue::String(s) => json!(s),
            crate::PropertyValue::Int(i) => json!(i),
            crate::PropertyValue::Float(f) => json!(f),
            crate::PropertyValue::Bool(b) => json!(b),
            crate::PropertyValue::StringList(v) => json!(v),
            crate::PropertyValue::IntList(v) => json!(v),
            crate::PropertyValue::Null => json!(null),
        };
        obj.insert(key.clone(), json_value);
    }

    Value::Object(obj)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PropertyMap;

    #[test]
    fn test_properties_to_json() {
        let mut props = PropertyMap::new();
        props.insert("name", "test");
        props.insert("count", 42);

        let json = properties_to_json(&props);
        assert!(json.is_object());
        assert_eq!(json["name"], "test");
        assert_eq!(json["count"], 42);
    }
}
