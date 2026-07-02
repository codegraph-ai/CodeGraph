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

    #[test]
    fn test_edge_not_found_error() {
        let err = GraphError::EdgeNotFound {
            edge_id: "edge-42".to_string(),
        };
        assert_eq!(err.to_string(), "Edge not found: edge-42");
    }

    #[test]
    fn test_file_not_found_error() {
        let err = GraphError::FileNotFound {
            path: PathBuf::from("/tmp/missing.rs"),
        };
        assert_eq!(err.to_string(), "File not found: /tmp/missing.rs");
    }

    #[test]
    fn test_property_type_mismatch_error() {
        let err = GraphError::PropertyTypeMismatch {
            key: "count".to_string(),
            expected: "integer".to_string(),
            actual: "string".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Property type mismatch: expected integer, got string for key 'count'"
        );
    }

    #[test]
    fn test_serialization_error_message() {
        let err = GraphError::serialization("bad json", None::<std::io::Error>);
        assert_eq!(err.to_string(), "Serialization error: bad json");
    }

    #[test]
    fn test_storage_error_with_source_chains() {
        use std::error::Error;
        let io_err = std::io::Error::other("disk full");
        let err = GraphError::storage("write failed", Some(io_err));
        assert_eq!(err.to_string(), "Storage error: write failed");
        // The Some(source) branch wraps the io error, exposed via Error::source().
        let src = err.source().expect("expected a source error");
        assert_eq!(src.to_string(), "disk full");
    }

    #[test]
    fn test_serialization_error_with_source_chains() {
        use std::error::Error;
        let io_err = std::io::Error::new(std::io::ErrorKind::InvalidData, "not utf8");
        let err = GraphError::serialization("decode failed", Some(io_err));
        let src = err.source().expect("expected a source error");
        assert_eq!(src.to_string(), "not utf8");
    }

    #[test]
    fn test_sourceless_variants_have_no_source() {
        use std::error::Error;
        // The six variants that carry no `#[source]` field must all return
        // source() == None. The existing source tests only exercise the
        // Some/None arms of Storage/Serialization, never pinning that these
        // variants stay sourceless - a stray #[source] added to any of them
        // (via a thiserror change or refactor) would otherwise go uncaught.
        let sourceless: Vec<GraphError> = vec![
            GraphError::NodeNotFound {
                node_id: "n1".to_string(),
            },
            GraphError::EdgeNotFound {
                edge_id: "e1".to_string(),
            },
            GraphError::FileNotFound {
                path: PathBuf::from("/tmp/x.rs"),
            },
            GraphError::InvalidOperation {
                message: "dup".to_string(),
            },
            GraphError::PropertyNotFound {
                entity_type: "node".to_string(),
                entity_id: "n1".to_string(),
                key: "name".to_string(),
            },
            GraphError::PropertyTypeMismatch {
                key: "count".to_string(),
                expected: "int".to_string(),
                actual: "str".to_string(),
            },
        ];
        for err in &sourceless {
            assert!(
                err.source().is_none(),
                "variant should have no source: {err}"
            );
        }
        // The None arm of the Storage/Serialization constructors must also
        // surface no source (distinct from the sourceless variants above,
        // since these variants *can* carry a source but were built without one).
        assert!(GraphError::storage("m", None::<std::io::Error>)
            .source()
            .is_none());
        assert!(GraphError::serialization("m", None::<std::io::Error>)
            .source()
            .is_none());
    }

    #[test]
    fn test_storage_source_downcasts_to_concrete_io_error() {
        use std::error::Error;
        // The existing source-chain tests assert only source().to_string();
        // they never confirm the boxed source downcasts back to a concrete
        // io::Error preserving its ErrorKind. ErrorKind preservation is what
        // lets callers branch on NotFound/PermissionDenied rather than string-match.
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let err = GraphError::storage("write failed", Some(io_err));
        let src = err.source().expect("expected a source error");
        let downcast = src
            .downcast_ref::<std::io::Error>()
            .expect("source should downcast to a concrete io::Error");
        assert_eq!(downcast.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(downcast.to_string(), "denied");
    }
}
