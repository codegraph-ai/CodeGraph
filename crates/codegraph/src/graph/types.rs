// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Core graph types: nodes, edges, IDs, and enums.

use super::property::{PropertyMap, PropertyValue};
use serde::{Deserialize, Serialize};

/// Unique identifier for a node (monotonic counter).
pub type NodeId = u64;

/// Unique identifier for an edge (monotonic counter).
pub type EdgeId = u64;

/// Type of a node in the code graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeType {
    /// Source code file
    CodeFile,
    /// Function, method, or procedure
    Function,
    /// Class, struct, or type definition
    Class,
    /// Module, namespace, or package
    Module,
    /// Variable, constant, or field
    Variable,
    /// Type alias or primitive type
    Type,
    /// Interface, trait, or protocol
    Interface,
    /// Catch-all for custom entity types
    Generic,
}

impl std::fmt::Display for NodeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeType::CodeFile => write!(f, "CodeFile"),
            NodeType::Function => write!(f, "Function"),
            NodeType::Class => write!(f, "Class"),
            NodeType::Module => write!(f, "Module"),
            NodeType::Variable => write!(f, "Variable"),
            NodeType::Type => write!(f, "Type"),
            NodeType::Interface => write!(f, "Interface"),
            NodeType::Generic => write!(f, "Generic"),
        }
    }
}

/// Type of edge (relationship) between nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EdgeType {
    /// File A imports File B
    Imports,
    /// File A imports symbols from File B
    ImportsFrom,
    /// Parent contains child entity (file contains function)
    Contains,
    /// Function A calls Function B
    Calls,
    /// Function invokes method on object
    Invokes,
    /// Function creates instance of class
    Instantiates,
    /// Class A extends/inherits from Class B
    Extends,
    /// Class implements interface/trait
    Implements,
    /// Generic usage relationship
    Uses,
    /// Module defines entity
    Defines,
    /// Generic reference
    References,
    /// Runtime dependency (e.g., HTTP client call → route handler)
    RuntimeCalls,
}

impl std::fmt::Display for EdgeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EdgeType::Imports => write!(f, "Imports"),
            EdgeType::ImportsFrom => write!(f, "ImportsFrom"),
            EdgeType::Contains => write!(f, "Contains"),
            EdgeType::Calls => write!(f, "Calls"),
            EdgeType::Invokes => write!(f, "Invokes"),
            EdgeType::Instantiates => write!(f, "Instantiates"),
            EdgeType::Extends => write!(f, "Extends"),
            EdgeType::Implements => write!(f, "Implements"),
            EdgeType::Uses => write!(f, "Uses"),
            EdgeType::Defines => write!(f, "Defines"),
            EdgeType::References => write!(f, "References"),
            EdgeType::RuntimeCalls => write!(f, "RuntimeCalls"),
        }
    }
}

/// Direction for neighbor queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Direction {
    /// Follow outgoing edges (from this node)
    Outgoing,
    /// Follow incoming edges (to this node)
    Incoming,
    /// Follow edges in both directions
    Both,
}

/// A node in the code graph.
///
/// Nodes represent code entities like files, functions, classes, etc.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    /// Unique identifier (assigned by graph)
    pub id: NodeId,
    /// Type of code entity
    pub node_type: NodeType,
    /// Flexible key-value metadata
    pub properties: PropertyMap,
}

impl Node {
    /// Create a new node (ID will be assigned by graph).
    pub fn new(id: NodeId, node_type: NodeType, properties: PropertyMap) -> Self {
        Self {
            id,
            node_type,
            properties,
        }
    }

    /// Add or update a property.
    pub fn set_property(&mut self, key: impl Into<String>, value: impl Into<PropertyValue>) {
        self.properties.insert(key, value);
    }

    /// Get a property value.
    pub fn get_property(&self, key: &str) -> Option<&PropertyValue> {
        self.properties.get(key)
    }
}

/// A directed edge in the code graph.
///
/// Edges represent relationships between nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    /// Unique identifier (assigned by graph)
    pub id: EdgeId,
    /// Source node ID
    pub source_id: NodeId,
    /// Target node ID
    pub target_id: NodeId,
    /// Type of relationship
    pub edge_type: EdgeType,
    /// Optional metadata (e.g., line number for calls)
    pub properties: PropertyMap,
}

impl Edge {
    /// Create a new edge (ID will be assigned by graph).
    pub fn new(
        id: EdgeId,
        source_id: NodeId,
        target_id: NodeId,
        edge_type: EdgeType,
        properties: PropertyMap,
    ) -> Self {
        Self {
            id,
            source_id,
            target_id,
            edge_type,
            properties,
        }
    }

    /// Add or update a property.
    pub fn set_property(&mut self, key: impl Into<String>, value: impl Into<PropertyValue>) {
        self.properties.insert(key, value);
    }

    /// Get a property value.
    pub fn get_property(&self, key: &str) -> Option<&PropertyValue> {
        self.properties.get(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_type_display_covers_all_variants() {
        assert_eq!(NodeType::CodeFile.to_string(), "CodeFile");
        assert_eq!(NodeType::Function.to_string(), "Function");
        assert_eq!(NodeType::Class.to_string(), "Class");
        assert_eq!(NodeType::Module.to_string(), "Module");
        assert_eq!(NodeType::Variable.to_string(), "Variable");
        assert_eq!(NodeType::Type.to_string(), "Type");
        assert_eq!(NodeType::Interface.to_string(), "Interface");
        assert_eq!(NodeType::Generic.to_string(), "Generic");
    }

    #[test]
    fn edge_type_display_covers_all_variants() {
        assert_eq!(EdgeType::Imports.to_string(), "Imports");
        assert_eq!(EdgeType::ImportsFrom.to_string(), "ImportsFrom");
        assert_eq!(EdgeType::Contains.to_string(), "Contains");
        assert_eq!(EdgeType::Calls.to_string(), "Calls");
        assert_eq!(EdgeType::Invokes.to_string(), "Invokes");
        assert_eq!(EdgeType::Instantiates.to_string(), "Instantiates");
        assert_eq!(EdgeType::Extends.to_string(), "Extends");
        assert_eq!(EdgeType::Implements.to_string(), "Implements");
        assert_eq!(EdgeType::Uses.to_string(), "Uses");
        assert_eq!(EdgeType::Defines.to_string(), "Defines");
        assert_eq!(EdgeType::References.to_string(), "References");
        assert_eq!(EdgeType::RuntimeCalls.to_string(), "RuntimeCalls");
    }

    #[test]
    fn node_type_display_matches_debug_for_each_variant() {
        // The Display strings are intended to mirror the Rust identifier, so
        // Display and Debug should agree for these fieldless variants.
        for nt in [
            NodeType::CodeFile,
            NodeType::Function,
            NodeType::Class,
            NodeType::Module,
            NodeType::Variable,
            NodeType::Type,
            NodeType::Interface,
            NodeType::Generic,
        ] {
            assert_eq!(nt.to_string(), format!("{nt:?}"));
        }
    }

    #[test]
    fn edge_type_display_matches_debug_for_each_variant() {
        for et in [
            EdgeType::Imports,
            EdgeType::ImportsFrom,
            EdgeType::Contains,
            EdgeType::Calls,
            EdgeType::Invokes,
            EdgeType::Instantiates,
            EdgeType::Extends,
            EdgeType::Implements,
            EdgeType::Uses,
            EdgeType::Defines,
            EdgeType::References,
            EdgeType::RuntimeCalls,
        ] {
            assert_eq!(et.to_string(), format!("{et:?}"));
        }
    }

    #[test]
    fn direction_variants_are_distinct_and_copy() {
        // Direction is Copy + PartialEq; a copy compares equal to its source
        // and the three variants are pairwise distinct.
        let d = Direction::Outgoing;
        let copied = d;
        assert_eq!(d, copied);
        assert_ne!(Direction::Outgoing, Direction::Incoming);
        assert_ne!(Direction::Incoming, Direction::Both);
        assert_ne!(Direction::Outgoing, Direction::Both);
    }

    #[test]
    fn node_new_stores_id_type_and_properties() {
        let mut props = PropertyMap::new();
        props.insert("name", "main");
        let node = Node::new(7, NodeType::Function, props);
        assert_eq!(node.id, 7);
        assert_eq!(node.node_type, NodeType::Function);
        assert_eq!(
            node.get_property("name"),
            Some(&PropertyValue::String("main".to_string()))
        );
    }

    #[test]
    fn node_get_property_missing_returns_none() {
        let node = Node::new(1, NodeType::Module, PropertyMap::new());
        assert!(node.get_property("absent").is_none());
    }

    #[test]
    fn node_set_property_inserts_and_overwrites() {
        let mut node = Node::new(2, NodeType::Class, PropertyMap::new());
        node.set_property("line_start", 10i64);
        assert_eq!(
            node.get_property("line_start"),
            Some(&PropertyValue::Int(10))
        );
        // Re-setting the same key overwrites in place.
        node.set_property("line_start", 42i64);
        assert_eq!(
            node.get_property("line_start"),
            Some(&PropertyValue::Int(42))
        );
    }

    #[test]
    fn edge_new_stores_all_fields() {
        let mut props = PropertyMap::new();
        props.insert("line", 3i64);
        let edge = Edge::new(5, 1, 2, EdgeType::Calls, props);
        assert_eq!(edge.id, 5);
        assert_eq!(edge.source_id, 1);
        assert_eq!(edge.target_id, 2);
        assert_eq!(edge.edge_type, EdgeType::Calls);
        assert_eq!(edge.get_property("line"), Some(&PropertyValue::Int(3)));
    }

    #[test]
    fn edge_set_property_inserts_and_overwrites() {
        let mut edge = Edge::new(1, 0, 0, EdgeType::References, PropertyMap::new());
        assert!(edge.get_property("weight").is_none());
        edge.set_property("weight", 1.5f64);
        assert_eq!(
            edge.get_property("weight"),
            Some(&PropertyValue::Float(1.5))
        );
        edge.set_property("weight", 2.5f64);
        assert_eq!(
            edge.get_property("weight"),
            Some(&PropertyValue::Float(2.5))
        );
    }

    #[test]
    fn node_serde_round_trip_preserves_fields() {
        let mut props = PropertyMap::new();
        props.insert("name", "widget");
        props.insert("is_test", true);
        let node = Node::new(99, NodeType::Interface, props);
        let json = serde_json::to_string(&node).expect("serialize node");
        let back: Node = serde_json::from_str(&json).expect("deserialize node");
        assert_eq!(back.id, node.id);
        assert_eq!(back.node_type, node.node_type);
        assert_eq!(
            back.get_property("name"),
            Some(&PropertyValue::String("widget".to_string()))
        );
        assert_eq!(
            back.get_property("is_test"),
            Some(&PropertyValue::Bool(true))
        );
    }

    #[test]
    fn edge_serde_round_trip_preserves_fields() {
        let edge = Edge::new(4, 10, 20, EdgeType::Implements, PropertyMap::new());
        let json = serde_json::to_string(&edge).expect("serialize edge");
        let back: Edge = serde_json::from_str(&json).expect("deserialize edge");
        assert_eq!(back.id, edge.id);
        assert_eq!(back.source_id, edge.source_id);
        assert_eq!(back.target_id, edge.target_id);
        assert_eq!(back.edge_type, edge.edge_type);
    }

    #[test]
    fn node_type_serde_round_trips_each_variant() {
        for nt in [
            NodeType::CodeFile,
            NodeType::Function,
            NodeType::Class,
            NodeType::Module,
            NodeType::Variable,
            NodeType::Type,
            NodeType::Interface,
            NodeType::Generic,
        ] {
            let json = serde_json::to_string(&nt).expect("serialize node type");
            let back: NodeType = serde_json::from_str(&json).expect("deserialize node type");
            assert_eq!(back, nt);
        }
    }

    #[test]
    fn edge_type_serde_round_trips_each_variant() {
        for et in [
            EdgeType::Imports,
            EdgeType::ImportsFrom,
            EdgeType::Contains,
            EdgeType::Calls,
            EdgeType::Invokes,
            EdgeType::Instantiates,
            EdgeType::Extends,
            EdgeType::Implements,
            EdgeType::Uses,
            EdgeType::Defines,
            EdgeType::References,
            EdgeType::RuntimeCalls,
        ] {
            let json = serde_json::to_string(&et).expect("serialize edge type");
            let back: EdgeType = serde_json::from_str(&json).expect("deserialize edge type");
            assert_eq!(back, et);
        }
    }

    #[test]
    fn node_type_wire_format_is_bare_variant_string() {
        // Every NodeType is embedded in each persisted Node, so its exact
        // externally-tagged wire form (a bare JSON string equal to the variant
        // identifier) is an on-disk contract. The round-trip test above would
        // still pass under a #[serde(rename)] that silently invalidates existing
        // databases; only pinning the literal bytes catches that.
        for (nt, wire) in [
            (NodeType::CodeFile, "\"CodeFile\""),
            (NodeType::Function, "\"Function\""),
            (NodeType::Class, "\"Class\""),
            (NodeType::Module, "\"Module\""),
            (NodeType::Variable, "\"Variable\""),
            (NodeType::Type, "\"Type\""),
            (NodeType::Interface, "\"Interface\""),
            (NodeType::Generic, "\"Generic\""),
        ] {
            assert_eq!(serde_json::to_string(&nt).unwrap(), wire);
        }
    }

    #[test]
    fn edge_type_wire_format_is_bare_variant_string() {
        // Same on-disk contract for EdgeType, embedded in every persisted Edge.
        for (et, wire) in [
            (EdgeType::Imports, "\"Imports\""),
            (EdgeType::ImportsFrom, "\"ImportsFrom\""),
            (EdgeType::Contains, "\"Contains\""),
            (EdgeType::Calls, "\"Calls\""),
            (EdgeType::Invokes, "\"Invokes\""),
            (EdgeType::Instantiates, "\"Instantiates\""),
            (EdgeType::Extends, "\"Extends\""),
            (EdgeType::Implements, "\"Implements\""),
            (EdgeType::Uses, "\"Uses\""),
            (EdgeType::Defines, "\"Defines\""),
            (EdgeType::References, "\"References\""),
            (EdgeType::RuntimeCalls, "\"RuntimeCalls\""),
        ] {
            assert_eq!(serde_json::to_string(&et).unwrap(), wire);
        }
    }
}
