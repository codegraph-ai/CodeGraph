// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Memory layer integration for CodeGraph Server
//!
//! Provides persistent memory storage with semantic search for AI agent context.
//! Uses on-demand database opening to avoid lock conflicts between processes.
//!
//! Data is stored globally at `~/.codegraph/projects/<slug>/memory/` where
//! `<slug>` is derived from the workspace directory name + a short hash of
//! the full path for uniqueness (e.g. `myproject-a3f2`).

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

// Import and re-export types from codegraph_memory
pub use codegraph_memory::{
    MemoryError, MemoryNode, MemorySearch, MemoryStore, SearchConfig, SearchResult, VectorEngine,
};

/// Generate a project slug from a workspace path.
///
/// The slug is `<dir-name-lowercase>-<4-hex-hash>` where the hash is derived
/// from the full canonical path, ensuring uniqueness even when two projects
/// share the same directory name.
///
/// Example: `/Users/dev/projects/myapp` → `"myapp-a3f2"`
pub(crate) fn project_slug(workspace_path: &Path) -> String {
    // Canonicalize for stable hashing (resolve symlinks, normalize)
    let canonical = workspace_path
        .canonicalize()
        .unwrap_or_else(|_| workspace_path.to_path_buf());

    // Slug: last path component, lowercased, non-alphanumeric replaced with '-'
    let dir_name = canonical
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project");
    let slug_base: String = dir_name
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();

    // 4-hex-char hash of full path for uniqueness
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    canonical.to_string_lossy().as_ref().hash(&mut hasher);
    let hash = hasher.finish();
    let short_hash = format!("{:04x}", hash & 0xFFFF);

    format!("{slug_base}-{short_hash}")
}

/// Path to the shared graph database.
///
/// All projects share a single RocksDB at `~/.codegraph/graph.db`,
/// with per-project key namespacing via [`codegraph::NamespacedBackend`].
pub(crate) fn shared_graph_db_path() -> Result<PathBuf, MemoryError> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| MemoryError::Other("Cannot determine home directory".to_string()))?;

    Ok(PathBuf::from(home).join(".codegraph").join("graph.db"))
}

/// Derive a global data directory for a workspace under `~/.codegraph/projects/<slug>/`.
fn project_data_dir(workspace_path: &Path) -> Result<PathBuf, MemoryError> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| MemoryError::Other("Cannot determine home directory".to_string()))?;

    let slug = project_slug(workspace_path);

    Ok(PathBuf::from(home)
        .join(".codegraph")
        .join("projects")
        .join(slug))
}

/// Memory manager for the LSP server
///
/// Opens the database on-demand for each operation and closes it immediately after.
/// This allows multiple processes (VS Code extension + Claude MCP) to share the same
/// database without lock conflicts.
///
/// Data is stored at `~/.codegraph/projects/<slug>/memory/` rather than in the
/// workspace directory, keeping workspaces clean.
pub struct MemoryManager {
    /// Resolved path to memory database (e.g. ~/.codegraph/projects/<slug>/memory)
    data_dir: Arc<RwLock<Option<PathBuf>>>,
    /// Path to extension root (unused by fastembed, kept for API compatibility)
    #[allow(dead_code)]
    extension_path: Option<PathBuf>,
    /// Cached vector engine (holds model, not DB - safe to keep)
    engine: Arc<RwLock<Option<Arc<VectorEngine>>>>,
    /// Embedding model selection
    embedding_model: codegraph_memory::CodeGraphEmbeddingModel,
}

impl MemoryManager {
    /// Create a new MemoryManager
    pub fn new(extension_path: Option<PathBuf>) -> Self {
        Self::with_model(
            extension_path,
            codegraph_memory::CodeGraphEmbeddingModel::default(),
        )
    }

    /// Create a new MemoryManager with a specific embedding model
    pub fn with_model(
        extension_path: Option<PathBuf>,
        embedding_model: codegraph_memory::CodeGraphEmbeddingModel,
    ) -> Self {
        Self {
            data_dir: Arc::new(RwLock::new(None)),
            extension_path,
            engine: Arc::new(RwLock::new(None)),
            embedding_model,
        }
    }

    /// Initialize the memory manager with workspace path
    ///
    /// Resolves the global data directory at `~/.codegraph/projects/<slug>/memory/`,
    /// migrating from the old `workspace/.codegraph/memory/` location if needed.
    /// Does NOT hold the database open - that happens on-demand per operation.
    ///
    /// # Arguments
    /// * `workspace_path` - Path to the workspace root
    pub async fn initialize(&self, workspace_path: &Path) -> Result<(), MemoryError> {
        tracing::info!("[MemoryManager::initialize] Starting initialization");
        tracing::info!(
            "[MemoryManager::initialize] Workspace path: {:?}",
            workspace_path
        );

        // Resolve global data directory
        let project_dir = project_data_dir(workspace_path)?;
        let data_dir = project_dir.join("memory");
        tracing::info!("[MemoryManager::initialize] Data directory: {:?}", data_dir);

        // Auto-migrate from old workspace-local location if needed
        let old_dir = workspace_path.join(".codegraph").join("memory");
        if !data_dir.exists() && old_dir.exists() {
            tracing::info!(
                "[MemoryManager::initialize] Migrating memory from {:?} to {:?}",
                old_dir,
                data_dir
            );
            if let Err(e) = Self::migrate_data(&old_dir, &data_dir) {
                tracing::warn!(
                    "[MemoryManager::initialize] Migration failed, starting fresh: {}",
                    e
                );
            }
        }

        // Create data directory
        std::fs::create_dir_all(&data_dir).map_err(|e| {
            tracing::error!(
                "[MemoryManager::initialize] Failed to create data directory: {}",
                e
            );
            e
        })?;

        // Initialize vector engine with selected model (cached, doesn't hold DB lock)
        let cache_dir = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(".codegraph")
            .join("fastembed_cache");
        let engine = VectorEngine::with_model(cache_dir, self.embedding_model).map_err(|e| {
            tracing::error!(
                "[MemoryManager::initialize] VectorEngine initialization failed: {:?}",
                e
            );
            e
        })?;

        // Store resolved path and engine for on-demand use
        *self.data_dir.write().await = Some(data_dir.clone());
        *self.engine.write().await = Some(Arc::new(engine));

        tracing::info!(
            "[MemoryManager::initialize] Memory initialized at {:?}",
            data_dir
        );
        Ok(())
    }

    /// Migrate memory data from old workspace-local path to new global path
    fn migrate_data(old_dir: &Path, new_dir: &Path) -> Result<(), String> {
        // Ensure parent exists
        if let Some(parent) = new_dir.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create parent dir: {e}"))?;
        }

        // Move the directory
        std::fs::rename(old_dir, new_dir).map_err(|e| {
            // rename() fails across filesystems; fall back to copy
            format!("rename failed ({e}), data will be recreated at new location")
        })?;

        tracing::info!(
            "[MemoryManager] Successfully migrated memory to {:?}",
            new_dir
        );

        // Clean up empty .codegraph/ in workspace
        if let Some(codegraph_dir) = old_dir.parent() {
            if codegraph_dir
                .read_dir()
                .map(|mut d| d.next().is_none())
                .unwrap_or(false)
            {
                let _ = std::fs::remove_dir(codegraph_dir);
                tracing::info!(
                    "[MemoryManager] Removed empty {:?} from workspace",
                    codegraph_dir
                );
            }
        }

        Ok(())
    }

    /// Check if memory manager is initialized
    pub async fn is_initialized(&self) -> bool {
        self.data_dir.read().await.is_some() && self.engine.read().await.is_some()
    }

    /// Get the shared VectorEngine for embedding operations.
    /// Returns None if memory manager hasn't been initialized yet.
    pub async fn get_vector_engine(&self) -> Option<Arc<VectorEngine>> {
        self.engine.read().await.clone()
    }

    /// Open a fresh MemoryStore for an operation
    ///
    /// The store is dropped when it goes out of scope, releasing the DB lock.
    async fn open_store(&self) -> Result<MemoryStore, MemoryError> {
        let data_dir = self
            .data_dir
            .read()
            .await
            .clone()
            .ok_or_else(|| MemoryError::Other("Memory manager not initialized".to_string()))?;

        let engine = self
            .engine
            .read()
            .await
            .clone()
            .ok_or_else(|| MemoryError::Other("Vector engine not initialized".to_string()))?;

        MemoryStore::new(&data_dir, engine)
    }

    /// Store a memory node
    ///
    /// Opens DB, stores memory, closes DB.
    pub async fn put(&self, node: MemoryNode) -> Result<String, MemoryError> {
        let store = self.open_store().await?;
        store.put(node).await
    }

    /// Get a memory by ID
    ///
    /// Opens DB, retrieves memory, closes DB.
    pub async fn get(&self, id: &str) -> Result<Option<MemoryNode>, MemoryError> {
        let store = self.open_store().await?;
        Ok(store.get(id))
    }

    /// Search memories with hybrid search
    ///
    /// Opens DB, performs search, closes DB.
    pub async fn search(
        &self,
        query: &str,
        config: &SearchConfig,
        code_context: &[String],
    ) -> Result<Vec<SearchResult>, MemoryError> {
        let store = self.open_store().await?;
        let store = Arc::new(store);
        let search = MemorySearch::new(store)?;
        search.search(query, code_context, config)
    }

    /// Find memories linked to a code node
    pub async fn find_by_code_node(
        &self,
        code_node_id: &str,
    ) -> Result<Vec<MemoryNode>, MemoryError> {
        let store = self.open_store().await?;
        Ok(store.find_by_code_node(code_node_id))
    }

    /// Find memories with a specific tag
    pub async fn find_by_tag(&self, tag: &str) -> Result<Vec<MemoryNode>, MemoryError> {
        let store = self.open_store().await?;
        Ok(store.find_by_tag(tag))
    }

    /// Invalidate a memory (mark as no longer current)
    pub async fn invalidate(&self, id: &str, reason: &str) -> Result<(), MemoryError> {
        let store = self.open_store().await?;
        store.invalidate(id, reason)
    }

    /// Delete a memory permanently
    pub async fn delete(&self, id: &str) -> Result<bool, MemoryError> {
        let store = self.open_store().await?;
        store.delete(id)
    }

    /// Get all current (non-invalidated) memories
    pub async fn get_all_current(&self) -> Result<Vec<MemoryNode>, MemoryError> {
        let store = self.open_store().await?;
        Ok(store.get_all_current())
    }

    /// Get all memories, optionally including invalidated ones
    pub async fn get_all_memories(
        &self,
        current_only: bool,
    ) -> Result<Vec<MemoryNode>, MemoryError> {
        let store = self.open_store().await?;
        Ok(store.get_all_memories(current_only))
    }

    /// Get store statistics
    pub async fn stats(&self) -> Result<serde_json::Value, MemoryError> {
        let store = self.open_store().await?;
        Ok(store.stats())
    }

    /// Invalidate all memories linked to any of the given code node IDs
    ///
    /// Used for auto-invalidation when code changes.
    pub async fn invalidate_for_code_nodes(
        &self,
        node_ids: &[String],
        reason: &str,
    ) -> Result<Vec<(String, String)>, MemoryError> {
        if !self.is_initialized().await {
            return Ok(vec![]);
        }

        let store = self.open_store().await?;
        let mut invalidated = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();

        for node_id in node_ids {
            let memories = store.find_by_code_node(node_id);
            for memory in memories {
                let id_str = memory.id.to_string();
                // Avoid invalidating the same memory twice
                if seen_ids.insert(id_str.clone())
                    && memory.temporal.is_current()
                    && store.invalidate(&id_str, reason).is_ok()
                {
                    invalidated.push((id_str, memory.title.clone()));
                }
            }
        }

        if !invalidated.is_empty() {
            tracing::info!(
                "Auto-invalidated {} memories due to code changes: {}",
                invalidated.len(),
                reason
            );
        }

        Ok(invalidated)
    }

    /// Create a memory builder for convenience
    pub fn builder() -> codegraph_memory::MemoryNodeBuilder {
        MemoryNode::builder()
    }
}

// Re-export additional commonly used types for convenience
pub use codegraph_memory::{
    search::{MatchReason, MemoryKindFilter},
    CodeLink, IssueSeverity, LinkedNodeType, MemoryId, MemoryKind, MemoryNodeBuilder, MemorySource,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_project_data_dir_format() {
        // Uses a path that exists so canonicalize works
        let dir = project_data_dir(Path::new("/tmp")).unwrap();
        let dir_str = dir.to_string_lossy();

        assert!(dir_str.contains(".codegraph/projects/"));
        // Should end with slug containing "tmp" (or "private" on macOS due to canonicalize)
        // and a 4-char hex suffix
        let slug = dir.file_name().unwrap().to_string_lossy();
        // Slug format: <name>-<4hex>
        assert!(slug.len() >= 6, "slug too short: {slug}");
        let parts: Vec<&str> = slug.rsplitn(2, '-').collect();
        assert_eq!(
            parts[0].len(),
            4,
            "hash should be 4 hex chars: {}",
            parts[0]
        );
        assert!(
            parts[0].chars().all(|c| c.is_ascii_hexdigit()),
            "hash should be hex: {}",
            parts[0]
        );
    }

    #[test]
    fn test_project_data_dir_different_paths_different_hashes() {
        let dir1 = project_data_dir(Path::new("/tmp/project-a")).unwrap();
        let dir2 = project_data_dir(Path::new("/tmp/project-b")).unwrap();
        assert_ne!(dir1, dir2);
    }

    #[test]
    fn test_project_data_dir_same_name_different_parent() {
        let dir1 = project_data_dir(Path::new("/tmp/a/app")).unwrap();
        let dir2 = project_data_dir(Path::new("/tmp/b/app")).unwrap();
        // Same base name but different hashes
        let slug1 = dir1.file_name().unwrap().to_string_lossy();
        let slug2 = dir2.file_name().unwrap().to_string_lossy();
        assert!(slug1.starts_with("app-"));
        assert!(slug2.starts_with("app-"));
        assert_ne!(slug1, slug2);
    }

    #[tokio::test]
    async fn test_memory_manager_uninitialized() {
        let manager = MemoryManager::new(None);
        assert!(!manager.is_initialized().await);

        // Operations should fail when not initialized
        let result = manager.get("test-id").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    #[ignore = "requires model files"]
    async fn test_memory_manager_lifecycle() {
        use tempfile::TempDir;
        let temp_dir = TempDir::new().unwrap();
        let manager = MemoryManager::new(None);

        // Initialize
        manager.initialize(temp_dir.path()).await.unwrap();
        assert!(manager.is_initialized().await);

        // Create and store a memory
        let memory = MemoryManager::builder()
            .debug_context("Test problem", "Test solution")
            .title("Test Memory")
            .content("This is test content")
            .tag("test")
            .build()
            .unwrap();

        let id = manager.put(memory).await.unwrap();

        // Retrieve it
        let retrieved = manager.get(&id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().title, "Test Memory");

        // Search for it
        let config = SearchConfig::default();
        let results = manager.search("test problem", &config, &[]).await.unwrap();
        assert!(!results.is_empty());

        // Invalidate it
        manager.invalidate(&id, "testing").await.unwrap();
    }
}
