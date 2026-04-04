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
