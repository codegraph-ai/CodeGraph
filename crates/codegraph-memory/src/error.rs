// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Error types for codegraph-memory

use thiserror::Error;

/// Errors that can occur in the memory system
#[derive(Debug, Error)]
pub enum MemoryError {
    /// RocksDB error
    #[error("Storage error: {0}")]
    Storage(#[from] rocksdb::Error),

    /// Serialization error (bincode)
    #[error("Serialization error: {0}")]
    Bincode(#[from] bincode::Error),

    /// MessagePack serialization error
    #[error("MessagePack error: {0}")]
    MessagePack(#[from] rmp_serde::encode::Error),

    /// MessagePack deserialization error
    #[error("MessagePack decode error: {0}")]
    MessagePackDecode(#[from] rmp_serde::decode::Error),

    /// JSON serialization error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// UUID parsing error
    #[error("UUID error: {0}")]
    Uuid(#[from] uuid::Error),

    /// Model loading error
    #[error("Model error: {0}")]
    Model(String),

    /// Embedding generation error
    #[error("Embedding error: {0}")]
    Embedding(String),

    /// Memory not found
    #[error("Memory not found: {0}")]
    NotFound(String),

    /// Invalid path
    #[error("Invalid path: {0}")]
    InvalidPath(String),

    /// Search error
    #[error("Search error: {0}")]
    Search(String),

    /// Builder error
    #[error("Builder error: {0}")]
    Builder(#[from] crate::node::MemoryNodeBuilderError),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Generic error
    #[error("{0}")]
    Other(String),
}

impl MemoryError {
    /// Create a model error
    pub fn model(msg: impl Into<String>) -> Self {
        Self::Model(msg.into())
    }

    /// Create an embedding error
    pub fn embedding(msg: impl Into<String>) -> Self {
        Self::Embedding(msg.into())
    }

    /// Create a not found error
    pub fn not_found(id: impl Into<String>) -> Self {
        Self::NotFound(id.into())
    }

    /// Create an invalid path error
    pub fn invalid_path(path: impl Into<String>) -> Self {
        Self::InvalidPath(path.into())
    }

    /// Create a search error
    pub fn search(msg: impl Into<String>) -> Self {
        Self::Search(msg.into())
    }

    /// Create a generic error
    pub fn other(msg: impl Into<String>) -> Self {
        Self::Other(msg.into())
    }
}

/// Result type for memory operations
pub type Result<T> = std::result::Result<T, MemoryError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_constructor_wraps_message() {
        let err = MemoryError::model("load failed");
        assert!(matches!(err, MemoryError::Model(ref m) if m == "load failed"));
        assert_eq!(err.to_string(), "Model error: load failed");
    }

    #[test]
    fn embedding_constructor_wraps_message() {
        let err = MemoryError::embedding("no vector");
        assert!(matches!(err, MemoryError::Embedding(ref m) if m == "no vector"));
        assert_eq!(err.to_string(), "Embedding error: no vector");
    }

    #[test]
    fn not_found_constructor_interpolates_id() {
        let err = MemoryError::not_found("abc-123");
        assert!(matches!(err, MemoryError::NotFound(ref id) if id == "abc-123"));
        assert_eq!(err.to_string(), "Memory not found: abc-123");
    }

    #[test]
    fn invalid_path_constructor_and_display() {
        let err = MemoryError::invalid_path("/bad/path");
        assert!(matches!(err, MemoryError::InvalidPath(ref p) if p == "/bad/path"));
        assert_eq!(err.to_string(), "Invalid path: /bad/path");
    }

    #[test]
    fn search_constructor_and_display() {
        let err = MemoryError::search("index missing");
        assert!(matches!(err, MemoryError::Search(ref m) if m == "index missing"));
        assert_eq!(err.to_string(), "Search error: index missing");
    }

    #[test]
    fn other_constructor_display_has_no_prefix() {
        let err = MemoryError::other("raw message");
        assert!(matches!(err, MemoryError::Other(ref m) if m == "raw message"));
        assert_eq!(err.to_string(), "raw message");
    }

    #[test]
    fn constructors_accept_str_and_string() {
        // Into<String> covers both &str and owned String inputs.
        let from_str = MemoryError::model("x");
        let from_string = MemoryError::model(String::from("x"));
        assert_eq!(from_str.to_string(), from_string.to_string());
    }

    #[test]
    fn from_io_error_yields_io_variant() {
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "missing db");
        let err: MemoryError = io.into();
        assert!(matches!(err, MemoryError::Io(_)));
        assert!(err.to_string().starts_with("IO error: "));
    }

    #[test]
    fn from_json_error_yields_json_variant() {
        let json_err = serde_json::from_str::<i32>("not json").unwrap_err();
        let err: MemoryError = json_err.into();
        assert!(matches!(err, MemoryError::Json(_)));
        assert!(err.to_string().starts_with("JSON error: "));
    }

    #[test]
    fn from_uuid_error_yields_uuid_variant() {
        let uuid_err = uuid::Uuid::parse_str("not-a-uuid").unwrap_err();
        let err: MemoryError = uuid_err.into();
        assert!(matches!(err, MemoryError::Uuid(_)));
        assert!(err.to_string().starts_with("UUID error: "));
    }

    #[test]
    fn source_exposes_wrapped_error_for_from_variants() {
        use std::error::Error as _;

        // #[from] variants auto-wire std::error::Error::source() to the inner error.
        let io_err: MemoryError =
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied").into();
        let src = io_err.source().expect("Io variant chains a source");
        assert!(src.downcast_ref::<std::io::Error>().is_some());

        let json_err: MemoryError = serde_json::from_str::<i32>("nope").unwrap_err().into();
        let json_src = json_err.source().expect("Json variant chains a source");
        assert!(json_src.downcast_ref::<serde_json::Error>().is_some());

        let uuid_err: MemoryError = uuid::Uuid::parse_str("not-a-uuid").unwrap_err().into();
        let uuid_src = uuid_err.source().expect("Uuid variant chains a source");
        assert!(uuid_src.downcast_ref::<uuid::Error>().is_some());
    }

    #[test]
    fn source_is_none_for_string_variants() {
        use std::error::Error as _;

        // String-backed variants carry no inner error, so the chain terminates.
        assert!(MemoryError::model("x").source().is_none());
        assert!(MemoryError::not_found("y").source().is_none());
        assert!(MemoryError::other("z").source().is_none());
    }

    #[test]
    fn from_bincode_error_yields_bincode_variant() {
        // Two bytes cannot decode into a u64, producing a bincode::Error that the
        // #[from] impl lifts into the Bincode variant with the "Serialization error:" prefix.
        let bincode_err = bincode::deserialize::<u64>(&[0u8, 1]).unwrap_err();
        let err: MemoryError = bincode_err.into();
        assert!(matches!(err, MemoryError::Bincode(_)));
        assert!(err.to_string().starts_with("Serialization error: "));
    }

    #[test]
    fn from_msgpack_decode_error_yields_variant() {
        // 0xc1 is a reserved MessagePack marker that never decodes, so rmp_serde
        // returns a decode error the #[from] impl maps to MessagePackDecode.
        let mp_err = rmp_serde::from_slice::<u64>(&[0xc1u8]).unwrap_err();
        let err: MemoryError = mp_err.into();
        assert!(matches!(err, MemoryError::MessagePackDecode(_)));
        assert!(err.to_string().starts_with("MessagePack decode error: "));
    }

    #[test]
    fn result_alias_carries_memory_error() {
        let ok: Result<u32> = Ok(7);
        assert!(matches!(ok, Ok(7)));
        let err: Result<u32> = Err(MemoryError::not_found("z"));
        assert!(err.is_err());
    }
}
