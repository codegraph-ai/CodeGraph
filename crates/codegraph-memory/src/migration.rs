// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Database migration utilities
//!
//! Handles format migrations to preserve user data across versions.

use crate::error::{MemoryError, Result};
use crate::node::MemoryNode;
use rocksdb::{IteratorMode, Options, DB};
use std::path::Path;

/// Database version stored in metadata
const DB_VERSION_KEY: &[u8] = b"_db_version";
const CURRENT_VERSION: u32 = 5; // v5 = Jina Code V2 768d vectors (was BGE-Small 384d)

/// Check if database needs migration and perform if needed
pub fn migrate_if_needed(db_path: impl AsRef<Path>) -> Result<()> {
    let path = db_path.as_ref();

    // Check if database has been initialized (CURRENT file exists)
    let current_file = path.join("CURRENT");
    if !current_file.exists() {
        // Database not initialized yet, no migration needed
        log::debug!(
            "No existing database found at {}, skipping migration",
            path.display()
        );
        return Ok(());
    }

    let mut opts = Options::default();
    opts.create_if_missing(false);
    let db = DB::open(&opts, path).map_err(|e| {
        MemoryError::InvalidPath(format!("Failed to open database for migration: {}", e))
    })?;

    // Get current database version
    let current_version = match db.get(DB_VERSION_KEY)? {
        Some(bytes) => {
            let bytes_slice: &[u8] = bytes.as_ref();
            let version_bytes: [u8; 4] = bytes_slice
                .try_into()
                .map_err(|_| MemoryError::InvalidPath("Invalid version format".into()))?;
            u32::from_le_bytes(version_bytes)
        }
        None => 1, // Version 1 didn't have version key
    };

    log::info!(
        "Database version: {} (current: {})",
        current_version,
        CURRENT_VERSION
    );

    if current_version < CURRENT_VERSION {
        log::warn!(
            "Database needs migration from v{} to v{}",
            current_version,
            CURRENT_VERSION
        );
        perform_migration(&db, current_version)?;

        // Update version
        db.put(DB_VERSION_KEY, CURRENT_VERSION.to_le_bytes())?;
        db.flush()?;

        log::info!("Migration completed successfully");
    }

    Ok(())
}

/// Perform migration from old version to current
fn perform_migration(db: &DB, from_version: u32) -> Result<()> {
    // Sequential migration: apply each step in order
    if from_version < 2 {
        // v1 was JSON without version key - just set version, no conversion needed
        log::info!("Migrating v1 to v2: JSON format preserved");
    }
    if from_version == 2 {
        // v2 was bincode - need to convert back to JSON
        migrate_v2_to_v3(db)?;
    }
    if from_version < 4 {
        // v3→v4: Delete 256d Model2Vec vectors, clear embedding fields
        // Memories are preserved; vectors are regenerated with fastembed 384d on next load_cache()
        migrate_v3_to_v4(db)?;
    }
    if from_version < 5 {
        // v4→v5: Clear 384d BGE-Small vectors for Jina Code V2 768d re-embedding
        // Same pattern as v3→v4 — delete vec: keys, clear embedding fields
        migrate_v4_to_v5(db)?;
    }
    if from_version > CURRENT_VERSION {
        return Err(MemoryError::InvalidPath(format!(
            "Database version {} is newer than supported version {}",
            from_version, CURRENT_VERSION
        )));
    }
    Ok(())
}

/// Migrate from v2 (Bincode) back to v3 (JSON)
fn migrate_v2_to_v3(db: &DB) -> Result<()> {
    log::info!("Migrating database from v2 (bincode) to v3 (JSON)...");

    let mut memories_to_migrate = Vec::new();
    let iter = db.iterator(IteratorMode::Start);

    // Collect all entries
    for item in iter {
        let (key, value) = item.map_err(|e| {
            MemoryError::InvalidPath(format!("Failed to read database entry: {}", e))
        })?;
        let key_str = String::from_utf8_lossy(&key);

        if key_str.starts_with("mem:") {
            // Try deserializing as bincode (v2 format)
            match bincode::deserialize::<MemoryNode>(&value) {
                Ok(memory) => {
                    let id = key_str.strip_prefix("mem:").unwrap().to_string();
                    memories_to_migrate.push((id, memory));
                    log::debug!("Successfully parsed memory as bincode: {}", key_str);
                }
                Err(bincode_err) => {
                    // Try JSON (might already be v1/v3 format)
                    match serde_json::from_slice::<MemoryNode>(&value) {
                        Ok(memory) => {
                            log::debug!("Memory already in JSON format: {}", key_str);
                            let id = key_str.strip_prefix("mem:").unwrap().to_string();
                            memories_to_migrate.push((id, memory));
                        }
                        Err(json_err) => {
                            log::error!(
                                "Failed to deserialize memory {}: Bincode error: {}, JSON error: {}. Skipping.",
                                key_str,
                                bincode_err,
                                json_err
                            );
                            continue;
                        }
                    }
                }
            }
        }
    }

    log::info!(
        "Found {} memories to migrate to JSON",
        memories_to_migrate.len()
    );

    // Reserialize memories as JSON
    for (id, memory) in memories_to_migrate {
        let key = format!("mem:{}", id);
        match serde_json::to_vec(&memory) {
            Ok(bytes) => {
                db.put(key.as_bytes(), bytes).map_err(|e| {
                    MemoryError::InvalidPath(format!(
                        "Failed to write migrated memory {}: {}",
                        id, e
                    ))
                })?;
                log::debug!("Migrated memory to JSON: {}", id);
            }
            Err(e) => {
                log::error!("Failed to serialize memory {}: {}. Skipping.", id, e);
            }
        }
    }

    db.flush()
        .map_err(|e| MemoryError::InvalidPath(format!("Failed to flush database: {}", e)))?;

    Ok(())
}

/// Migrate from v3 (256d Model2Vec) to v4 (384d fastembed)
///
/// Deletes all `vec:` keys (incompatible dimensions) and clears `embedding`
/// fields in `mem:` entries. Memories are preserved; vectors are regenerated
/// with fastembed BGE-Small-EN-v1.5 (384d) on next `load_cache()`.
fn migrate_v3_to_v4(db: &DB) -> Result<()> {
    log::info!("Migrating v3→v4: clearing 256d vectors for fastembed 384d re-embedding...");

    let mut vec_keys_deleted = 0;
    let mut embeddings_cleared = 0;

    // Collect keys to delete (can't delete while iterating)
    let mut vec_keys: Vec<Vec<u8>> = Vec::new();
    let mut mem_updates: Vec<(Vec<u8>, MemoryNode)> = Vec::new();

    let iter = db.iterator(IteratorMode::Start);
    for item in iter {
        let (key, value) = item.map_err(|e| {
            MemoryError::InvalidPath(format!("Failed to read database entry: {}", e))
        })?;
        let key_str = String::from_utf8_lossy(&key);

        if key_str.starts_with("vec:") {
            vec_keys.push(key.to_vec());
        } else if key_str.starts_with("mem:") {
            // Clear embedding field if present
            if let Ok(mut memory) = serde_json::from_slice::<MemoryNode>(&value) {
                if memory.embedding.is_some() {
                    memory.embedding = None;
                    mem_updates.push((key.to_vec(), memory));
                }
            }
        }
    }

    // Delete all vec: keys
    for key in &vec_keys {
        db.delete(key)?;
        vec_keys_deleted += 1;
    }

    // Update mem: entries with cleared embeddings
    for (key, memory) in &mem_updates {
        if let Ok(bytes) = serde_json::to_vec(memory) {
            db.put(key, bytes)?;
            embeddings_cleared += 1;
        }
    }

    db.flush()
        .map_err(|e| MemoryError::InvalidPath(format!("Failed to flush database: {}", e)))?;

    log::info!(
        "v3→v4 migration complete: deleted {} vectors, cleared {} embeddings",
        vec_keys_deleted,
        embeddings_cleared
    );

    Ok(())
}

/// Migrate from v4 (384d BGE-Small) to v5 (768d Jina Code V2)
///
/// Same pattern as v3→v4: deletes all `vec:` keys and clears `embedding`
/// fields. Vectors are regenerated with Jina Code V2 (768d) on next `load_cache()`.
fn migrate_v4_to_v5(db: &DB) -> Result<()> {
    log::info!("Migrating v4→v5: clearing 384d BGE-Small vectors for Jina Code V2 768d re-embedding...");

    let mut vec_keys_deleted = 0;
    let mut embeddings_cleared = 0;

    let mut vec_keys: Vec<Vec<u8>> = Vec::new();
    let mut mem_updates: Vec<(Vec<u8>, MemoryNode)> = Vec::new();

    let iter = db.iterator(IteratorMode::Start);
    for item in iter {
        let (key, value) = item.map_err(|e| {
            MemoryError::InvalidPath(format!("Failed to read database entry: {}", e))
        })?;
        let key_str = String::from_utf8_lossy(&key);

        if key_str.starts_with("vec:") {
            vec_keys.push(key.to_vec());
        } else if key_str.starts_with("mem:") {
            if let Ok(mut memory) = serde_json::from_slice::<MemoryNode>(&value) {
                if memory.embedding.is_some() {
                    memory.embedding = None;
                    mem_updates.push((key.to_vec(), memory));
                }
            }
        }
    }

    for key in &vec_keys {
        db.delete(key)?;
        vec_keys_deleted += 1;
    }

    for (key, memory) in &mem_updates {
        if let Ok(bytes) = serde_json::to_vec(memory) {
            db.put(key, bytes)?;
            embeddings_cleared += 1;
        }
    }

    db.flush()
        .map_err(|e| MemoryError::InvalidPath(format!("Failed to flush database: {}", e)))?;

    log::info!(
        "v4→v5 migration complete: deleted {} vectors, cleared {} embeddings",
        vec_keys_deleted,
        embeddings_cleared
    );

    Ok(())
}

/// Migrate from v1 (JSON) to v2 (Bincode with updated format)
/// Note: This function is kept for reference but no longer used since we skipped v2
#[allow(dead_code)]
fn migrate_v1_to_v2(db: &DB) -> Result<()> {
    log::info!("Migrating database from v1 to v2...");

    let mut memories_to_migrate = Vec::new();
    let mut vectors_to_migrate = Vec::new();
    let iter = db.iterator(IteratorMode::Start);

    // Collect all entries
    for item in iter {
        let (key, value) = item.map_err(|e| {
            MemoryError::InvalidPath(format!("Failed to read database entry: {}", e))
        })?;
        let key_str = String::from_utf8_lossy(&key);

        if key_str.starts_with("mem:") {
            // Try deserializing as JSON first (v1 format)
            match serde_json::from_slice::<MemoryNode>(&value) {
                Ok(memory) => {
                    let id = key_str.strip_prefix("mem:").unwrap().to_string();
                    memories_to_migrate.push((id, memory));
                    log::debug!("Successfully parsed memory as JSON: {}", key_str);
                }
                Err(json_err) => {
                    // Try bincode (might be already migrated or corrupted)
                    match bincode::deserialize::<MemoryNode>(&value) {
                        Ok(memory) => {
                            log::debug!("Memory already in bincode format: {}", key_str);
                            let id = key_str.strip_prefix("mem:").unwrap().to_string();
                            memories_to_migrate.push((id, memory));
                        }
                        Err(bincode_err) => {
                            log::error!(
                                "Failed to deserialize memory {}: JSON error: {}, Bincode error: {}. Skipping.",
                                key_str,
                                json_err,
                                bincode_err
                            );
                            // Skip corrupted entries rather than fail entire migration
                            continue;
                        }
                    }
                }
            }
        } else if key_str.starts_with("vec:") {
            // Vectors should already be in bincode format
            match bincode::deserialize::<Vec<f32>>(&value) {
                Ok(vector) => {
                    let id = key_str.strip_prefix("vec:").unwrap().to_string();
                    vectors_to_migrate.push((id, vector));
                }
                Err(e) => {
                    log::warn!("Failed to deserialize vector {}: {}. Skipping.", key_str, e);
                }
            }
        }
    }

    log::info!(
        "Found {} memories and {} vectors to migrate",
        memories_to_migrate.len(),
        vectors_to_migrate.len()
    );

    // Reserialize memories with new bincode format
    for (id, memory) in memories_to_migrate {
        let key = format!("mem:{}", id);
        match bincode::serialize(&memory) {
            Ok(bytes) => {
                db.put(key.as_bytes(), bytes).map_err(|e| {
                    MemoryError::InvalidPath(format!(
                        "Failed to write migrated memory {}: {}",
                        id, e
                    ))
                })?;
                log::debug!("Migrated memory: {}", id);
            }
            Err(e) => {
                log::error!("Failed to serialize memory {}: {}. Skipping.", id, e);
            }
        }
    }

    // Vectors should already be fine, but rewrite them to ensure consistency
    for (id, vector) in vectors_to_migrate {
        let key = format!("vec:{}", id);
        match bincode::serialize(&vector) {
            Ok(bytes) => {
                db.put(key.as_bytes(), bytes).map_err(|e| {
                    MemoryError::InvalidPath(format!(
                        "Failed to write migrated vector {}: {}",
                        id, e
                    ))
                })?;
            }
            Err(e) => {
                log::error!("Failed to serialize vector {}: {}. Skipping.", id, e);
            }
        }
    }

    db.flush()
        .map_err(|e| MemoryError::InvalidPath(format!("Failed to flush database: {}", e)))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::{MemoryId, MemoryKind, MemoryNode, MemorySource};
    use crate::temporal::TemporalMetadata;
    use tempfile::TempDir;

    #[test]
    fn test_migration_with_json_data() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path();

        // Create v1 database with JSON-serialized data
        {
            let mut opts = Options::default();
            opts.create_if_missing(true);
            let db = DB::open(&opts, db_path).unwrap();

            let memory = MemoryNode {
                id: MemoryId::new(),
                kind: MemoryKind::DebugContext {
                    problem_description: "Test problem".into(),
                    root_cause: Some("Test cause".into()),
                    solution: "Test solution".into(),
                    symptoms: vec![],
                    related_errors: vec![],
                },
                title: "Test Memory".into(),
                content: "Test content".into(),
                temporal: TemporalMetadata::new_current(),
                code_links: vec![],
                embedding: None,
                tags: vec![],
                source: MemorySource::default(),
                confidence: 1.0,
            };

            // Store as JSON (v1 format)
            let json_bytes = serde_json::to_vec(&memory).unwrap();
            db.put(b"mem:test-id", json_bytes).unwrap();
            db.flush().unwrap();
        }

        // Run migration
        migrate_if_needed(db_path).unwrap();

        // Verify migration succeeded
        {
            let db = DB::open_default(db_path).unwrap();

            // Check version was set
            let version_bytes = db.get(DB_VERSION_KEY).unwrap().unwrap();
            let bytes_slice: &[u8] = version_bytes.as_ref();
            let version = u32::from_le_bytes(bytes_slice.try_into().unwrap());
            assert_eq!(version, CURRENT_VERSION);

            // Verify data still exists (migration preserves memories, clears vectors)
            assert!(db.get(b"mem:test-id").unwrap().is_some());

            // Verify embedding was cleared during v3→v4 migration
            let value = db.get(b"mem:test-id").unwrap().unwrap();
            let memory: MemoryNode = serde_json::from_slice(&value).unwrap();
            assert!(
                memory.embedding.is_none(),
                "v3→v4 migration should clear embeddings"
            );
        }
    }
}
