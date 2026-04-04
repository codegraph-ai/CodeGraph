// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! CodeGraph Memory Layer
//!
//! Persistent memory layer for CodeGraph with bi-temporal tracking,
//! semantic search, and MCP tool definitions for AI agent access.
//!
//! ## Features
//!
//! - **Bi-temporal knowledge tracking** - Track when knowledge became true vs when it was recorded
//! - **Hybrid search** - BM25 + semantic (fastembed BGE-Small-EN-v1.5) + graph proximity
//! - **Auto-invalidation** - Memories linked to code are flagged when code changes
//! - **RocksDB persistence** - Efficient storage with HNSW index for O(log n) search
//!
//! ## Example
//!
//! ```ignore
//! use codegraph_memory::{MemoryNode, MemoryStore, VectorEngine};
//!
//! // Initialize engine and store
//! let engine = VectorEngine::new(Some(&extension_path))?;
//! let store = MemoryStore::new(&db_path, engine)?;
//!
//! // Create and store a memory
//! let memory = MemoryNode::builder()
//!     .debug_context("API returns 500 on large payloads", "Increase nginx body size")
//!     .title("Nginx body size limit fix")
//!     .content("The /upload endpoint fails...")
//!     .tag("nginx")
//!     .build()?;
//!
//! store.put(memory).await?;
//! ```

pub mod embedding;
pub mod error;
pub mod migration;
pub mod node;
pub mod search;
pub mod storage;
pub mod temporal;

// Re-exports for convenience
pub use embedding::{CodeGraphEmbeddingModel, VectorEngine};
pub use error::MemoryError;
pub use node::{
    CodeLink, IssueSeverity, LinkedNodeType, MemoryId, MemoryKind, MemoryNode, MemoryNodeBuilder,
    MemorySource,
};
pub use search::{MemorySearch, SearchConfig, SearchResult};
pub use storage::MemoryStore;
pub use temporal::TemporalMetadata;
