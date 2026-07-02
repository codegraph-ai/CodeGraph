// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Namespaced storage backend wrapper.
//!
//! Wraps any [`StorageBackend`] and prepends a namespace prefix to all keys.
//! This enables multiple projects to share a single database (e.g., one RocksDB)
//! with per-project key isolation.
//!
//! ## Key Scheme
//!
//! Given namespace `"my-project-a1b2"`, keys are prefixed as:
//! - `node:0` → `my-project-a1b2:node:0`
//! - `edge:5` → `my-project-a1b2:edge:5`
//! - `meta:counters` → `my-project-a1b2:meta:counters`

use super::{BatchOperation, KeyValue, StorageBackend};
use crate::error::Result;

/// A storage backend wrapper that namespaces all keys with a project prefix.
///
/// This allows multiple projects to coexist in a single database instance.
/// Each project's data is isolated by key prefix, and `scan_prefix` operations
/// are automatically scoped to the namespace.
pub struct NamespacedBackend {
    inner: Box<dyn StorageBackend>,
    prefix: Vec<u8>,
}

impl NamespacedBackend {
    /// Create a new namespaced backend wrapping the given inner backend.
    ///
    /// # Arguments
    ///
    /// * `inner` - The underlying storage backend
    /// * `namespace` - Project namespace (e.g., `"myproject-a1b2"`)
    pub fn new(inner: Box<dyn StorageBackend>, namespace: &str) -> Self {
        Self {
            inner,
            prefix: format!("{namespace}:").into_bytes(),
        }
    }

    /// Get the namespace string (without trailing colon).
    pub fn namespace(&self) -> &str {
        let prefix_str = std::str::from_utf8(&self.prefix).unwrap_or("");
        prefix_str.trim_end_matches(':')
    }

    fn prefixed_key(&self, key: &[u8]) -> Vec<u8> {
        let mut prefixed = Vec::with_capacity(self.prefix.len() + key.len());
        prefixed.extend_from_slice(&self.prefix);
        prefixed.extend_from_slice(key);
        prefixed
    }

    fn strip_prefix<'a>(&self, key: &'a [u8]) -> &'a [u8] {
        if key.starts_with(&self.prefix) {
            &key[self.prefix.len()..]
        } else {
            key
        }
    }
}

impl StorageBackend for NamespacedBackend {
    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        self.inner.put(&self.prefixed_key(key), value)
    }

    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        self.inner.get(&self.prefixed_key(key))
    }

    fn delete(&mut self, key: &[u8]) -> Result<()> {
        self.inner.delete(&self.prefixed_key(key))
    }

    fn exists(&self, key: &[u8]) -> Result<bool> {
        self.inner.exists(&self.prefixed_key(key))
    }

    fn scan_prefix(&self, prefix: &[u8]) -> Result<Vec<KeyValue>> {
        let namespaced_prefix = self.prefixed_key(prefix);
        let results = self.inner.scan_prefix(&namespaced_prefix)?;

        // Strip the namespace prefix from returned keys so callers see
        // the same keys they would with an un-namespaced backend.
        Ok(results
            .into_iter()
            .map(|(k, v)| (self.strip_prefix(&k).to_vec(), v))
            .collect())
    }

    fn write_batch(&mut self, operations: Vec<BatchOperation>) -> Result<()> {
        let namespaced_ops = operations
            .into_iter()
            .map(|op| match op {
                BatchOperation::Put { key, value } => BatchOperation::Put {
                    key: self.prefixed_key(&key),
                    value,
                },
                BatchOperation::Delete { key } => BatchOperation::Delete {
                    key: self.prefixed_key(&key),
                },
            })
            .collect();

        self.inner.write_batch(namespaced_ops)
    }

    fn flush(&mut self) -> Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryBackend;

    fn create_namespaced() -> NamespacedBackend {
        NamespacedBackend::new(Box::new(MemoryBackend::new()), "project-a")
    }

    #[test]
    fn test_put_and_get() {
        let mut backend = create_namespaced();
        backend.put(b"node:1", b"data1").unwrap();

        let value = backend.get(b"node:1").unwrap();
        assert_eq!(value, Some(b"data1".to_vec()));
    }

    #[test]
    fn test_keys_are_namespaced_in_inner() {
        let inner = MemoryBackend::new();
        let mut backend = NamespacedBackend::new(Box::new(inner.clone()), "proj-x");

        backend.put(b"node:0", b"data").unwrap();

        // The inner backend should have the prefixed key
        assert!(inner.get(b"proj-x:node:0").unwrap().is_some());
        // But not the raw key
        assert!(inner.get(b"node:0").unwrap().is_none());
    }

    #[test]
    fn test_namespace_isolation() {
        let inner = MemoryBackend::new();
        let mut backend_a = NamespacedBackend::new(Box::new(inner.clone()), "proj-a");
        let mut backend_b = NamespacedBackend::new(Box::new(inner.clone()), "proj-b");

        backend_a.put(b"node:0", b"a-data").unwrap();
        backend_b.put(b"node:0", b"b-data").unwrap();

        assert_eq!(backend_a.get(b"node:0").unwrap(), Some(b"a-data".to_vec()));
        assert_eq!(backend_b.get(b"node:0").unwrap(), Some(b"b-data".to_vec()));
    }

    #[test]
    fn test_scan_prefix_scoped() {
        let inner = MemoryBackend::new();
        let mut backend_a = NamespacedBackend::new(Box::new(inner.clone()), "proj-a");
        let mut backend_b = NamespacedBackend::new(Box::new(inner.clone()), "proj-b");

        backend_a.put(b"node:1", b"a1").unwrap();
        backend_a.put(b"node:2", b"a2").unwrap();
        backend_b.put(b"node:3", b"b3").unwrap();

        // Scanning "node:" in proj-a should only return proj-a nodes
        let results = backend_a.scan_prefix(b"node:").unwrap();
        assert_eq!(results.len(), 2);
        // Keys should be stripped of namespace prefix
        assert!(results.iter().any(|(k, _)| k == b"node:1"));
        assert!(results.iter().any(|(k, _)| k == b"node:2"));

        let results_b = backend_b.scan_prefix(b"node:").unwrap();
        assert_eq!(results_b.len(), 1);
        assert!(results_b.iter().any(|(k, _)| k == b"node:3"));
    }

    #[test]
    fn test_delete() {
        let mut backend = create_namespaced();
        backend.put(b"key1", b"value1").unwrap();
        assert!(backend.exists(b"key1").unwrap());

        backend.delete(b"key1").unwrap();
        assert!(!backend.exists(b"key1").unwrap());
    }

    #[test]
    fn test_write_batch() {
        let mut backend = create_namespaced();

        let ops = vec![
            BatchOperation::Put {
                key: b"node:1".to_vec(),
                value: b"data1".to_vec(),
            },
            BatchOperation::Put {
                key: b"node:2".to_vec(),
                value: b"data2".to_vec(),
            },
        ];

        backend.write_batch(ops).unwrap();
        assert_eq!(backend.get(b"node:1").unwrap(), Some(b"data1".to_vec()));
        assert_eq!(backend.get(b"node:2").unwrap(), Some(b"data2".to_vec()));
    }

    #[test]
    fn test_namespace_string() {
        let backend = create_namespaced();
        assert_eq!(backend.namespace(), "project-a");
    }

    #[test]
    fn test_write_batch_delete_is_namespaced() {
        // The BatchOperation::Delete arm must prefix its key with the namespace,
        // otherwise a batched delete would either miss the row or clobber another
        // namespace's identically-named key. Two namespaces share one inner
        // backend and both hold "node:1"; deleting it in proj-a via write_batch
        // must leave proj-b's "node:1" intact.
        let inner = MemoryBackend::new();
        let mut backend_a = NamespacedBackend::new(Box::new(inner.clone()), "proj-a");
        let mut backend_b = NamespacedBackend::new(Box::new(inner.clone()), "proj-b");
        backend_a.put(b"node:1", b"a1").unwrap();
        backend_b.put(b"node:1", b"b1").unwrap();

        backend_a
            .write_batch(vec![BatchOperation::Delete {
                key: b"node:1".to_vec(),
            }])
            .unwrap();

        assert!(
            backend_a.get(b"node:1").unwrap().is_none(),
            "proj-a's key should be deleted"
        );
        assert_eq!(
            backend_b.get(b"node:1").unwrap(),
            Some(b"b1".to_vec()),
            "proj-b's identically-named key must survive — Delete arm must namespace the key"
        );
        // The un-prefixed raw key was never touched.
        assert!(inner.get(b"node:1").unwrap().is_none());
    }

    #[test]
    fn test_write_batch_mixed_puts_and_deletes() {
        // Exercise both arms of write_batch's match in one call.
        let mut backend = create_namespaced();
        backend.put(b"keep", b"v").unwrap();
        backend.put(b"drop", b"v").unwrap();

        backend
            .write_batch(vec![
                BatchOperation::Delete {
                    key: b"drop".to_vec(),
                },
                BatchOperation::Put {
                    key: b"added".to_vec(),
                    value: b"new".to_vec(),
                },
            ])
            .unwrap();

        assert!(backend.get(b"drop").unwrap().is_none());
        assert_eq!(backend.get(b"keep").unwrap(), Some(b"v".to_vec()));
        assert_eq!(backend.get(b"added").unwrap(), Some(b"new".to_vec()));
    }

    #[test]
    fn test_flush_delegates_to_inner() {
        // flush() is a pass-through; verify it returns Ok and preserves data.
        let mut backend = create_namespaced();
        backend.put(b"k", b"v").unwrap();
        backend.flush().unwrap();
        assert_eq!(backend.get(b"k").unwrap(), Some(b"v".to_vec()));
    }

    #[test]
    fn test_scan_prefix_leaves_unprefixed_keys_intact() {
        // strip_prefix has an else branch for keys that do NOT start with the
        // namespace prefix — unreachable via a well-behaved backend, so drive it
        // with a mock whose scan_prefix returns a key lacking the prefix. The key
        // must pass through unchanged rather than being truncated.
        struct RawKeyBackend;
        impl StorageBackend for RawKeyBackend {
            fn put(&mut self, _k: &[u8], _v: &[u8]) -> Result<()> {
                Ok(())
            }
            fn get(&self, _k: &[u8]) -> Result<Option<Vec<u8>>> {
                Ok(None)
            }
            fn delete(&mut self, _k: &[u8]) -> Result<()> {
                Ok(())
            }
            fn exists(&self, _k: &[u8]) -> Result<bool> {
                Ok(false)
            }
            fn scan_prefix(&self, _prefix: &[u8]) -> Result<Vec<KeyValue>> {
                // A key that does NOT begin with "proj:" — hits the else arm.
                Ok(vec![(b"unprefixed:key".to_vec(), b"val".to_vec())])
            }
            fn write_batch(&mut self, _ops: Vec<BatchOperation>) -> Result<()> {
                Ok(())
            }
            fn flush(&mut self) -> Result<()> {
                Ok(())
            }
        }

        let backend = NamespacedBackend::new(Box::new(RawKeyBackend), "proj");
        let results = backend.scan_prefix(b"anything").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].0, b"unprefixed:key",
            "a key without the namespace prefix must be returned verbatim"
        );
    }
}
