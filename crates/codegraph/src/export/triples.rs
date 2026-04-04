// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! RDF Triples format export for semantic web and SPARQL queries.
//!
//! Generates N-Triples format where each line is a triple: (subject, predicate, object).

use crate::{CodeGraph, Result};

/// Export graph as RDF triples in N-Triples format
pub fn export_triples(graph: &CodeGraph) -> Result<String> {
    let mut output = String::new();

    // Export node types
    for node_id in 0..graph.node_count() as u64 {
        if let Ok(node) = graph.get_node(node_id) {
            // Node type triple
            output.push_str(&format!(
                "<node:{}> <rdf:type> <type:{:?}> .\n",
                node_id, node.node_type
            ));

            // Property triples
            for (key, value) in node.properties.iter() {
                let object = format_triple_object(value);
                output.push_str(&format!("<node:{node_id}> <prop:{key}> {object} .\n"));
            }
        }
    }

    // Export edges as triples
    for edge_id in 0..graph.edge_count() as u64 {
        if let Ok(edge) = graph.get_edge(edge_id) {
            output.push_str(&format!(
                "<node:{}> <edge:{:?}> <node:{}> .\n",
                edge.source_id, edge.edge_type, edge.target_id
            ));

            // Edge properties as triples about the edge
            for (key, value) in edge.properties.iter() {
                let object = format_triple_object(value);
                output.push_str(&format!("<edge:{edge_id}> <prop:{key}> {object} .\n"));
            }
        }
    }

    Ok(output)
}

/// Format property value as RDF triple object (with type annotations)
fn format_triple_object(value: &crate::PropertyValue) -> String {
    match value {
        crate::PropertyValue::String(s) => {
            // Escape quotes and backslashes
            let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
            format!("\"{escaped}\"")
        }
        crate::PropertyValue::Int(i) => {
            format!("\"{i}\"^^<xsd:integer>")
        }
        crate::PropertyValue::Float(f) => {
            format!("\"{f}\"^^<xsd:double>")
        }
        crate::PropertyValue::Bool(b) => {
            format!("\"{b}\"^^<xsd:boolean>")
        }
        crate::PropertyValue::StringList(v) => {
            // Represent as JSON array (alternative: create multiple triples)
            let escaped = v.join(",").replace('\\', "\\\\").replace('"', "\\\"");
            format!("\"[{escaped}]\"")
        }
        crate::PropertyValue::IntList(v) => {
            let joined = v
                .iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(",");
            format!("\"[{joined}]\"^^<xsd:array>")
        }
        crate::PropertyValue::Null => "\"null\"".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_triple_object() {
        use crate::PropertyValue;

        assert_eq!(
            format_triple_object(&PropertyValue::String("hello".to_string())),
            "\"hello\""
        );
        assert_eq!(
            format_triple_object(&PropertyValue::Int(42)),
            "\"42\"^^<xsd:integer>"
        );
        assert_eq!(
            format_triple_object(&PropertyValue::Bool(true)),
            "\"true\"^^<xsd:boolean>"
        );
    }

    #[test]
    fn test_escape_quotes() {
        use crate::PropertyValue;

        let val = PropertyValue::String("say \"hi\"".to_string());
        let result = format_triple_object(&val);
        assert!(result.contains("\\\""));
    }
}
