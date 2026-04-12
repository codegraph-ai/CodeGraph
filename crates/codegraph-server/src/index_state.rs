// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Persistent index state — shared by both LSP and MCP backends.
//!
//! Saves file content hashes to disk so incremental indexing survives
//! server restarts. Only changed files are re-parsed on next startup.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Persistent index state for a project.
pub struct IndexState {
    /// Project slug (determines storage path)
    slug: String,
    /// File content hashes: path → FNV-1a hash
    hashes: HashMap<PathBuf, u64>,
}

impl IndexState {
    /// Create a new empty index state for a project.
    pub fn new(slug: &str) -> Self {
        Self {
            slug: slug.to_string(),
            hashes: HashMap::new(),
        }
    }

    /// Path to the state file: ~/.codegraph/projects/<slug>/index_state.json
    fn state_path(&self) -> PathBuf {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(home)
            .join(".codegraph")
            .join("projects")
            .join(&self.slug)
            .join("index_state.json")
    }

    /// Check if a saved state exists on disk.
    pub fn exists_on_disk(&self) -> bool {
        self.state_path().exists()
    }

    /// Load saved hashes from disk. Returns number of entries loaded.
    pub fn load(&mut self) -> usize {
        let path = self.state_path();
        let json = match std::fs::read_to_string(&path) {
            Ok(j) => j,
            Err(_) => return 0,
        };

        let saved: HashMap<String, u64> = match serde_json::from_str(&json) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("Failed to parse index state: {}", e);
                return 0;
            }
        };

        self.hashes.clear();
        for (path_str, hash) in &saved {
            self.hashes.insert(PathBuf::from(path_str), *hash);
        }

        tracing::info!(
            "Loaded index state ({} files) from {:?}",
            self.hashes.len(),
            path
        );
        self.hashes.len()
    }

    /// Save current hashes to disk.
    pub fn save(&self) {
        if self.hashes.is_empty() {
            return;
        }

        let state: HashMap<String, u64> = self
            .hashes
            .iter()
            .map(|(path, hash)| (path.display().to_string(), *hash))
            .collect();

        let path = self.state_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        match serde_json::to_string(&state) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&path, json) {
                    tracing::warn!("Failed to save index state: {}", e);
                } else {
                    tracing::info!("Saved index state ({} files)", state.len());
                }
            }
            Err(e) => tracing::warn!("Failed to serialize index state: {}", e),
        }
    }

    /// Get the hash for a file path.
    pub fn get_hash(&self, path: &Path) -> Option<u64> {
        self.hashes.get(path).copied()
    }

    /// Set the hash for a file path.
    pub fn set_hash(&mut self, path: PathBuf, hash: u64) {
        self.hashes.insert(path, hash);
    }

    /// Remove a file from the state.
    pub fn remove(&mut self, path: &Path) {
        self.hashes.remove(path);
    }

    /// Clear all hashes.
    pub fn clear(&mut self) {
        self.hashes.clear();
    }

    /// Get all hashes (for comparison).
    pub fn all_hashes(&self) -> &HashMap<PathBuf, u64> {
        &self.hashes
    }

    /// Number of tracked files.
    pub fn len(&self) -> usize {
        self.hashes.len()
    }

    /// Is the state empty?
    pub fn is_empty(&self) -> bool {
        self.hashes.is_empty()
    }

    /// Merge from an existing HashMap (used by MCP backend's file_hashes).
    pub fn merge_from(&mut self, other: &HashMap<PathBuf, u64>) {
        for (path, hash) in other {
            self.hashes.insert(path.clone(), *hash);
        }
    }
}
