// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! RocksDB storage backend for production use.
//!
//! This backend provides crash-safe, persistent storage with write-ahead logging.
//! All writes are durable immediately (no deferred writes).

use super::{BatchOperation, KeyValue, StorageBackend};
use crate::error::{GraphError, Result};
use rocksdb::{Options, WriteBatch, DB};
use std::path::Path;
use std::sync::Arc;

/// RocksDB-backed persistent storage.
///
/// This is the production storage backend. It provides:
/// - Crash-safe writes with WAL
/// - Atomic batch operations
/// - Efficient prefix scans
/// - Durability guarantees
#[derive(Clone)]
pub struct RocksDBBackend {
    db: Arc<DB>,
}

impl RocksDBBackend {
    /// Open or create a RocksDB database at the given path.
    ///
    /// # Arguments
    ///
    /// * `path` - Directory path for the database files
    ///
    /// # Errors
    ///
    /// Returns [`GraphError::Storage`] if the database cannot be opened.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);

        let db = DB::open(&opts, path.as_ref()).map_err(|e| {
            GraphError::storage(
                format!("Failed to open RocksDB at {:?}", path.as_ref()),
                Some(e),
            )
        })?;

        Ok(Self { db: Arc::new(db) })
    }

    /// Open a RocksDB database with custom options.
    ///
    /// For advanced use cases where specific RocksDB tuning is needed.
    ///
    /// # Errors
    ///
    /// Returns [`GraphError::Storage`] if the database cannot be opened.
    pub fn open_with_options<P: AsRef<Path>>(path: P, opts: Options) -> Result<Self> {
        let db = DB::open(&opts, path.as_ref()).map_err(|e| {
            GraphError::storage(
                format!("Failed to open RocksDB at {:?}", path.as_ref()),
                Some(e),
            )
        })?;

        Ok(Self { db: Arc::new(db) })
    }

    /// Get the underlying RocksDB database handle.
    ///
    /// Useful for advanced operations not exposed by the storage trait.
    pub fn db(&self) -> &Arc<DB> {
        &self.db
    }
}

impl StorageBackend for RocksDBBackend {
    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        self.db
            .put(key, value)
            .map_err(|e| GraphError::storage("Failed to put key-value pair", Some(e)))
    }

    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        self.db
            .get(key)
            .map_err(|e| GraphError::storage("Failed to get value", Some(e)))
    }

    fn delete(&mut self, key: &[u8]) -> Result<()> {
        self.db
            .delete(key)
            .map_err(|e| GraphError::storage("Failed to delete key", Some(e)))
    }

    fn exists(&self, key: &[u8]) -> Result<bool> {
        self.db
            .get(key)
            .map(|opt| opt.is_some())
            .map_err(|e| GraphError::storage("Failed to check key existence", Some(e)))
    }

    fn scan_prefix(&self, prefix: &[u8]) -> Result<Vec<KeyValue>> {
        let mut results = Vec::new();
        let iter = self.db.prefix_iterator(prefix);

        for item in iter {
            let (key, value) =
                item.map_err(|e| GraphError::storage("Failed to iterate over prefix", Some(e)))?;

            // RocksDB prefix iterator may return keys beyond the prefix
            // so we need to check explicitly
            if !key.starts_with(prefix) {
                break;
            }

            results.push((key.to_vec(), value.to_vec()));
        }

        Ok(results)
    }

    fn write_batch(&mut self, operations: Vec<BatchOperation>) -> Result<()> {
        let mut batch = WriteBatch::default();

        for op in operations {
            match op {
                BatchOperation::Put { key, value } => {
                    batch.put(&key, &value);
                }
                BatchOperation::Delete { key } => {
                    batch.delete(&key);
                }
            }
        }

        self.db
            .write(batch)
            .map_err(|e| GraphError::storage("Failed to write batch", Some(e)))
    }

    fn flush(&mut self) -> Result<()> {
        self.db
            .flush()
            .map_err(|e| GraphError::storage("Failed to flush database", Some(e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_temp_backend() -> (RocksDBBackend, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let backend = RocksDBBackend::open(temp_dir.path()).unwrap();
        (backend, temp_dir)
    }

    #[test]
    fn test_open_creates_database() {
        let temp_dir = TempDir::new().unwrap();
        let result = RocksDBBackend::open(temp_dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_put_and_get() {
        let (mut backend, _temp) = create_temp_backend();
        backend.put(b"key1", b"value1").unwrap();

        let value = backend.get(b"key1").unwrap();
        assert_eq!(value, Some(b"value1".to_vec()));
    }

    #[test]
    fn test_get_nonexistent_key() {
        let (backend, _temp) = create_temp_backend();
        let value = backend.get(b"missing").unwrap();
        assert_eq!(value, None);
    }

    #[test]
    fn test_delete() {
        let (mut backend, _temp) = create_temp_backend();
        backend.put(b"key1", b"value1").unwrap();

        backend.delete(b"key1").unwrap();
        assert!(backend.get(b"key1").unwrap().is_none());
    }

    #[test]
    fn test_exists() {
        let (mut backend, _temp) = create_temp_backend();
        assert!(!backend.exists(b"key1").unwrap());

        backend.put(b"key1", b"value1").unwrap();
        assert!(backend.exists(b"key1").unwrap());

        backend.delete(b"key1").unwrap();
        assert!(!backend.exists(b"key1").unwrap());
    }

    #[test]
    fn test_scan_prefix() {
        let (mut backend, _temp) = create_temp_backend();
        backend.put(b"node:1", b"data1").unwrap();
        backend.put(b"node:2", b"data2").unwrap();
        backend.put(b"edge:1", b"data3").unwrap();

        let results = backend.scan_prefix(b"node:").unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().any(|(k, _)| k == b"node:1"));
        assert!(results.iter().any(|(k, _)| k == b"node:2"));
    }

    #[test]
    fn test_write_batch_puts() {
        let (mut backend, _temp) = create_temp_backend();
        let ops = vec![
            BatchOperation::Put {
                key: b"key1".to_vec(),
                value: b"value1".to_vec(),
            },
            BatchOperation::Put {
                key: b"key2".to_vec(),
                value: b"value2".to_vec(),
            },
        ];

        backend.write_batch(ops).unwrap();
        assert_eq!(backend.get(b"key1").unwrap(), Some(b"value1".to_vec()));
        assert_eq!(backend.get(b"key2").unwrap(), Some(b"value2".to_vec()));
    }

    #[test]
    fn test_write_batch_mixed_operations() {
        let (mut backend, _temp) = create_temp_backend();
        backend.put(b"key1", b"value1").unwrap();
        backend.put(b"key2", b"value2").unwrap();

        let ops = vec![
            BatchOperation::Delete {
                key: b"key1".to_vec(),
            },
            BatchOperation::Put {
                key: b"key3".to_vec(),
                value: b"value3".to_vec(),
            },
        ];

        backend.write_batch(ops).unwrap();
        assert!(backend.get(b"key1").unwrap().is_none());
        assert_eq!(backend.get(b"key2").unwrap(), Some(b"value2".to_vec()));
        assert_eq!(backend.get(b"key3").unwrap(), Some(b"value3".to_vec()));
    }

    #[test]
    fn test_flush() {
        let (mut backend, _temp) = create_temp_backend();
        backend.put(b"key1", b"value1").unwrap();

        // Should not error
        backend.flush().unwrap();
        assert_eq!(backend.get(b"key1").unwrap(), Some(b"value1".to_vec()));
    }

    #[test]
    fn test_persistence_across_reopens() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().to_path_buf();

        {
            let mut backend = RocksDBBackend::open(&path).unwrap();
            backend.put(b"persistent", b"data").unwrap();
        }

        // Reopen the database
        let backend = RocksDBBackend::open(&path).unwrap();
        assert_eq!(backend.get(b"persistent").unwrap(), Some(b"data".to_vec()));
    }
}
