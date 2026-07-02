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

    #[test]
    fn test_format_triple_object_float() {
        use crate::PropertyValue;

        assert_eq!(
            format_triple_object(&PropertyValue::Float(3.5)),
            "\"3.5\"^^<xsd:double>"
        );
    }

    #[test]
    fn test_format_triple_object_null() {
        use crate::PropertyValue;

        assert_eq!(format_triple_object(&PropertyValue::Null), "\"null\"");
    }

    #[test]
    fn test_format_triple_object_string_list() {
        use crate::PropertyValue;

        assert_eq!(
            format_triple_object(&PropertyValue::StringList(vec![
                "a".to_string(),
                "b".to_string(),
            ])),
            "\"[a,b]\""
        );
        // An empty list still yields a well-formed bracketed literal.
        assert_eq!(
            format_triple_object(&PropertyValue::StringList(vec![])),
            "\"[]\""
        );
    }

    #[test]
    fn test_format_triple_object_string_list_escapes() {
        use crate::PropertyValue;

        // Backslashes and quotes in list elements are escaped after joining.
        let val = PropertyValue::StringList(vec!["x\"".to_string(), "y\\".to_string()]);
        let result = format_triple_object(&val);
        assert!(result.contains("\\\""), "quote should be escaped: {result}");
        assert!(
            result.contains("\\\\"),
            "backslash should be escaped: {result}"
        );
    }

    #[test]
    fn test_format_triple_object_int_list() {
        use crate::PropertyValue;

        assert_eq!(
            format_triple_object(&PropertyValue::IntList(vec![1, 2, 3])),
            "\"[1,2,3]\"^^<xsd:array>"
        );
        assert_eq!(
            format_triple_object(&PropertyValue::IntList(vec![])),
            "\"[]\"^^<xsd:array>"
        );
    }

    #[test]
    fn test_format_triple_object_string_escapes_backslash() {
        use crate::PropertyValue;

        // The backslash branch of the String escape (only quotes were pinned before).
        let val = PropertyValue::String("path\\to".to_string());
        assert_eq!(format_triple_object(&val), "\"path\\\\to\"");
    }

    #[test]
    fn test_export_triples_emits_node_and_edge_properties() {
        use crate::{EdgeType, NodeType, PropertyMap, PropertyValue};

        let mut graph = CodeGraph::in_memory().unwrap();
        let mut node_props = PropertyMap::new();
        node_props.insert("name".to_string(), PropertyValue::String("f".to_string()));
        node_props.insert("arity".to_string(), PropertyValue::Int(2));
        let a = graph.add_node(NodeType::Function, node_props).unwrap();
        let b = graph
            .add_node(NodeType::Function, PropertyMap::new())
            .unwrap();

        let mut edge_props = PropertyMap::new();
        edge_props.insert("weight".to_string(), PropertyValue::Float(1.5));
        graph.add_edge(a, b, EdgeType::Calls, edge_props).unwrap();

        let triples = graph.export_triples().unwrap();
        // Node property triple with the xsd:integer annotation.
        assert!(triples.contains("<node:0> <prop:arity> \"2\"^^<xsd:integer> ."));
        // Node type triple.
        assert!(triples.contains("<node:0> <rdf:type>"));
        // Edge triple plus an edge-property triple keyed by edge id.
        assert!(triples.contains("<node:0> <edge:Calls> <node:1> ."));
        assert!(triples.contains("<edge:0> <prop:weight> \"1.5\"^^<xsd:double> ."));
    }

    #[test]
    fn test_export_triples_pins_exact_node_type_triple() {
        use crate::{NodeType, PropertyMap};

        let mut graph = CodeGraph::in_memory().unwrap();
        graph
            .add_node(NodeType::Function, PropertyMap::new())
            .unwrap();

        let triples = graph.export_triples().unwrap();
        // Pin the full node-type triple including the <type:{Debug}> object and
        // trailing " ." terminator, which prior coverage only checked via the
        // <node:0> <rdf:type> prefix.
        assert!(
            triples.contains("<node:0> <rdf:type> <type:Function> .\n"),
            "exact node-type triple not found: {triples}"
        );
    }

    #[test]
    fn test_export_triples_propertyless_node_emits_only_type_triple() {
        use crate::{NodeType, PropertyMap};

        let mut graph = CodeGraph::in_memory().unwrap();
        // A node with an empty PropertyMap exercises the zero-iteration arm of
        // the per-node property loop: it must emit its rdf:type triple and no
        // <node:0> <prop:...> triples at all.
        graph
            .add_node(NodeType::Function, PropertyMap::new())
            .unwrap();

        let triples = graph.export_triples().unwrap();
        assert!(triples.contains("<node:0> <rdf:type> <type:Function> .\n"));
        assert!(
            !triples.contains("<node:0> <prop:"),
            "propertyless node must emit no property triples: {triples}"
        );
        // The type triple is the only line produced for this graph.
        assert_eq!(triples.lines().count(), 1);
    }
}
