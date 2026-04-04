// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Filesystem watcher for MCP server — auto-indexes files on change.
//!
//! Watches all indexed workspace directories for file create/modify/delete
//! events. Debounces rapid changes (2s), then incrementally updates the
//! graph: removes old nodes, re-parses changed files, re-indexes dependents,
//! resolves cross-file imports, and rebuilds search indexes.

use crate::ai_query::QueryEngine;
use crate::parser_registry::ParserRegistry;
use crate::watcher::GraphUpdater;
use codegraph::CodeGraph;
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};

/// Debounce interval — wait 2 seconds after last change before processing.
const DEBOUNCE_MS: u64 = 2000;

/// Directories to skip when watching
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "__pycache__",
    ".git",
    "dist",
    "build",
    "out",
    "vendor",
    "coverage",
    "logs",
];

/// Watches workspace directories for file changes and auto-indexes them.
pub struct McpFileWatcher {
    _watcher: RecommendedWatcher,
}

/// Shared context for the watcher's async task.
struct WatcherCtx {
    graph: Arc<RwLock<CodeGraph>>,
    parsers: Arc<ParserRegistry>,
    query_engine: Arc<QueryEngine>,
    supported_extensions: Vec<String>,
}

impl McpFileWatcher {
    /// Start watching the given directories for file changes.
    ///
    /// Spawns a background tokio task that processes file events with debouncing.
    pub fn start(
        graph: Arc<RwLock<CodeGraph>>,
        parsers: Arc<ParserRegistry>,
        query_engine: Arc<QueryEngine>,
        directories: &[PathBuf],
    ) -> Result<Self, notify::Error> {
        let (tx, mut rx) = mpsc::channel::<Event>(100);

        let mut watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Ok(event) = res {
                    let _ = tx.blocking_send(event);
                }
            },
            Config::default(),
        )?;

        // Watch each workspace directory recursively
        for dir in directories {
            if dir.exists() {
                if let Err(e) = watcher.watch(dir, RecursiveMode::Recursive) {
                    tracing::warn!("Failed to watch {:?}: {}", dir, e);
                }
            }
        }

        let supported_extensions: Vec<String> = parsers
            .supported_extensions()
            .iter()
            .map(|e| e.trim_start_matches('.').to_string())
            .collect();

        let ctx = WatcherCtx {
            graph,
            parsers,
            query_engine,
            supported_extensions,
        };

        tokio::spawn(async move {
            let debounce = Duration::from_millis(DEBOUNCE_MS);
            let mut pending: HashSet<PathBuf> = HashSet::new();
            let mut deleted: HashSet<PathBuf> = HashSet::new();
            let mut last_event: Option<Instant> = None;

            loop {
                tokio::select! {
                    event = rx.recv() => {
                        match event {
                            Some(event) => {
                                for path in &event.paths {
                                    if !is_watchable(path, &ctx.supported_extensions) {
                                        continue;
                                    }
                                    match event.kind {
                                        EventKind::Create(_) | EventKind::Modify(_) => {
                                            deleted.remove(path);
                                            pending.insert(path.clone());
                                            last_event = Some(Instant::now());
                                        }
                                        EventKind::Remove(_) => {
                                            pending.remove(path);
                                            deleted.insert(path.clone());
                                            last_event = Some(Instant::now());
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            None => break, // Channel closed
                        }
                    }
                    _ = tokio::time::sleep(Duration::from_millis(500)) => {
                        // Check debounce timer
                        if let Some(last) = last_event {
                            if last.elapsed() >= debounce && (!pending.is_empty() || !deleted.is_empty()) {
                                let changed: Vec<PathBuf> = pending.drain().collect();
                                let removed: Vec<PathBuf> = deleted.drain().collect();
                                last_event = None;

                                process_changes(&ctx, &changed, &removed).await;
                            }
                        }
                    }
                }
            }
        });

        let watch_count = directories.len();
        tracing::info!("MCP file watcher started ({} directories)", watch_count);

        Ok(McpFileWatcher { _watcher: watcher })
    }
}

/// Check if a path is a supported source file worth watching.
fn is_watchable(path: &Path, supported_extensions: &[String]) -> bool {
    // Skip directories
    if path.is_dir() {
        return false;
    }

    // Skip paths in excluded directories
    let path_str = path.to_string_lossy();
    for skip in SKIP_DIRS {
        if path_str.contains(&format!("/{skip}/")) || path_str.contains(&format!("\\{skip}\\")) {
            return false;
        }
    }

    // Skip hidden files
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        if name.starts_with('.') {
            return false;
        }
    }

    // Check extension
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        supported_extensions.iter().any(|se| se == ext)
    } else {
        false
    }
}

/// Process accumulated file changes: re-index changed files, remove deleted files.
async fn process_changes(ctx: &WatcherCtx, changed: &[PathBuf], removed: &[PathBuf]) {
    let total = changed.len() + removed.len();
    tracing::info!(
        "[file-watcher] Processing {} changes ({} modified, {} deleted)",
        total,
        changed.len(),
        removed.len()
    );

    // Handle deleted files
    let mut had_deletes = false;
    if !removed.is_empty() {
        for path in removed {
            let path_str = path.to_string_lossy().to_string();
            // Remove vectors before deleting nodes (needs node IDs still in graph)
            ctx.query_engine.remove_file_vectors(&path_str).await;
            // Remove nodes and connected edges
            let mut graph = ctx.graph.write().await;
            if let Ok(old_nodes) = graph.query().property("path", path_str.as_str()).execute() {
                let count = old_nodes.len();
                for old_id in old_nodes {
                    let _ = graph.delete_node(old_id);
                }
                if count > 0 {
                    had_deletes = true;
                    tracing::info!(
                        "[file-watcher] Removed {} nodes for deleted {:?}",
                        count,
                        path
                    );
                }
            }
        }
    }

    // Separate changed files into actual changes vs files that were deleted
    // (macOS FSEvents sometimes reports deletes as modifications)
    let mut actual_changed = Vec::new();
    for path in changed {
        if path.exists() {
            actual_changed.push(path.clone());
        } else {
            // File was reported as modified but doesn't exist — treat as delete
            let path_str = path.to_string_lossy().to_string();
            ctx.query_engine.remove_file_vectors(&path_str).await;
            let mut graph = ctx.graph.write().await;
            if let Ok(old_nodes) = graph.query().property("path", path_str.as_str()).execute() {
                let count = old_nodes.len();
                for old_id in old_nodes {
                    let _ = graph.delete_node(old_id);
                }
                if count > 0 {
                    had_deletes = true;
                    tracing::info!(
                        "[file-watcher] Removed {} nodes for vanished {:?}",
                        count,
                        path
                    );
                }
            }
        }
    }
    let changed = &actual_changed;

    // Handle changed/new files
    if !changed.is_empty() {
        // Find dependents before deleting old nodes
        let mut dependents: HashSet<PathBuf> = HashSet::new();
        {
            let graph = ctx.graph.read().await;
            for path in changed {
                let path_str = path.to_string_lossy().to_string();
                if let Ok(file_nodes) = graph.query().property("path", path_str.as_str()).execute()
                {
                    for node_id in &file_nodes {
                        if let Ok(neighbors) =
                            graph.get_neighbors(*node_id, codegraph::Direction::Incoming)
                        {
                            for neighbor_id in neighbors {
                                if let Ok(neighbor) = graph.get_node(neighbor_id) {
                                    if let Some(dep_path) = neighbor.properties.get_string("path") {
                                        let dep = PathBuf::from(dep_path);
                                        if !changed.contains(&dep) && dep.exists() {
                                            dependents.insert(dep);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Delete old nodes + re-parse changed files
        let mut indexed = 0;
        for path in changed {
            {
                let mut graph = ctx.graph.write().await;
                let path_str = path.to_string_lossy().to_string();
                if let Ok(old_nodes) = graph.query().property("path", path_str.as_str()).execute() {
                    for old_id in old_nodes {
                        let _ = graph.delete_node(old_id);
                    }
                }
            }
            {
                let mut graph = ctx.graph.write().await;
                if ctx.parsers.parse_file(path, &mut graph).is_ok() {
                    indexed += 1;
                }
            }
        }

        // Re-parse dependents
        if !dependents.is_empty() {
            tracing::info!("[file-watcher] Re-indexing {} dependents", dependents.len());
            for dep in &dependents {
                {
                    let mut graph = ctx.graph.write().await;
                    let path_str = dep.to_string_lossy().to_string();
                    if let Ok(old_nodes) =
                        graph.query().property("path", path_str.as_str()).execute()
                    {
                        for old_id in old_nodes {
                            let _ = graph.delete_node(old_id);
                        }
                    }
                }
                {
                    let mut graph = ctx.graph.write().await;
                    if ctx.parsers.parse_file(dep, &mut graph).is_ok() {
                        indexed += 1;
                    }
                }
            }
        }

        if indexed > 0 || had_deletes {
            // Resolve cross-file imports
            {
                let mut graph = ctx.graph.write().await;
                GraphUpdater::resolve_cross_file_imports(&mut graph);
            }
            // Rebuild search indexes
            ctx.query_engine.build_indexes().await;
            // Incrementally re-embed changed files
            for path in changed.iter().chain(dependents.iter()) {
                let path_str = path.to_string_lossy().to_string();
                ctx.query_engine.update_file_vectors(&path_str).await;
            }

            tracing::info!(
                "[file-watcher] Indexed {} files (incl. dependents), indexes rebuilt",
                indexed
            );
        }
    }
}
