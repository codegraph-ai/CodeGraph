// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Storage backend abstractions and implementations.
//!
//! This module defines the [`StorageBackend`] trait and provides implementations:
//! - [`RocksDBBackend`]: Production-ready persistent storage
//! - [`MemoryBackend`]: In-memory storage for testing
//!
//! ## Design Philosophy
//!
//! - **Persistence Primary**: RocksDB is the default, memory backend only for tests
//! - **Explicit Operations**: No automatic flushing or background magic
//! - **Fail Fast**: Operations return errors immediately, no silent failures

mod memory;
mod namespaced;
#[cfg(feature = "rocksdb-backend")]
mod rocksdb_backend;

pub use memory::MemoryBackend;
pub use namespaced::NamespacedBackend;
#[cfg(feature = "rocksdb-backend")]
pub use rocksdb_backend::RocksDBBackend;

use crate::error::Result;
use serde::{Deserialize, Serialize};

/// Key-value pair for storage operations.
pub type KeyValue = (Vec<u8>, Vec<u8>);

/// Trait defining the storage backend interface.
///
/// All storage operations are explicit and return `Result` to handle failures.
/// Implementations must ensure crash-safety and atomic batch operations.
pub trait StorageBackend: Send + Sync {
    /// Store a key-value pair.
    ///
    /// This operation is durable immediately (no deferred writes).
    ///
    /// # Errors
    ///
    /// Returns an error if the write fails.
    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<()>;

    /// Retrieve a value by key.
    ///
    /// # Errors
    ///
    /// Returns an error if the read fails.
    /// Returns `Ok(None)` if the key doesn't exist.
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>>;

    /// Delete a key-value pair.
    ///
    /// # Errors
    ///
    /// Returns an error if the delete fails.
    /// Does not error if the key doesn't exist (idempotent).
    fn delete(&mut self, key: &[u8]) -> Result<()>;

    /// Check if a key exists.
    ///
    /// # Errors
    ///
    /// Returns an error if the check fails.
    fn exists(&self, key: &[u8]) -> Result<bool>;

    /// Iterate over all key-value pairs with keys starting with the given prefix.
    ///
    /// Returns an iterator over matching pairs.
    ///
    /// # Errors
    ///
    /// Returns an error if iteration setup fails.
    fn scan_prefix(&self, prefix: &[u8]) -> Result<Vec<KeyValue>>;

    /// Execute a batch of write operations atomically.
    ///
    /// Either all operations succeed or none do.
    ///
    /// # Errors
    ///
    /// Returns an error if any operation in the batch fails.
    fn write_batch(&mut self, operations: Vec<BatchOperation>) -> Result<()>;

    /// Flush any buffered writes to disk.
    ///
    /// This is explicit - no automatic flushing happens.
    ///
    /// # Errors
    ///
    /// Returns an error if flush fails.
    fn flush(&mut self) -> Result<()>;
}

/// Batch write operation for atomic updates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BatchOperation {
    /// Put a key-value pair
    Put {
        /// Key to write
        key: Vec<u8>,
        /// Value to write
        value: Vec<u8>,
    },
    /// Delete a key
    Delete {
        /// Key to delete
        key: Vec<u8>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test that storage backend trait is object-safe and can be used as trait object
    #[test]
    fn test_trait_object_safe() {
        fn _accept_trait_object(_backend: &dyn StorageBackend) {}
    }
}
