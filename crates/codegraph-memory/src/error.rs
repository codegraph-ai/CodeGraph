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
