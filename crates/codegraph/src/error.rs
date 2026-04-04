// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Error types for codegraph operations.
//!
//! All fallible operations return [`Result<T>`] with context-rich error messages.

use std::path::PathBuf;
use thiserror::Error;

/// Result type alias for codegraph operations.
pub type Result<T> = std::result::Result<T, GraphError>;

/// Comprehensive error type for all graph operations.
///
/// Errors are designed to fail fast and provide clear context about what went wrong.
#[derive(Error, Debug)]
pub enum GraphError {
    /// Storage backend error (RocksDB, file I/O, etc.)
    #[error("Storage error: {message}")]
    Storage {
        /// Detailed error message
        message: String,
        /// Optional source error
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Node not found in the graph
    #[error("Node not found: {node_id}")]
    NodeNotFound {
        /// ID of the missing node
        node_id: String,
    },

    /// Edge not found in the graph
    #[error("Edge not found: {edge_id}")]
    EdgeNotFound {
        /// ID of the missing edge
        edge_id: String,
    },

    /// File not found in the graph
    #[error("File not found: {path}")]
    FileNotFound {
        /// Path to the missing file
        path: PathBuf,
    },

    /// Invalid operation (e.g., adding duplicate node)
    #[error("Invalid operation: {message}")]
    InvalidOperation {
        /// Description of what went wrong
        message: String,
    },

    /// Serialization/deserialization error
    #[error("Serialization error: {message}")]
    Serialization {
        /// Error details
        message: String,
        /// Optional source error
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Property not found
    #[error("Property '{key}' not found on {entity_type} {entity_id}")]
    PropertyNotFound {
        /// Entity type (node, edge, etc.)
        entity_type: String,
        /// Entity identifier
        entity_id: String,
        /// Property key that was missing
        key: String,
    },

    /// Type mismatch when retrieving property
    #[error("Property type mismatch: expected {expected}, got {actual} for key '{key}'")]
    PropertyTypeMismatch {
        /// Property key
        key: String,
        /// Expected type
        expected: String,
        /// Actual type found
        actual: String,
    },
}

impl GraphError {
    /// Create a storage error from a message and optional source.
    pub fn storage<E>(message: impl Into<String>, source: Option<E>) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Self::Storage {
            message: message.into(),
            source: source.map(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>),
        }
    }

    /// Create a serialization error from a message and optional source.
    pub fn serialization<E>(message: impl Into<String>, source: Option<E>) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Self::Serialization {
            message: message.into(),
            source: source.map(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_not_found_error() {
        let err = GraphError::NodeNotFound {
            node_id: "test-node-123".to_string(),
        };
        assert_eq!(err.to_string(), "Node not found: test-node-123");
    }

    #[test]
    fn test_storage_error() {
        let err = GraphError::storage("Failed to write to disk", None::<std::io::Error>);
        assert_eq!(err.to_string(), "Storage error: Failed to write to disk");
    }

    #[test]
    fn test_invalid_operation_error() {
        let err = GraphError::InvalidOperation {
            message: "Cannot add duplicate node".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Invalid operation: Cannot add duplicate node"
        );
    }

    #[test]
    fn test_property_not_found_error() {
        let err = GraphError::PropertyNotFound {
            entity_type: "node".to_string(),
            entity_id: "node-123".to_string(),
            key: "name".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Property 'name' not found on node node-123"
        );
    }
}
