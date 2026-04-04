// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! CSV format export for data analysis in spreadsheets and pandas.
//!
//! Generates separate CSV files for nodes and edges with auto-detected columns.

use crate::{CodeGraph, Result};
use std::collections::HashSet;
use std::fs::File;
use std::io::Write;
use std::path::Path;

/// Export nodes to CSV file
pub fn export_csv_nodes(graph: &CodeGraph, path: &Path) -> Result<()> {
    let mut file = File::create(path).map_err(|e| crate::GraphError::Storage {
        message: format!("Failed to create CSV file: {}", path.display()),
        source: Some(Box::new(e)),
    })?;

    // Collect all property keys used in the graph
    let mut all_keys = HashSet::new();
    for node_id in 0..graph.node_count() as u64 {
        if let Ok(node) = graph.get_node(node_id) {
            for (key, _) in node.properties.iter() {
                all_keys.insert(key.clone());
            }
        }
    }

    let mut keys_vec: Vec<String> = all_keys.into_iter().collect();
    keys_vec.sort();

    // Write header
    write!(file, "id,type").map_err(|e| crate::GraphError::Storage {
        message: "Failed to write CSV header".to_string(),
        source: Some(Box::new(e)),
    })?;
    for key in &keys_vec {
        write!(file, ",{key}").map_err(|e| crate::GraphError::Storage {
            message: "Failed to write CSV header".to_string(),
            source: Some(Box::new(e)),
        })?;
    }
    writeln!(file).map_err(|e| crate::GraphError::Storage {
        message: "Failed to write CSV header".to_string(),
        source: Some(Box::new(e)),
    })?;

    // Write rows
    for node_id in 0..graph.node_count() as u64 {
        if let Ok(node) = graph.get_node(node_id) {
            write!(file, "{},{:?}", node_id, node.node_type).map_err(|e| {
                crate::GraphError::Storage {
                    message: "Failed to write CSV row".to_string(),
                    source: Some(Box::new(e)),
                }
            })?;

            for key in &keys_vec {
                write!(file, ",").map_err(|e| crate::GraphError::Storage {
                    message: "Failed to write CSV row".to_string(),
                    source: Some(Box::new(e)),
                })?;
                if let Some(value) = node.properties.get(key) {
                    write!(file, "{}", escape_csv(&format_property_value(value))).map_err(|e| {
                        crate::GraphError::Storage {
                            message: "Failed to write CSV row".to_string(),
                            source: Some(Box::new(e)),
                        }
                    })?;
                }
            }
            writeln!(file).map_err(|e| crate::GraphError::Storage {
                message: "Failed to write CSV row".to_string(),
                source: Some(Box::new(e)),
            })?;
        }
    }

    Ok(())
}

/// Export edges to CSV file
pub fn export_csv_edges(graph: &CodeGraph, path: &Path) -> Result<()> {
    let mut file = File::create(path).map_err(|e| crate::GraphError::Storage {
        message: format!("Failed to create CSV file: {}", path.display()),
        source: Some(Box::new(e)),
    })?;

    // Collect all property keys used in edges
    let mut all_keys = HashSet::new();
    for edge_id in 0..graph.edge_count() as u64 {
        if let Ok(edge) = graph.get_edge(edge_id) {
            for (key, _) in edge.properties.iter() {
                all_keys.insert(key.clone());
            }
        }
    }

    let mut keys_vec: Vec<String> = all_keys.into_iter().collect();
    keys_vec.sort();

    // Write header
    write!(file, "id,source,target,type").map_err(|e| crate::GraphError::Storage {
        message: "Failed to write CSV header".to_string(),
        source: Some(Box::new(e)),
    })?;
    for key in &keys_vec {
        write!(file, ",{key}").map_err(|e| crate::GraphError::Storage {
            message: "Failed to write CSV header".to_string(),
            source: Some(Box::new(e)),
        })?;
    }
    writeln!(file).map_err(|e| crate::GraphError::Storage {
        message: "Failed to write CSV header".to_string(),
        source: Some(Box::new(e)),
    })?;

    // Write rows
    for edge_id in 0..graph.edge_count() as u64 {
        if let Ok(edge) = graph.get_edge(edge_id) {
            write!(
                file,
                "{},{},{},{:?}",
                edge_id, edge.source_id, edge.target_id, edge.edge_type
            )
            .map_err(|e| crate::GraphError::Storage {
                message: "Failed to write CSV row".to_string(),
                source: Some(Box::new(e)),
            })?;

            for key in &keys_vec {
                write!(file, ",").map_err(|e| crate::GraphError::Storage {
                    message: "Failed to write CSV row".to_string(),
                    source: Some(Box::new(e)),
                })?;
                if let Some(value) = edge.properties.get(key) {
                    write!(file, "{}", escape_csv(&format_property_value(value))).map_err(|e| {
                        crate::GraphError::Storage {
                            message: "Failed to write CSV row".to_string(),
                            source: Some(Box::new(e)),
                        }
                    })?;
                }
            }
            writeln!(file).map_err(|e| crate::GraphError::Storage {
                message: "Failed to write CSV row".to_string(),
                source: Some(Box::new(e)),
            })?;
        }
    }

    Ok(())
}

/// Export both nodes and edges to separate CSV files (convenience method)
pub fn export_csv(graph: &CodeGraph, nodes_path: &Path, edges_path: &Path) -> Result<()> {
    export_csv_nodes(graph, nodes_path)?;
    export_csv_edges(graph, edges_path)?;
    Ok(())
}

/// Format property value for CSV
fn format_property_value(value: &crate::PropertyValue) -> String {
    match value {
        crate::PropertyValue::String(s) => s.clone(),
        crate::PropertyValue::Int(i) => i.to_string(),
        crate::PropertyValue::Float(f) => f.to_string(),
        crate::PropertyValue::Bool(b) => b.to_string(),
        crate::PropertyValue::StringList(v) => v.join(";"),
        crate::PropertyValue::IntList(v) => v
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(";"),
        crate::PropertyValue::Null => String::new(),
    }
}

/// Escape CSV value (add quotes if contains comma, quote, or newline)
fn escape_csv(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_csv() {
        assert_eq!(escape_csv("hello"), "hello");
        assert_eq!(escape_csv("hello,world"), "\"hello,world\"");
        assert_eq!(escape_csv("say \"hi\""), "\"say \"\"hi\"\"\"");
    }
}
