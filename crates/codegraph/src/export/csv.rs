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
    use crate::{helpers, PropertyMap, PropertyValue};

    #[test]
    fn test_escape_csv() {
        assert_eq!(escape_csv("hello"), "hello");
        assert_eq!(escape_csv("hello,world"), "\"hello,world\"");
        assert_eq!(escape_csv("say \"hi\""), "\"say \"\"hi\"\"\"");
    }

    #[test]
    fn test_escape_csv_newline() {
        // A newline alone (no comma/quote) still forces quoting.
        assert_eq!(escape_csv("line1\nline2"), "\"line1\nline2\"");
    }

    #[test]
    fn test_format_property_value_scalars() {
        assert_eq!(
            format_property_value(&PropertyValue::String("hi".to_string())),
            "hi"
        );
        assert_eq!(format_property_value(&PropertyValue::Int(42)), "42");
        assert_eq!(format_property_value(&PropertyValue::Float(1.5)), "1.5");
        assert_eq!(format_property_value(&PropertyValue::Bool(true)), "true");
        assert_eq!(format_property_value(&PropertyValue::Null), "");
    }

    #[test]
    fn test_format_property_value_lists_joined_with_semicolon() {
        assert_eq!(
            format_property_value(&PropertyValue::StringList(vec![
                "a".to_string(),
                "b".to_string(),
            ])),
            "a;b"
        );
        assert_eq!(
            format_property_value(&PropertyValue::IntList(vec![1, 2, 3])),
            "1;2;3"
        );
    }

    #[test]
    fn test_export_csv_nodes_header_and_rows() {
        let mut graph = CodeGraph::in_memory().unwrap();
        helpers::add_file(&mut graph, "a.py", "python").unwrap();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nodes.csv");
        export_csv_nodes(&graph, &path).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let mut lines = content.lines();
        // Keys are sorted alphabetically: language, path.
        assert_eq!(lines.next().unwrap(), "id,type,language,path");
        assert_eq!(lines.next().unwrap(), "0,CodeFile,python,a.py");
        assert!(lines.next().is_none());
    }

    #[test]
    fn test_export_csv_nodes_escapes_values_with_commas() {
        let mut graph = CodeGraph::in_memory().unwrap();
        let props = PropertyMap::new().with("doc", "hello, world");
        graph.add_node(crate::NodeType::Function, props).unwrap();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nodes.csv");
        export_csv_nodes(&graph, &path).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        // The comma-containing value must be quoted so it stays one CSV field.
        assert!(content.contains("\"hello, world\""));
    }

    #[test]
    fn test_export_csv_edges_header_and_rows() {
        let mut graph = CodeGraph::in_memory().unwrap();
        let a = helpers::add_file(&mut graph, "a.py", "python").unwrap();
        let b = helpers::add_file(&mut graph, "b.py", "python").unwrap();
        helpers::add_import(&mut graph, a, b, vec!["foo", "bar"]).unwrap();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("edges.csv");
        export_csv_edges(&graph, &path).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let mut lines = content.lines();
        assert_eq!(lines.next().unwrap(), "id,source,target,type,symbols");
        // StringList symbols are joined with ';' by format_property_value.
        assert_eq!(lines.next().unwrap(), "0,0,1,Imports,foo;bar");
        assert!(lines.next().is_none());
    }

    #[test]
    fn test_export_csv_nodes_sparse_columns_leave_empty_fields() {
        // Two nodes with disjoint property keys force a union header; each row
        // must leave an empty field wherever it lacks one of the union keys,
        // exercising the None arm of `node.properties.get(key)`.
        let mut graph = CodeGraph::in_memory().unwrap();
        // node 0: CodeFile carries `language` and `path`.
        helpers::add_file(&mut graph, "a.py", "python").unwrap();
        // node 1: Function carries only `name`.
        graph
            .add_node(
                crate::NodeType::Function,
                PropertyMap::new().with("name", "f"),
            )
            .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nodes.csv");
        export_csv_nodes(&graph, &path).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let mut lines = content.lines();
        // Union of keys, sorted: language, name, path.
        assert_eq!(lines.next().unwrap(), "id,type,language,name,path");
        // CodeFile row: name column is empty (missing key -> empty field).
        assert_eq!(lines.next().unwrap(), "0,CodeFile,python,,a.py");
        // Function row: language and path columns are empty.
        assert_eq!(lines.next().unwrap(), "1,Function,,f,");
        assert!(lines.next().is_none());
    }

    #[test]
    fn test_export_csv_edges_sparse_columns_leave_empty_fields() {
        // Two edges with disjoint property keys likewise exercise the empty-field
        // (None) arm of `edge.properties.get(key)` in the edge writer.
        let mut graph = CodeGraph::in_memory().unwrap();
        let a = helpers::add_file(&mut graph, "a.py", "python").unwrap();
        let b = helpers::add_file(&mut graph, "b.py", "python").unwrap();
        // edge 0: Imports carries only `symbols`.
        helpers::add_import(&mut graph, a, b, vec!["foo"]).unwrap();
        // edge 1: Calls carries only `line`.
        graph
            .add_edge(
                a,
                b,
                crate::EdgeType::Calls,
                PropertyMap::new().with("line", 5),
            )
            .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("edges.csv");
        export_csv_edges(&graph, &path).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let mut lines = content.lines();
        // Union of edge keys, sorted: line, symbols.
        assert_eq!(lines.next().unwrap(), "id,source,target,type,line,symbols");
        // Imports edge: line column is empty, symbols is present.
        assert_eq!(lines.next().unwrap(), "0,0,1,Imports,,foo");
        // Calls edge: line is present, symbols column is empty.
        assert_eq!(lines.next().unwrap(), "1,0,1,Calls,5,");
        assert!(lines.next().is_none());
    }

    #[test]
    fn test_export_csv_nodes_create_failure_is_storage_error() {
        // A path under a directory that does not exist makes File::create fail,
        // exercising the map_err arm that wraps the io::Error into
        // GraphError::Storage with the "Failed to create CSV file" message.
        let graph = CodeGraph::in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let bad_path = dir.path().join("missing_subdir").join("nodes.csv");

        let err = export_csv_nodes(&graph, &bad_path).unwrap_err();
        match err {
            crate::GraphError::Storage { message, source } => {
                assert!(message.starts_with("Failed to create CSV file:"));
                // The originating io::Error is preserved as the source.
                assert!(source.is_some());
            }
            other => panic!("expected Storage error, got {other:?}"),
        }
    }

    #[test]
    fn test_export_csv_edges_create_failure_is_storage_error() {
        // Same unwritable-path failure for the edge writer's File::create arm.
        let graph = CodeGraph::in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let bad_path = dir.path().join("missing_subdir").join("edges.csv");

        let err = export_csv_edges(&graph, &bad_path).unwrap_err();
        match err {
            crate::GraphError::Storage { message, source } => {
                assert!(message.starts_with("Failed to create CSV file:"));
                assert!(source.is_some());
            }
            other => panic!("expected Storage error, got {other:?}"),
        }
    }

    #[test]
    fn test_export_csv_writes_both_files() {
        let mut graph = CodeGraph::in_memory().unwrap();
        let a = helpers::add_file(&mut graph, "a.py", "python").unwrap();
        let b = helpers::add_file(&mut graph, "b.py", "python").unwrap();
        helpers::add_import(&mut graph, a, b, vec![]).unwrap();

        let dir = tempfile::tempdir().unwrap();
        let nodes_path = dir.path().join("nodes.csv");
        let edges_path = dir.path().join("edges.csv");
        export_csv(&graph, &nodes_path, &edges_path).unwrap();

        assert!(nodes_path.exists());
        assert!(edges_path.exists());
        // Two file nodes -> two data rows plus one header row.
        let nodes = std::fs::read_to_string(&nodes_path).unwrap();
        assert_eq!(nodes.lines().count(), 3);
        // One import edge -> one data row plus one header row.
        let edges = std::fs::read_to_string(&edges_path).unwrap();
        assert_eq!(edges.lines().count(), 2);
    }
}
