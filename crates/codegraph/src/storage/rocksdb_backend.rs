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
use std::time::{Duration, Instant};

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
        // Crash tolerance: a hard kill mid-write (the win32 crash loops, since
        // fixed) can leave a torn WAL tail. PointInTime recovery truncates the
        // trailing corrupt record and recovers everything before it, instead
        // of refusing to open. SST/MANIFEST corruption isn't covered here — the
        // poison-pill quarantine in `open_persistent_graph` is the backstop for
        // that, since a corrupt block surfaces as a native 0xC0000005 that no
        // open-time option can catch.
        opts.set_wal_recovery_mode(rocksdb::DBRecoveryMode::PointInTime);

        let db = DB::open(&opts, path.as_ref()).map_err(|e| open_error(path.as_ref(), e))?;

        Ok(Self { db: Arc::new(db) })
    }

    /// Open, waiting up to `max_wait` for another open handle to release the
    /// database.
    ///
    /// RocksDB allows one open handle per database, and the holder is usually
    /// another process that keeps it open only for a load or a persist, so
    /// contention clears within moments. This retries with backoff while
    /// [`Self::open`] reports [`GraphError::Locked`], and returns that error
    /// once `max_wait` has elapsed.
    ///
    /// It never removes the `LOCK` file. The OS releases a lock when its
    /// holder exits, crashed or not, so a `LOCK` file left behind does not
    /// block the next open, and removing one that is still held would let a
    /// second process open the same database.
    ///
    /// # Errors
    ///
    /// Returns [`GraphError::Locked`] if the database is still held after
    /// `max_wait`, or [`GraphError::Storage`] for any other open failure.
    pub fn open_waiting_for_lock<P: AsRef<Path>>(path: P, max_wait: Duration) -> Result<Self> {
        Self::open_waiting_for_lock_with(path, max_wait, || {}, || {})
    }

    /// [`Self::open_waiting_for_lock`], calling `before_attempt` right before
    /// every open attempt and `on_locked` right after every attempt refused
    /// with [`GraphError::Locked`], including the last one. Nothing runs
    /// between `on_locked` and the next `before_attempt` except the backoff
    /// sleep, so state set up in `before_attempt` and torn down in
    /// `on_locked` never exists while waiting.
    ///
    /// # Errors
    ///
    /// As [`Self::open_waiting_for_lock`].
    pub fn open_waiting_for_lock_with<P: AsRef<Path>>(
        path: P,
        max_wait: Duration,
        mut before_attempt: impl FnMut(),
        mut on_locked: impl FnMut(),
    ) -> Result<Self> {
        let deadline = Instant::now() + max_wait;
        let mut delay = Duration::from_millis(20);
        loop {
            before_attempt();
            match Self::open(path.as_ref()) {
                Err(GraphError::Locked { .. }) if Instant::now() < deadline => {
                    on_locked();
                    std::thread::sleep(
                        delay.min(deadline.saturating_duration_since(Instant::now())),
                    );
                    delay = (delay * 2).min(Duration::from_millis(500));
                }
                result => {
                    if matches!(result, Err(GraphError::Locked { .. })) {
                        on_locked();
                    }
                    return result;
                }
            }
        }
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

    /// Open the database as a read-only **secondary** instance.
    ///
    /// A secondary opens the same on-disk database WITHOUT taking the primary's
    /// `LOCK`, so it can read a `graph.db` that a live writer (the watcher
    /// daemon) currently owns. It sees a point-in-time view of the primary's
    /// flushed state plus tailed WAL; call [`Self::try_catch_up_with_primary`]
    /// to pull in writes the primary has made since.
    ///
    /// `secondary_path` is a private scratch directory for the secondary's own
    /// info logs and manifest — it MUST be different from `primary_path`.
    ///
    /// Writes on a secondary instance fail; this is a read path only.
    ///
    /// # Errors
    ///
    /// Returns [`GraphError::Storage`] if the secondary cannot be opened (e.g.
    /// the primary database does not exist yet).
    pub fn open_as_secondary<P: AsRef<Path>, Q: AsRef<Path>>(
        primary_path: P,
        secondary_path: Q,
    ) -> Result<Self> {
        let opts = Options::default();
        let db = DB::open_as_secondary(&opts, primary_path.as_ref(), secondary_path.as_ref())
            .map_err(|e| {
                GraphError::storage(
                    format!(
                        "Failed to open RocksDB secondary at {:?} (primary {:?})",
                        secondary_path.as_ref(),
                        primary_path.as_ref()
                    ),
                    Some(e),
                )
            })?;

        Ok(Self { db: Arc::new(db) })
    }

    /// Pull in the primary's latest flushed writes.
    ///
    /// Only meaningful on an instance opened via [`Self::open_as_secondary`];
    /// a secondary does not auto-refresh, so callers invoke this before reads
    /// that must reflect the primary's current state. A no-op-ish call on a
    /// primary instance returns an error from RocksDB, which callers may ignore.
    ///
    /// # Errors
    ///
    /// Returns [`GraphError::Storage`] if catch-up fails.
    pub fn try_catch_up_with_primary(&self) -> Result<()> {
        self.db.try_catch_up_with_primary().map_err(|e| {
            GraphError::storage(
                "Failed to catch up RocksDB secondary with primary".to_string(),
                Some(e),
            )
        })
    }

    /// Get the underlying RocksDB database handle.
    ///
    /// Useful for advanced operations not exposed by the storage trait.
    pub fn db(&self) -> &Arc<DB> {
        &self.db
    }
}

/// Classify a failed `DB::open`: lock contention becomes [`GraphError::Locked`],
/// anything else [`GraphError::Storage`].
///
/// RocksDB's error is string-only. These are the messages its `LockFile`
/// returns when another handle holds the database: another process on POSIX,
/// this process on POSIX, and any holder on Windows (the `LOCK` file is opened
/// without sharing there).
fn open_error(path: &Path, e: rocksdb::Error) -> GraphError {
    const LOCK_HELD: [&str; 3] = [
        "While lock file",
        "lock hold by current process",
        "Failed to create lock file",
    ];
    let message = e.to_string();
    if LOCK_HELD.iter().any(|m| message.contains(m)) {
        GraphError::Locked {
            path: path.to_path_buf(),
            source: Box::new(e),
        }
    } else {
        GraphError::storage(format!("Failed to open RocksDB at {path:?}"), Some(e))
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

    fn scan_prefix_keys(&self, prefix: &[u8]) -> Result<Vec<Vec<u8>>> {
        let mut keys = Vec::new();
        let iter = self.db.prefix_iterator(prefix);

        for item in iter {
            let (key, _) =
                item.map_err(|e| GraphError::storage("Failed to iterate over prefix", Some(e)))?;

            // RocksDB prefix iterator may return keys beyond the prefix
            // so we need to check explicitly
            if !key.starts_with(prefix) {
                break;
            }

            keys.push(key.to_vec());
        }

        Ok(keys)
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
    fn test_secondary_reads_primary_writes_after_catch_up() {
        let primary_dir = TempDir::new().unwrap();
        let secondary_dir = TempDir::new().unwrap();
        let primary_path = primary_dir.path().to_path_buf();
        let secondary_path = secondary_dir.path().to_path_buf();

        // Primary writer owns the LOCK and writes a key.
        let mut primary = RocksDBBackend::open(&primary_path).unwrap();
        primary.put(b"alpha", b"1").unwrap();
        primary.flush().unwrap();

        // Secondary opens read-only WITHOUT taking the primary's LOCK
        // (the primary handle above is still live) and catches up to the write.
        let mut secondary =
            RocksDBBackend::open_as_secondary(&primary_path, &secondary_path).unwrap();
        secondary.try_catch_up_with_primary().unwrap();
        assert_eq!(secondary.get(b"alpha").unwrap(), Some(b"1".to_vec()));

        // A subsequent primary write is visible after another catch-up.
        primary.put(b"beta", b"2").unwrap();
        primary.flush().unwrap();
        secondary.try_catch_up_with_primary().unwrap();
        assert_eq!(secondary.get(b"beta").unwrap(), Some(b"2".to_vec()));

        // A secondary is read-only: writes must fail rather than corrupt state.
        assert!(secondary.put(b"gamma", b"3").is_err());
    }

    #[test]
    fn test_leftover_lock_file_does_not_block_open() {
        // A holder that exits, crashed or not, leaves its LOCK file behind
        // but not its lock: the next open succeeds and the file is reused.
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().to_path_buf();
        drop(RocksDBBackend::open(&db_path).unwrap());
        assert!(db_path.join("LOCK").exists());

        let backend = RocksDBBackend::open(&db_path).unwrap();
        backend.get(b"anything").unwrap();
    }

    #[test]
    fn test_open_reports_a_held_database_as_locked() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().to_path_buf();
        let _holder = RocksDBBackend::open(&db_path).unwrap();

        let err = RocksDBBackend::open(&db_path).err().unwrap();
        assert!(matches!(err, GraphError::Locked { .. }), "got {err}");
    }

    #[test]
    fn test_waiting_open_gives_up_after_max_wait_and_keeps_the_lock_file() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().to_path_buf();
        let mut holder = RocksDBBackend::open(&db_path).unwrap();
        holder.put(b"k", b"v").unwrap();

        let started = std::time::Instant::now();
        let err = RocksDBBackend::open_waiting_for_lock(&db_path, Duration::from_millis(300))
            .err()
            .unwrap();
        assert!(matches!(err, GraphError::Locked { .. }), "got {err}");
        assert!(started.elapsed() >= Duration::from_millis(300));
        assert!(db_path.join("LOCK").exists());
        // The holder is undisturbed.
        assert_eq!(holder.get(b"k").unwrap(), Some(b"v".to_vec()));
    }

    #[test]
    fn test_waiting_open_hooks_bracket_every_refused_attempt() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().to_path_buf();
        let _holder = RocksDBBackend::open(&db_path).unwrap();

        let attempts = std::cell::Cell::new(0u32);
        let refused = std::cell::Cell::new(0u32);
        let err = RocksDBBackend::open_waiting_for_lock_with(
            &db_path,
            Duration::from_millis(300),
            || {
                assert_eq!(attempts.get(), refused.get(), "attempt began while armed");
                attempts.set(attempts.get() + 1);
            },
            || refused.set(refused.get() + 1),
        )
        .err()
        .unwrap();

        assert!(matches!(err, GraphError::Locked { .. }), "got {err}");
        assert!(
            attempts.get() > 1,
            "expected retries, got {}",
            attempts.get()
        );
        assert_eq!(refused.get(), attempts.get());
    }

    #[test]
    fn test_waiting_open_hooks_leave_a_successful_attempt_armed() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().to_path_buf();
        let holder = RocksDBBackend::open(&db_path).unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            drop(holder);
        });

        let attempts = std::cell::Cell::new(0u32);
        let refused = std::cell::Cell::new(0u32);
        RocksDBBackend::open_waiting_for_lock_with(
            &db_path,
            Duration::from_secs(10),
            || attempts.set(attempts.get() + 1),
            || refused.set(refused.get() + 1),
        )
        .unwrap();
        release.join().unwrap();

        assert!(refused.get() > 0);
        assert_eq!(attempts.get(), refused.get() + 1);
    }

    #[test]
    fn test_waiting_open_succeeds_once_the_holder_releases() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().to_path_buf();
        let mut holder = RocksDBBackend::open(&db_path).unwrap();
        holder.put(b"k", b"v").unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            drop(holder);
        });

        let backend =
            RocksDBBackend::open_waiting_for_lock(&db_path, Duration::from_secs(10)).unwrap();
        assert_eq!(backend.get(b"k").unwrap(), Some(b"v".to_vec()));
        release.join().unwrap();
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
