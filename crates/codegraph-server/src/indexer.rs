// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Shared indexer — used by both MCP and LSP backends.
//!
//! Provides hash-based incremental indexing, configurable directory exclusions,
//! and cross-file import resolution. Persists file hashes via [`IndexState`] so
//! unchanged files are skipped across server restarts.

use crate::index_state::IndexState;
use crate::parser_registry::ParserRegistry;
use crate::watcher::GraphUpdater;
use codegraph::CodeGraph;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

/// Configuration for a single indexing run.
#[derive(Debug, Clone)]
pub struct IndexConfig {
    /// Directory names to always skip (e.g. `node_modules`, `target`).
    pub exclude_dirs: Vec<String>,
    /// Glob patterns for additional exclusions (user-configured).
    pub exclude_patterns: Vec<String>,
    /// Maximum file size in bytes. Files larger than this are skipped.
    pub max_file_size_bytes: u64,
    /// Maximum recursion depth for directory traversal.
    pub max_depth: u32,
    /// Maximum number of files to index in a single run.
    pub max_files: usize,
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            exclude_dirs: Self::default_exclude_dirs(),
            exclude_patterns: Vec::new(),
            max_file_size_bytes: 1024 * 1024, // 1 MiB
            max_depth: 20,
            max_files: 5_000,
        }
    }
}

impl IndexConfig {
    /// The hardcoded list of directories that are always excluded.
    pub fn default_exclude_dirs() -> Vec<String> {
        [
            "node_modules",
            "target",
            "dist",
            "build",
            "out",
            ".git",
            "__pycache__",
            "vendor",
            "DerivedData",
            "tmp",
            "coverage",
            "htmlcov",
            "results",
            "logs",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect()
    }

    /// Build a `GlobSet` from `exclude_patterns`.
    pub(crate) fn build_exclude_set(&self) -> globset::GlobSet {
        let mut builder = globset::GlobSetBuilder::new();
        for pattern in &self.exclude_patterns {
            match globset::Glob::new(pattern) {
                Ok(g) => {
                    builder.add(g);
                }
                Err(e) => {
                    tracing::warn!("Invalid exclude pattern '{}': {}", pattern, e);
                }
            }
        }
        builder.build().unwrap_or_else(|e| {
            tracing::warn!("Failed to build exclude GlobSet: {}", e);
            globset::GlobSet::empty()
        })
    }
}

/// Result of an indexing run.
#[derive(Debug, Clone, Default)]
pub struct IndexResult {
    /// Total files encountered (parsed + skipped).
    pub total_files: usize,
    /// Files that were actually parsed (new or changed).
    pub files_parsed: usize,
    /// Files skipped because their content hash was unchanged.
    pub files_skipped: usize,
}

/// Shared indexer for walking directories, hashing files, and parsing them
/// into a [`CodeGraph`].
pub struct Indexer {
    parsers: Arc<ParserRegistry>,
    index_state: Arc<Mutex<IndexState>>,
}

impl Indexer {
    /// Create a new indexer backed by the given parser registry and index state.
    pub fn new(parsers: Arc<ParserRegistry>, index_state: Arc<Mutex<IndexState>>) -> Self {
        Self {
            parsers,
            index_state,
        }
    }

    /// Compute a fast content hash (FNV-1a 64-bit).
    pub fn hash_content(content: &[u8]) -> u64 {
        let mut hash: u64 = 0xcbf29ce484222325;
        for &byte in content {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash
    }

    /// Index all workspace folders, resolve cross-file imports, and persist
    /// index state.
    ///
    /// This is the main entry-point for full (re-)indexing.
    pub async fn index_workspace(
        &self,
        graph: &Arc<RwLock<CodeGraph>>,
        folders: &[PathBuf],
        config: &IndexConfig,
    ) -> IndexResult {
        let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut result = IndexResult::default();

        for folder in folders {
            let (total, parsed, skipped) = self
                .index_directory(graph, folder, config, 0, counter.clone())
                .await;
            result.total_files += total;
            result.files_parsed += parsed;
            result.files_skipped += skipped;
        }

        // Resolve cross-file imports
        {
            let mut g = graph.write().await;
            GraphUpdater::resolve_cross_file_imports(&mut g);
        }

        // Detect runtime dependencies
        {
            let mut g = graph.write().await;
            let routes = crate::runtime_deps::detect_route_handlers(&mut g);
            let clients = crate::runtime_deps::detect_http_client_calls(&mut g);
            if routes > 0 || clients > 0 {
                let edges = crate::runtime_deps::create_runtime_call_edges(&mut g);
                tracing::info!(
                    "Runtime deps: {} routes, {} clients, {} edges",
                    routes,
                    clients,
                    edges
                );
            }
        }

        // Persist index state
        {
            let state = self.index_state.lock().await;
            state.save();
        }

        result
    }

    /// Recursively walk a directory and index supported files.
    ///
    /// Returns `(total_encountered, files_parsed, files_skipped)`.
    pub fn index_directory<'a>(
        &'a self,
        graph: &'a Arc<RwLock<CodeGraph>>,
        dir: &'a Path,
        config: &'a IndexConfig,
        depth: u32,
        counter: Arc<std::sync::atomic::AtomicUsize>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = (usize, usize, usize)> + Send + 'a>>
    {
        Box::pin(async move {
            use std::sync::atomic::Ordering;

            if depth > config.max_depth {
                tracing::warn!(
                    "Skipping {:?}: exceeded max indexing depth of {}",
                    dir,
                    config.max_depth
                );
                return (0, 0, 0);
            }

            if counter.load(Ordering::Relaxed) >= config.max_files {
                return (0, 0, 0);
            }

            let exclude_set = config.build_exclude_set();
            let supported_extensions = self.parsers.supported_extensions();

            tracing::info!("Indexing directory: {:?}", dir);

            let mut total = 0usize;
            let mut parsed = 0usize;
            let mut skipped = 0usize;

            let entries = match std::fs::read_dir(dir) {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!("Cannot read directory {:?}: {}", dir, e);
                    return (0, 0, 0);
                }
            };

            for entry in entries.flatten() {
                if counter.load(Ordering::Relaxed) >= config.max_files {
                    tracing::warn!(
                        "Reached max indexed file limit of {}; stopping",
                        config.max_files
                    );
                    break;
                }

                let path = entry.path();

                // Skip hidden files and directories
                if let Some(name) = path.file_name() {
                    if name.to_string_lossy().starts_with('.') {
                        continue;
                    }
                }

                if path.is_dir() {
                    let dir_name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();

                    // Skip hardcoded exclude directories
                    if config.exclude_dirs.iter().any(|e| e == &dir_name) {
                        continue;
                    }

                    // Skip directories matching user-configured exclude globs
                    let path_str = path.to_string_lossy();
                    if exclude_set.is_match(path_str.as_ref())
                        || exclude_set.is_match(dir_name.as_str())
                    {
                        tracing::info!("Skipping {:?}: matched exclude pattern", path);
                        continue;
                    }

                    let (t, p, s) = self
                        .index_directory(graph, &path, config, depth + 1, counter.clone())
                        .await;
                    total += t;
                    parsed += p;
                    skipped += s;
                } else if path.is_file() {
                    // Skip files matching exclude globs
                    let path_str = path.to_string_lossy();
                    if exclude_set.is_match(path_str.as_ref()) {
                        continue;
                    }

                    // Skip files that exceed the configurable size limit
                    if let Ok(metadata) = std::fs::metadata(&path) {
                        if metadata.len() > config.max_file_size_bytes {
                            tracing::info!(
                                "Skipping {:?}: file size {} exceeds limit of {}",
                                path,
                                metadata.len(),
                                config.max_file_size_bytes
                            );
                            continue;
                        }
                    }

                    // Check if file has a supported extension
                    if let Some(ext) = path.extension() {
                        let ext_str = ext.to_string_lossy();
                        let ext_with_dot = format!(".{}", ext_str);
                        let is_supported = supported_extensions
                            .iter()
                            .any(|e| *e == ext_str.as_ref() || *e == ext_with_dot);

                        if is_supported {
                            match self.index_file(graph, &path).await {
                                Ok(was_parsed) => {
                                    total += 1;
                                    counter.fetch_add(1, Ordering::Relaxed);
                                    if was_parsed {
                                        parsed += 1;
                                    } else {
                                        skipped += 1;
                                        tracing::trace!("Skipped unchanged: {:?}", path);
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!("Failed to index {:?}: {}", path, e);
                                }
                            }
                        }
                    }
                }
            }

            (total, parsed, skipped)
        })
    }

    /// Index a single file. Returns `Ok(true)` if the file was parsed,
    /// `Ok(false)` if it was skipped because the content hash is unchanged.
    pub async fn index_file(
        &self,
        graph: &Arc<RwLock<CodeGraph>>,
        path: &Path,
    ) -> Result<bool, String> {
        // Read content and compute hash
        let content = std::fs::read(path).map_err(|e| format!("Read error: {e}"))?;
        let hash = Self::hash_content(&content);

        // Check if file content has changed since last index
        {
            let state = self.index_state.lock().await;
            if let Some(cached_hash) = state.get_hash(path) {
                if cached_hash == hash {
                    return Ok(false); // Unchanged
                }
            }
        }

        // File is new or changed — remove old nodes and parse
        {
            let mut g = graph.write().await;
            let path_str = path.to_string_lossy().to_string();
            if let Ok(old_nodes) = g.query().property("path", path_str).execute() {
                for old_id in old_nodes {
                    let _ = g.delete_node(old_id);
                }
            }

            match self.parsers.parse_file(path, &mut g) {
                Ok(_file_info) => {
                    drop(g);
                    // Update hash in index state
                    let mut state = self.index_state.lock().await;
                    state.set_hash(path.to_path_buf(), hash);
                    Ok(true)
                }
                Err(e) => Err(format!("{:?}", e)),
            }
        }
    }
}
