// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! RocksDB storage with HNSW indexing
//!
//! Persistent storage for memories using RocksDB with LZ4 compression.
//! Uses instant-distance HNSW for O(log n) semantic search.

use dashmap::DashMap;
use instant_distance::{Builder, HnswMap, Point, Search};
use parking_lot::RwLock;
use rocksdb::{IteratorMode, Options, DB};
use std::path::Path;
use std::sync::Arc;

use crate::embedding::VectorEngine;
use crate::error::Result;
use crate::node::MemoryNode;

/// HNSW point wrapper for semantic search
#[derive(Clone)]
struct MemoryPoint {
    id: String,
    vector: Vec<f32>,
}

impl Point for MemoryPoint {
    fn distance(&self, other: &Self) -> f32 {
        // Cosine distance = 1 - similarity (HNSW finds minimum)
        1.0 - cosine_similarity(&self.vector, &other.vector)
    }
}

/// HNSW index wrapper
struct HnswIndex {
    hnsw: HnswMap<MemoryPoint, MemoryPoint>,
}

/// RocksDB-based memory store with HNSW indexing
pub struct MemoryStore {
    db: Arc<DB>,
    memory_cache: Arc<DashMap<String, MemoryNode>>,
    vector_cache: Arc<DashMap<String, Vec<f32>>>,
    hnsw_index: Arc<RwLock<Option<HnswIndex>>>,
    hnsw_points: Arc<RwLock<Vec<MemoryPoint>>>,
    engine: Arc<VectorEngine>,
}

impl MemoryStore {
    /// Create a new MemoryStore at the given path
    pub fn new(path: impl AsRef<Path>, engine: Arc<VectorEngine>) -> Result<Self> {
        let path = path.as_ref();
        std::fs::create_dir_all(path)?;

        // Run migration if needed before opening database
        crate::migration::migrate_if_needed(path)?;

        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.set_max_background_jobs(2);
        opts.set_bytes_per_sync(1048576); // 1MB
        opts.set_compression_type(rocksdb::DBCompressionType::Lz4);
        // Ensure WAL is synced for durability across on-demand open/close cycles
        opts.set_wal_dir(path);
        opts.set_manual_wal_flush(false);
        // Limit info log file accumulation (DB opens/closes per operation create LOG.old files)
        opts.set_keep_log_file_num(1);
        opts.set_recycle_log_file_num(1);
        opts.set_log_level(rocksdb::LogLevel::Error);

        let db = DB::open(&opts, path)?;

        // Set version key to current version to prevent migration on new data
        // Migration code expects v1 = JSON, but we now use JSON in v3+ too
        const DB_VERSION_KEY: &[u8] = b"_db_version";
        const CURRENT_VERSION: u32 = 5; // v5 = Jina Code V2 768d vectors
        if db.get(DB_VERSION_KEY)?.is_none() {
            db.put(DB_VERSION_KEY, CURRENT_VERSION.to_le_bytes())?;
            db.flush()?;
            log::info!("Initialized database with version {}", CURRENT_VERSION);
        }

        log::info!("MemoryStore opened at: {}", path.display());

        let store = Self {
            db: Arc::new(db),
            memory_cache: Arc::new(DashMap::new()),
            vector_cache: Arc::new(DashMap::new()),
            hnsw_index: Arc::new(RwLock::new(None)),
            hnsw_points: Arc::new(RwLock::new(Vec::new())),
            engine,
        };

        store.load_cache()?;
        Ok(store)
    }

    /// Load existing memories into cache on startup
    fn load_cache(&self) -> Result<()> {
        let mut count = 0;
        let mut skipped = 0;
        let mut total_keys = 0;
        let mut points = Vec::new();
        let iter = self.db.iterator(IteratorMode::Start);
        log::debug!("load_cache: starting iteration over RocksDB keys");

        for item in iter {
            let (key, value) = item?;
            let key_str = String::from_utf8_lossy(&key);
            total_keys += 1;
            log::debug!(
                "load_cache: found key '{}' ({} bytes)",
                key_str,
                value.len()
            );

            if key_str.starts_with("mem:") {
                let id = key_str.strip_prefix("mem:").unwrap().to_string();

                // Gracefully handle deserialization errors
                match serde_json::from_slice::<MemoryNode>(&value) {
                    Ok(memory) => {
                        if memory.temporal.is_current() {
                            self.memory_cache.insert(id.clone(), memory);

                            // Load vector
                            if let Ok(Some(vec_bytes)) =
                                self.db.get(format!("vec:{}", id).as_bytes())
                            {
                                if let Ok(vector) = bincode::deserialize::<Vec<f32>>(&vec_bytes) {
                                    self.vector_cache.insert(id.clone(), vector.clone());
                                    points.push(MemoryPoint { id, vector });
                                }
                            }

                            count += 1;
                        }
                    }
                    Err(e) => {
                        log::warn!("Failed to deserialize memory {}: {}. Skipping.", id, e);
                        skipped += 1;
                    }
                }
            }
        }

        log::info!(
            "load_cache complete: {} total keys, {} memories loaded, {} skipped",
            total_keys,
            count,
            skipped
        );

        // Re-embed memories that have no vectors (e.g. after v3→v4 migration)
        let missing_vectors: Vec<(String, String)> = self
            .memory_cache
            .iter()
            .filter(|entry| !self.vector_cache.contains_key(entry.key()))
            .map(|entry| (entry.key().clone(), entry.value().searchable_text()))
            .collect();

        if !missing_vectors.is_empty() {
            log::info!(
                "Re-embedding {} memories with missing vectors...",
                missing_vectors.len()
            );
            for (id, text) in &missing_vectors {
                match self.engine.embed(text) {
                    Ok(vector) => {
                        // Persist to RocksDB
                        let vec_key = format!("vec:{}", id);
                        if let Ok(bytes) = bincode::serialize(&vector) {
                            let _ = self.db.put(vec_key.as_bytes(), bytes);
                        }
                        self.vector_cache.insert(id.clone(), vector.clone());
                        points.push(MemoryPoint {
                            id: id.clone(),
                            vector,
                        });
                    }
                    Err(e) => {
                        log::warn!(
                            "Failed to re-embed memory {}: {}",
                            &id[..8.min(id.len())],
                            e
                        );
                    }
                }
            }
            if !missing_vectors.is_empty() {
                let _ = self.db.flush();
                log::info!(
                    "Re-embedding complete: {}/{} vectors generated",
                    points.len(),
                    missing_vectors.len()
                );
            }
        }

        if !points.is_empty() {
            self.rebuild_hnsw_index(points)?;
        }

        Ok(())
    }

    /// Store a memory with embedding
    pub async fn put(&self, mut node: MemoryNode) -> Result<String> {
        let id = node.id.to_string();

        // Generate embedding if not present
        if node.embedding.is_none() {
            let text = node.searchable_text();
            let vector = self.engine.embed(&text)?;
            node.embedding = Some(vector.clone());

            // Persist vector
            let vec_key = format!("vec:{}", id);
            self.db
                .put(vec_key.as_bytes(), bincode::serialize(&vector)?)?;
            self.vector_cache.insert(id.clone(), vector.clone());

            // Update HNSW
            let mut points = self.hnsw_points.write();
            points.push(MemoryPoint {
                id: id.clone(),
                vector,
            });
            let all_points = points.clone();
            drop(points);
            self.rebuild_hnsw_index(all_points)?;
        }

        // Persist memory using JSON (human-readable and schema-flexible)
        let mem_key = format!("mem:{}", id);
        self.db
            .put(mem_key.as_bytes(), serde_json::to_vec(&node)?)?;
        self.memory_cache.insert(id.clone(), node);

        // Flush memtable to SST files and sync WAL for immediate visibility and durability
        self.db.flush()?;
        self.db.flush_wal(true)?;
        Ok(id)
    }

    /// Get a memory by ID
    pub fn get(&self, id: &str) -> Option<MemoryNode> {
        eprintln!("[MemoryStore::get] Looking for id: {}", id);

        // First check cache
        if let Some(cached) = self.memory_cache.get(id) {
            eprintln!("[MemoryStore::get] Found in cache");
            return Some(cached.clone());
        }

        eprintln!("[MemoryStore::get] Not in cache, checking DB...");
        // If not in cache, try to load from DB directly
        let mem_key = format!("mem:{}", id);
        match self.db.get(mem_key.as_bytes()) {
            Ok(Some(value)) => {
                eprintln!("[MemoryStore::get] Found in DB, {} bytes", value.len());
                match serde_json::from_slice::<MemoryNode>(&value) {
                    Ok(memory) => {
                        eprintln!(
                            "[MemoryStore::get] Deserialized successfully, is_current: {}",
                            memory.temporal.is_current()
                        );
                        if memory.temporal.is_current() {
                            // Cache it for future use
                            self.memory_cache.insert(id.to_string(), memory.clone());
                            return Some(memory);
                        } else {
                            eprintln!("[MemoryStore::get] Memory is invalidated");
                        }
                    }
                    Err(e) => {
                        eprintln!("[MemoryStore::get] Deserialization error: {:?}", e);
                    }
                }
            }
            Ok(None) => {
                eprintln!("[MemoryStore::get] Key not found in DB");
            }
            Err(e) => {
                eprintln!("[MemoryStore::get] DB get error: {:?}", e);
            }
        }

        eprintln!("[MemoryStore::get] Returning None");
        None
    }

    /// Find memories linked to a specific code node
    pub fn find_by_code_node(&self, code_node_id: &str) -> Vec<MemoryNode> {
        self.memory_cache
            .iter()
            .filter(|entry| {
                entry
                    .value()
                    .code_links
                    .iter()
                    .any(|l| l.node_id == code_node_id)
            })
            .map(|e| e.value().clone())
            .collect()
    }

    /// Find memories with a specific tag
    pub fn find_by_tag(&self, tag: &str) -> Vec<MemoryNode> {
        self.memory_cache
            .iter()
            .filter(|entry| entry.value().tags.contains(&tag.to_string()))
            .map(|e| e.value().clone())
            .collect()
    }

    /// Invalidate a memory
    pub fn invalidate(&self, id: &str, _reason: &str) -> Result<()> {
        if let Some(mut entry) = self.memory_cache.get_mut(id) {
            entry.temporal.invalidate();
            let mem_key = format!("mem:{}", id);
            self.db
                .put(mem_key.as_bytes(), serde_json::to_vec(&*entry)?)?;
            // Flush memtable to SST files and sync WAL for immediate visibility and durability
            self.db.flush()?;
            self.db.flush_wal(true)?;
        }
        Ok(())
    }

    /// Delete a memory permanently
    pub fn delete(&self, id: &str) -> Result<bool> {
        let removed = self.memory_cache.remove(id).is_some();
        self.vector_cache.remove(id);

        let mem_key = format!("mem:{}", id);
        let vec_key = format!("vec:{}", id);
        self.db.delete(mem_key.as_bytes())?;
        self.db.delete(vec_key.as_bytes())?;
        // Flush memtable to SST files and sync WAL for immediate visibility and durability
        self.db.flush()?;
        self.db.flush_wal(true)?;

        // Rebuild HNSW without this point
        let mut points = self.hnsw_points.write();
        points.retain(|p| p.id != id);
        let all_points = points.clone();
        drop(points);
        self.rebuild_hnsw_index(all_points)?;

        Ok(removed)
    }

    /// Get all current (non-invalidated) memories
    pub fn get_all_current(&self) -> Vec<MemoryNode> {
        self.get_all_memories(true)
    }

    /// Get all memories, optionally including invalidated ones
    pub fn get_all_memories(&self, current_only: bool) -> Vec<MemoryNode> {
        let mut memories = Vec::new();

        // Iterate over all keys in RocksDB
        let iter = self.db.iterator(rocksdb::IteratorMode::Start);
        for (key, value) in iter.flatten() {
            // Only process memory keys (not vector keys)
            if let Ok(key_str) = std::str::from_utf8(&key) {
                if key_str.starts_with("mem:") {
                    if let Ok(memory) = serde_json::from_slice::<MemoryNode>(&value) {
                        if !current_only || memory.temporal.is_current() {
                            memories.push(memory);
                        }
                    }
                }
            }
        }

        memories
    }

    /// Semantic search using HNSW
    pub fn semantic_search(&self, query_vector: &[f32], limit: usize) -> Vec<(String, f32)> {
        let index_guard = self.hnsw_index.read();
        let index = match index_guard.as_ref() {
            Some(idx) => idx,
            None => return self.linear_search(query_vector, limit),
        };

        let query_point = MemoryPoint {
            id: "query".to_string(),
            vector: query_vector.to_vec(),
        };

        let mut search = Search::default();
        let points = self.hnsw_points.read();
        let mut results = Vec::new();

        for candidate in index.hnsw.search(&query_point, &mut search) {
            let point = &points[candidate.pid.into_inner() as usize];
            let similarity = cosine_similarity(query_vector, &point.vector);
            results.push((point.id.clone(), similarity));

            if results.len() >= limit {
                break;
            }
        }

        results
    }

    /// Linear search fallback
    fn linear_search(&self, query_vector: &[f32], limit: usize) -> Vec<(String, f32)> {
        let mut results: Vec<(String, f32)> = self
            .vector_cache
            .iter()
            .map(|entry| {
                let similarity = cosine_similarity(query_vector, entry.value());
                (entry.key().clone(), similarity)
            })
            .collect();

        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit);
        results
    }

    /// Rebuild HNSW index
    fn rebuild_hnsw_index(&self, points: Vec<MemoryPoint>) -> Result<()> {
        if points.is_empty() {
            *self.hnsw_index.write() = None;
            *self.hnsw_points.write() = Vec::new();
            return Ok(());
        }

        let hnsw = Builder::default()
            .ef_construction(100)
            .build(points.clone(), points.clone());

        *self.hnsw_points.write() = points;
        *self.hnsw_index.write() = Some(HnswIndex { hnsw });

        Ok(())
    }

    /// Get store statistics
    pub fn stats(&self) -> serde_json::Value {
        use std::collections::HashMap;

        let mut by_kind: HashMap<String, i32> = HashMap::new();
        let mut by_tag: HashMap<String, i32> = HashMap::new();
        let mut current_count = 0;
        let mut invalidated_count = 0;

        // Iterate over all memories in RocksDB
        let iter = self.db.iterator(rocksdb::IteratorMode::Start);
        for (key, value) in iter.flatten() {
            if let Ok(key_str) = std::str::from_utf8(&key) {
                if key_str.starts_with("mem:") {
                    if let Ok(memory) = serde_json::from_slice::<MemoryNode>(&value) {
                        if memory.temporal.is_current() {
                            current_count += 1;

                            // Count by kind
                            let kind_str = memory.kind.discriminant_name().to_string();
                            *by_kind.entry(kind_str).or_insert(0) += 1;

                            // Count by tag
                            for tag in &memory.tags {
                                *by_tag.entry(tag.clone()).or_insert(0) += 1;
                            }
                        } else {
                            invalidated_count += 1;
                        }
                    }
                }
            }
        }

        let total = current_count + invalidated_count;

        serde_json::json!({
            "totalMemories": total,
            "currentMemories": current_count,
            "invalidatedMemories": invalidated_count,
            "byKind": by_kind,
            "byTag": by_tag,
        })
    }

    /// Get the vector engine reference
    pub fn engine(&self) -> &Arc<VectorEngine> {
        &self.engine
    }
}

/// Calculate cosine similarity between two vectors
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }

    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::MemoryNode;
    use tempfile::TempDir;

    #[test]
    fn test_cosine_similarity_identical() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        assert!(cosine_similarity(&a, &b).abs() < 0.001);
    }

    #[test]
    fn test_cosine_similarity_opposite() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![-1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &b) + 1.0).abs() < 0.001);
    }

    /// Test that MemoryNode serializes/deserializes correctly with JSON
    #[test]
    fn test_memory_node_json_roundtrip() {
        let memory = MemoryNode::builder()
            .debug_context("Test problem", "Test solution")
            .title("Test Memory")
            .content("This is test content")
            .tag("test")
            .build()
            .unwrap();

        // Serialize to JSON
        let json = serde_json::to_vec(&memory).expect("serialize to JSON");
        println!(
            "Serialized JSON ({} bytes): {}",
            json.len(),
            String::from_utf8_lossy(&json)
        );

        // Deserialize from JSON
        let deserialized: MemoryNode =
            serde_json::from_slice(&json).expect("deserialize from JSON");

        assert_eq!(memory.id, deserialized.id);
        assert_eq!(memory.title, deserialized.title);
        assert_eq!(memory.content, deserialized.content);
        assert_eq!(memory.tags, deserialized.tags);
    }

    /// Test basic store and get within same store instance
    #[tokio::test]
    async fn test_store_get_same_instance() {
        let temp_dir = TempDir::new().unwrap();
        let engine = Arc::new(VectorEngine::new(None).expect("create engine"));

        let store = MemoryStore::new(temp_dir.path(), engine).expect("create store");

        let memory = MemoryNode::builder()
            .debug_context("Test problem", "Test solution")
            .title("Test Memory")
            .content("This is test content")
            .tag("test")
            .build()
            .unwrap();

        let id = store.put(memory.clone()).await.expect("store memory");
        println!("Stored memory with id: {}", id);

        // Get from same instance (should use cache)
        let retrieved = store.get(&id);
        assert!(
            retrieved.is_some(),
            "Memory should be retrievable from same instance"
        );

        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.title, "Test Memory");
        assert_eq!(retrieved.content, "This is test content");
    }

    /// Test on-demand pattern: store in one instance, get in new instance
    #[tokio::test]
    async fn test_store_get_different_instances() {
        let temp_dir = TempDir::new().unwrap();
        let engine = Arc::new(VectorEngine::new(None).expect("create engine"));

        let id: String;

        // Store in first instance
        {
            let store = MemoryStore::new(temp_dir.path(), engine.clone()).expect("create store 1");

            let memory = MemoryNode::builder()
                .debug_context("Test problem", "Test solution")
                .title("Test Memory for On-Demand")
                .content("This is test content for on-demand pattern")
                .tag("test")
                .tag("on-demand")
                .build()
                .unwrap();

            id = store.put(memory).await.expect("store memory");
            println!("Stored memory with id: {}", id);

            // Store should be dropped here, closing DB
        }

        // Get from second instance (simulating on-demand pattern)
        {
            let store2 = MemoryStore::new(temp_dir.path(), engine).expect("create store 2");

            let retrieved = store2.get(&id);
            assert!(
                retrieved.is_some(),
                "Memory should be retrievable from new instance"
            );

            let retrieved = retrieved.unwrap();
            assert_eq!(retrieved.title, "Test Memory for On-Demand");
            assert_eq!(
                retrieved.content,
                "This is test content for on-demand pattern"
            );
            assert!(retrieved.tags.contains(&"test".to_string()));
            assert!(retrieved.tags.contains(&"on-demand".to_string()));
        }
    }

    /// Test stats after storing memories
    #[tokio::test]
    async fn test_stats_after_store() {
        let temp_dir = TempDir::new().unwrap();
        let engine = Arc::new(VectorEngine::new(None).expect("create engine"));

        // Store in first instance
        {
            let store = MemoryStore::new(temp_dir.path(), engine.clone()).expect("create store 1");

            let memory1 = MemoryNode::builder()
                .debug_context("Problem 1", "Solution 1")
                .title("Memory 1")
                .content("Content 1")
                .tag("tag1")
                .build()
                .unwrap();

            let memory2 = MemoryNode::builder()
                .debug_context("Problem 2", "Solution 2")
                .title("Memory 2")
                .content("Content 2")
                .tag("tag2")
                .build()
                .unwrap();

            store.put(memory1).await.expect("store memory 1");
            store.put(memory2).await.expect("store memory 2");
        }

        // Check stats from second instance
        {
            let store2 = MemoryStore::new(temp_dir.path(), engine).expect("create store 2");
            let stats = store2.stats();

            println!("Stats: {}", serde_json::to_string_pretty(&stats).unwrap());

            assert_eq!(stats["totalMemories"], 2);
            assert_eq!(stats["currentMemories"], 2);
            assert_eq!(stats["invalidatedMemories"], 0);
        }
    }

    /// Test get_all_current after storing memories
    #[tokio::test]
    async fn test_get_all_current() {
        let temp_dir = TempDir::new().unwrap();
        let engine = Arc::new(VectorEngine::new(None).expect("create engine"));

        // Store in first instance
        {
            let store = MemoryStore::new(temp_dir.path(), engine.clone()).expect("create store 1");

            for i in 0..3 {
                let memory = MemoryNode::builder()
                    .debug_context(format!("Problem {}", i), format!("Solution {}", i))
                    .title(format!("Memory {}", i))
                    .content(format!("Content {}", i))
                    .build()
                    .unwrap();

                store.put(memory).await.expect("store memory");
            }
        }

        // Get all from second instance
        {
            let store2 = MemoryStore::new(temp_dir.path(), engine).expect("create store 2");
            let all = store2.get_all_current();

            assert_eq!(all.len(), 3, "Should have 3 current memories");
        }
    }

    /// Test invalidate persists across instances
    #[tokio::test]
    async fn test_invalidate_persists() {
        let temp_dir = TempDir::new().unwrap();
        let engine = Arc::new(VectorEngine::new(None).expect("create engine"));

        let id: String;

        // Store and invalidate in first instance
        {
            let store = MemoryStore::new(temp_dir.path(), engine.clone()).expect("create store 1");

            let memory = MemoryNode::builder()
                .debug_context("Problem", "Solution")
                .title("To Be Invalidated")
                .content("Content")
                .build()
                .unwrap();

            id = store.put(memory).await.expect("store memory");
            store.invalidate(&id, "testing").expect("invalidate");
        }

        // Check from second instance - should not be returned as current
        {
            let store2 = MemoryStore::new(temp_dir.path(), engine).expect("create store 2");

            // get() only returns current memories
            let retrieved = store2.get(&id);
            assert!(
                retrieved.is_none(),
                "Invalidated memory should not be returned by get()"
            );

            // Stats should show it as invalidated
            let stats = store2.stats();
            assert_eq!(stats["currentMemories"], 0);
            assert_eq!(stats["invalidatedMemories"], 1);
        }
    }

    /// Debug test - check what's actually stored in DB
    #[tokio::test]
    async fn test_debug_db_contents() {
        let temp_dir = TempDir::new().unwrap();
        let engine = Arc::new(VectorEngine::new(None).expect("create engine"));

        let id: String;

        // Store memory
        {
            let store = MemoryStore::new(temp_dir.path(), engine.clone()).expect("create store");

            let memory = MemoryNode::builder()
                .debug_context("Debug Test Problem", "Debug Test Solution")
                .title("Debug Test Memory")
                .content("Debug test content")
                .build()
                .unwrap();

            id = store.put(memory).await.expect("store memory");
            println!("DEBUG: Stored memory with id: {}", id);
        }

        // Read raw bytes directly (no MemoryStore)
        {
            let mut opts = Options::default();
            opts.set_wal_dir(temp_dir.path());
            let db = DB::open(&opts, temp_dir.path()).expect("open DB directly");

            // List all keys
            println!("DEBUG: Listing all keys in DB:");
            let iter = db.iterator(rocksdb::IteratorMode::Start);
            for (key, value) in iter.flatten() {
                let key_str = String::from_utf8_lossy(&key);
                println!("  Key: '{}' ({} bytes)", key_str, value.len());

                if key_str.starts_with("mem:") {
                    // Try to parse as JSON
                    match serde_json::from_slice::<serde_json::Value>(&value) {
                        Ok(v) => println!(
                            "    Valid JSON: {}",
                            v.get("title").unwrap_or(&serde_json::json!("n/a"))
                        ),
                        Err(e) => {
                            println!("    NOT valid JSON: {:?}", e);
                            println!("    First 100 bytes: {:?}", &value[..value.len().min(100)]);
                        }
                    }
                }
            }

            // Try to read the specific key
            let mem_key = format!("mem:{}", id);
            println!("DEBUG: Looking for key: '{}'", mem_key);
            match db.get(mem_key.as_bytes()) {
                Ok(Some(value)) => {
                    println!("DEBUG: Found value ({} bytes)", value.len());
                    match serde_json::from_slice::<MemoryNode>(&value) {
                        Ok(m) => println!("DEBUG: Deserialized OK: {}", m.title),
                        Err(e) => {
                            println!("DEBUG: Deser failed: {:?}", e);
                            println!(
                                "DEBUG: First 200 bytes: {}",
                                String::from_utf8_lossy(&value[..value.len().min(200)])
                            );
                        }
                    }
                }
                Ok(None) => println!("DEBUG: Key not found!"),
                Err(e) => println!("DEBUG: DB error: {:?}", e),
            }
        }

        // Now try with MemoryStore
        {
            println!("\nDEBUG: Opening with MemoryStore...");
            let store2 = MemoryStore::new(temp_dir.path(), engine).expect("create store 2");
            let result = store2.get(&id);
            println!(
                "DEBUG: MemoryStore.get result: {:?}",
                result.map(|m| m.title)
            );
        }
    }

    /// Test that raw DB read matches what was written
    #[tokio::test]
    async fn test_raw_db_read() {
        let temp_dir = TempDir::new().unwrap();
        let engine = Arc::new(VectorEngine::new(None).expect("create engine"));

        let id: String;

        // Store memory
        {
            let store = MemoryStore::new(temp_dir.path(), engine.clone()).expect("create store");

            let memory = MemoryNode::builder()
                .debug_context("Raw Test Problem", "Raw Test Solution")
                .title("Raw Test Memory")
                .content("Raw test content")
                .build()
                .unwrap();

            id = store.put(memory).await.expect("store memory");
        }

        // Read raw bytes from DB
        {
            let opts = Options::default();
            let db = DB::open(&opts, temp_dir.path()).expect("open DB directly");

            let mem_key = format!("mem:{}", id);
            let raw_value = db.get(mem_key.as_bytes()).expect("read from DB");

            assert!(raw_value.is_some(), "Value should exist in DB");
            let raw_bytes = raw_value.unwrap();

            println!(
                "Raw bytes ({} bytes): {}",
                raw_bytes.len(),
                String::from_utf8_lossy(&raw_bytes)
            );

            // Should be valid JSON
            let parsed: MemoryNode =
                serde_json::from_slice(&raw_bytes).expect("Raw bytes should be valid JSON");

            assert_eq!(parsed.title, "Raw Test Memory");
        }
    }
}
