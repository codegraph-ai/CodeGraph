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
    /// Some when the workspace is ephemeral (test harness tempdir).
    /// Routes the state file to `<root>/.codegraph-state/index_state.json`
    /// instead of `~/.codegraph/projects/<slug>/` so we don't pollute
    /// the global registry.
    ephemeral_root: Option<PathBuf>,
}

impl IndexState {
    /// Create a new empty index state for a project.
    pub fn new(slug: &str) -> Self {
        Self {
            slug: slug.to_string(),
            hashes: HashMap::new(),
            ephemeral_root: None,
        }
    }

    /// Create state for a workspace, detecting whether the path is
    /// an ephemeral test-harness tempdir and routing state-file
    /// storage appropriately.
    pub fn for_workspace(slug: &str, workspace_path: &Path) -> Self {
        let ephemeral_root = if crate::memory::is_ephemeral_workspace(workspace_path) {
            Some(workspace_path.to_path_buf())
        } else {
            None
        };
        Self {
            slug: slug.to_string(),
            hashes: HashMap::new(),
            ephemeral_root,
        }
    }

    /// Path to the state file:
    /// - global:    `~/.codegraph/projects/<slug>/index_state.json`
    /// - ephemeral: `<workspace>/.codegraph-state/index_state.json`
    fn state_path(&self) -> PathBuf {
        if let Some(root) = &self.ephemeral_root {
            return root.join(".codegraph-state").join("index_state.json");
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an ephemeral workspace root under `dir` whose path contains a
    /// `codegraph-harness-` component, so `IndexState::for_workspace` routes
    /// its state file into `<root>/.codegraph-state/` instead of `~/.codegraph`.
    fn ephemeral_root(dir: &tempfile::TempDir) -> PathBuf {
        let root = dir.path().join("codegraph-harness-idx");
        std::fs::create_dir_all(&root).expect("create ephemeral root");
        root
    }

    #[test]
    fn new_state_is_empty() {
        let state = IndexState::new("some-project");
        assert!(state.is_empty());
        assert_eq!(state.len(), 0);
        assert!(state.all_hashes().is_empty());
    }

    #[test]
    fn set_and_get_hash_roundtrip() {
        let mut state = IndexState::new("p");
        let path = PathBuf::from("/src/main.rs");
        assert_eq!(state.get_hash(&path), None);
        state.set_hash(path.clone(), 42);
        assert_eq!(state.get_hash(&path), Some(42));
        assert_eq!(state.len(), 1);
        assert!(!state.is_empty());
    }

    #[test]
    fn set_hash_overwrites_existing() {
        let mut state = IndexState::new("p");
        let path = PathBuf::from("/src/lib.rs");
        state.set_hash(path.clone(), 1);
        state.set_hash(path.clone(), 2);
        assert_eq!(state.get_hash(&path), Some(2));
        assert_eq!(state.len(), 1);
    }

    #[test]
    fn remove_deletes_entry() {
        let mut state = IndexState::new("p");
        let path = PathBuf::from("/src/a.rs");
        state.set_hash(path.clone(), 7);
        state.remove(&path);
        assert_eq!(state.get_hash(&path), None);
        assert!(state.is_empty());
    }

    #[test]
    fn clear_removes_all_entries() {
        let mut state = IndexState::new("p");
        state.set_hash(PathBuf::from("/a"), 1);
        state.set_hash(PathBuf::from("/b"), 2);
        state.clear();
        assert!(state.is_empty());
        assert_eq!(state.len(), 0);
    }

    #[test]
    fn merge_from_inserts_and_overwrites() {
        let mut state = IndexState::new("p");
        state.set_hash(PathBuf::from("/a"), 1);
        let mut other = HashMap::new();
        other.insert(PathBuf::from("/a"), 99); // overwrites existing
        other.insert(PathBuf::from("/b"), 2); // new entry
        state.merge_from(&other);
        assert_eq!(state.get_hash(Path::new("/a")), Some(99));
        assert_eq!(state.get_hash(Path::new("/b")), Some(2));
        assert_eq!(state.len(), 2);
    }

    #[test]
    fn ephemeral_workspace_routes_state_into_workspace() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = ephemeral_root(&dir);
        let state = IndexState::for_workspace("slug", &root);
        // No disk write yet, so nothing exists.
        assert!(!state.exists_on_disk());
    }

    #[test]
    fn save_then_load_roundtrips_hashes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = ephemeral_root(&dir);

        let mut writer = IndexState::for_workspace("slug", &root);
        writer.set_hash(PathBuf::from("/src/x.rs"), 111);
        writer.set_hash(PathBuf::from("/src/y.rs"), 222);
        writer.save();
        assert!(writer.exists_on_disk());
        assert!(root
            .join(".codegraph-state")
            .join("index_state.json")
            .exists());

        let mut reader = IndexState::for_workspace("slug", &root);
        assert_eq!(reader.load(), 2);
        assert_eq!(reader.get_hash(Path::new("/src/x.rs")), Some(111));
        assert_eq!(reader.get_hash(Path::new("/src/y.rs")), Some(222));
    }

    #[test]
    fn save_is_noop_when_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = ephemeral_root(&dir);
        let state = IndexState::for_workspace("slug", &root);
        state.save();
        assert!(!state.exists_on_disk());
    }

    #[test]
    fn load_missing_file_returns_zero() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = ephemeral_root(&dir);
        let mut state = IndexState::for_workspace("slug", &root);
        assert_eq!(state.load(), 0);
        assert!(state.is_empty());
    }

    #[test]
    fn load_clears_prior_in_memory_hashes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = ephemeral_root(&dir);

        let mut writer = IndexState::for_workspace("slug", &root);
        writer.set_hash(PathBuf::from("/persisted.rs"), 5);
        writer.save();

        let mut reader = IndexState::for_workspace("slug", &root);
        // A stale in-memory entry that is not on disk must be dropped by load.
        reader.set_hash(PathBuf::from("/stale.rs"), 9);
        assert_eq!(reader.load(), 1);
        assert_eq!(reader.get_hash(Path::new("/stale.rs")), None);
        assert_eq!(reader.get_hash(Path::new("/persisted.rs")), Some(5));
    }

    #[test]
    fn load_ignores_corrupt_json() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = ephemeral_root(&dir);
        let state_dir = root.join(".codegraph-state");
        std::fs::create_dir_all(&state_dir).expect("state dir");
        std::fs::write(state_dir.join("index_state.json"), "not json{").expect("write");

        let mut state = IndexState::for_workspace("slug", &root);
        assert_eq!(state.load(), 0);
    }
}
