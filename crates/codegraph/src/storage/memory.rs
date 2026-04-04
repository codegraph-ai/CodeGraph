// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! In-memory storage backend for testing.
//!
//! **Note**: This backend is for testing only. Do not use in production.
//! All data is lost when the backend is dropped.

use super::{BatchOperation, KeyValue, StorageBackend};
use crate::error::Result;
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

/// In-memory storage backend using a BTreeMap.
///
/// This backend provides fast operations for testing but offers no persistence.
/// Data is stored in a thread-safe `BTreeMap` behind an `Arc<RwLock<>>`.
#[derive(Debug, Clone)]
pub struct MemoryBackend {
    data: Arc<RwLock<BTreeMap<Vec<u8>, Vec<u8>>>>,
}

impl MemoryBackend {
    /// Create a new empty in-memory backend.
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }

    /// Get the number of key-value pairs stored.
    ///
    /// Useful for testing and assertions.
    pub fn len(&self) -> usize {
        self.data.read().unwrap().len()
    }

    /// Check if the backend is empty.
    pub fn is_empty(&self) -> bool {
        self.data.read().unwrap().is_empty()
    }

    /// Clear all data from the backend.
    ///
    /// Useful for resetting state between tests.
    pub fn clear(&mut self) {
        self.data.write().unwrap().clear();
    }
}

impl Default for MemoryBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl StorageBackend for MemoryBackend {
    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        self.data
            .write()
            .unwrap()
            .insert(key.to_vec(), value.to_vec());
        Ok(())
    }

    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        Ok(self.data.read().unwrap().get(key).cloned())
    }

    fn delete(&mut self, key: &[u8]) -> Result<()> {
        self.data.write().unwrap().remove(key);
        Ok(())
    }

    fn exists(&self, key: &[u8]) -> Result<bool> {
        Ok(self.data.read().unwrap().contains_key(key))
    }

    fn scan_prefix(&self, prefix: &[u8]) -> Result<Vec<KeyValue>> {
        let data = self.data.read().unwrap();
        let results: Vec<KeyValue> = data
            .range(prefix.to_vec()..)
            .take_while(|(k, _)| k.starts_with(prefix))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        Ok(results)
    }

    fn write_batch(&mut self, operations: Vec<BatchOperation>) -> Result<()> {
        let mut data = self.data.write().unwrap();
        for op in operations {
            match op {
                BatchOperation::Put { key, value } => {
                    data.insert(key, value);
                }
                BatchOperation::Delete { key } => {
                    data.remove(&key);
                }
            }
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        // No-op for in-memory backend
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_backend_is_empty() {
        let backend = MemoryBackend::new();
        assert!(backend.is_empty());
        assert_eq!(backend.len(), 0);
    }

    #[test]
    fn test_put_and_get() {
        let mut backend = MemoryBackend::new();
        backend.put(b"key1", b"value1").unwrap();

        let value = backend.get(b"key1").unwrap();
        assert_eq!(value, Some(b"value1".to_vec()));
    }

    #[test]
    fn test_get_nonexistent_key() {
        let backend = MemoryBackend::new();
        let value = backend.get(b"missing").unwrap();
        assert_eq!(value, None);
    }

    #[test]
    fn test_delete() {
        let mut backend = MemoryBackend::new();
        backend.put(b"key1", b"value1").unwrap();
        assert_eq!(backend.len(), 1);

        backend.delete(b"key1").unwrap();
        assert_eq!(backend.len(), 0);
        assert!(backend.get(b"key1").unwrap().is_none());
    }

    #[test]
    fn test_delete_nonexistent_key() {
        let mut backend = MemoryBackend::new();
        // Should not error
        backend.delete(b"missing").unwrap();
    }

    #[test]
    fn test_exists() {
        let mut backend = MemoryBackend::new();
        assert!(!backend.exists(b"key1").unwrap());

        backend.put(b"key1", b"value1").unwrap();
        assert!(backend.exists(b"key1").unwrap());

        backend.delete(b"key1").unwrap();
        assert!(!backend.exists(b"key1").unwrap());
    }

    #[test]
    fn test_scan_prefix() {
        let mut backend = MemoryBackend::new();
        backend.put(b"node:1", b"data1").unwrap();
        backend.put(b"node:2", b"data2").unwrap();
        backend.put(b"edge:1", b"data3").unwrap();

        let results = backend.scan_prefix(b"node:").unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, b"node:1");
        assert_eq!(results[1].0, b"node:2");
    }

    #[test]
    fn test_write_batch_puts() {
        let mut backend = MemoryBackend::new();
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
        assert_eq!(backend.len(), 2);
        assert_eq!(backend.get(b"key1").unwrap(), Some(b"value1".to_vec()));
        assert_eq!(backend.get(b"key2").unwrap(), Some(b"value2".to_vec()));
    }

    #[test]
    fn test_write_batch_mixed_operations() {
        let mut backend = MemoryBackend::new();
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
        assert_eq!(backend.len(), 2);
        assert!(backend.get(b"key1").unwrap().is_none());
        assert_eq!(backend.get(b"key2").unwrap(), Some(b"value2".to_vec()));
        assert_eq!(backend.get(b"key3").unwrap(), Some(b"value3".to_vec()));
    }

    #[test]
    fn test_clear() {
        let mut backend = MemoryBackend::new();
        backend.put(b"key1", b"value1").unwrap();
        backend.put(b"key2", b"value2").unwrap();
        assert_eq!(backend.len(), 2);

        backend.clear();
        assert_eq!(backend.len(), 0);
        assert!(backend.is_empty());
    }

    #[test]
    fn test_flush_is_noop() {
        let mut backend = MemoryBackend::new();
        backend.put(b"key1", b"value1").unwrap();

        // Should not error or change state
        backend.flush().unwrap();
        assert_eq!(backend.get(b"key1").unwrap(), Some(b"value1".to_vec()));
    }
}
