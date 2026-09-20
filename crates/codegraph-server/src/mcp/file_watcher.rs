// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Filesystem watcher for MCP server — auto-indexes files on change.
//!
//! Watches all indexed workspace directories for file create/modify/delete
//! events. Debounces rapid changes (2s), then incrementally updates the
//! graph: removes old nodes, re-parses changed files, re-indexes dependents,
//! resolves cross-file imports, and rebuilds search indexes.

use crate::ai_query::QueryEngine;
use crate::indexer::IndexConfig;
use crate::parser_registry::ParserRegistry;
use crate::path_filter::WorkspaceFilter;
use crate::watcher::GraphUpdater;
use codegraph::{CodeGraph, NodeId};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};

/// Debounce interval — wait 2 seconds after last change before processing.
const DEBOUNCE_MS: u64 = 2000;

/// Watches workspace directories for file changes and auto-indexes them.
pub struct McpFileWatcher {
    _watcher: RecommendedWatcher,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for McpFileWatcher {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Shared context for the watcher's async task.
struct WatcherCtx {
    graph: Arc<RwLock<CodeGraph>>,
    parsers: Arc<ParserRegistry>,
    query_engine: Arc<QueryEngine>,
    supported_extensions: Vec<String>,
    max_files: usize,
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
        index_config: IndexConfig,
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
            max_files: index_config.max_files,
        };

        let mut filters: Vec<_> = directories
            .iter()
            .map(|directory| WorkspaceFilter::new(directory, &index_config))
            .collect();
        let task = tokio::spawn(async move {
            let debounce = Duration::from_millis(DEBOUNCE_MS);
            let mut pending: HashSet<PathBuf> = HashSet::new();
            let mut deleted: HashSet<PathBuf> = HashSet::new();
            let mut last_event: Option<Instant> = None;

            loop {
                tokio::select! {
                    event = rx.recv() => {
                        match event {
                            Some(event) => {
                                if !matches!(event.kind, EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)) {
                                    continue;
                                }
                                for path in &event.paths {
                                    if matches!(path.file_name().and_then(|n| n.to_str()), Some(".gitignore" | ".codegraphignore")) {
                                        for filter in &mut filters {
                                            filter.reload(&index_config);
                                        }
                                        continue;
                                    }
                                    if !is_watchable(path, &ctx.supported_extensions, &mut filters) {
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
                                let changed: Vec<PathBuf> = pending.drain()
                                    .filter(|p| is_watchable(p, &ctx.supported_extensions, &mut filters)).collect();
                                let removed: Vec<PathBuf> = deleted.drain()
                                    .filter(|p| is_watchable(p, &ctx.supported_extensions, &mut filters)).collect();
                                last_event = None;

                                process_changes(&ctx, &changed, &removed, &mut filters).await;
                            }
                        }
                    }
                }
            }
        });

        let watch_count = directories.len();
        tracing::info!("MCP file watcher started ({} directories)", watch_count);

        Ok(McpFileWatcher {
            _watcher: watcher,
            task,
        })
    }
}

/// Check if a path is a supported source file worth watching.
fn is_watchable(
    path: &Path,
    supported_extensions: &[String],
    filters: &mut [WorkspaceFilter],
) -> bool {
    // Skip directories
    if path.is_dir() {
        return false;
    }

    if !filters.iter_mut().any(|filter| filter.allows(path, false)) {
        return false;
    }

    // Check extension
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        supported_extensions.iter().any(|se| se == ext)
    } else {
        false
    }
}

/// Process accumulated file changes: re-index changed files, remove deleted files.
async fn process_changes(
    ctx: &WatcherCtx,
    changed: &[PathBuf],
    removed: &[PathBuf],
    filters: &mut [WorkspaceFilter],
) {
    let total = changed.len() + removed.len();
    if total == 0 {
        return;
    }
    tracing::info!(
        "[file-watcher] Processing {} changes ({} modified, {} deleted)",
        total,
        changed.len(),
        removed.len()
    );

    // Snapshot once per batch rather than scanning every graph node for every
    // changed/deleted/dependent file. Unknown deleted files need no graph query.
    let nodes_by_path = {
        let graph = ctx.graph.read().await;
        group_file_nodes(&graph)
    };
    let absent: HashSet<_> = removed
        .iter()
        .chain(changed.iter().filter(|p| !p.exists()))
        .collect();
    let mut file_count = nodes_by_path.len()
        - absent
            .iter()
            .filter(|p| nodes_by_path.contains_key(**p))
            .count();

    // Handle deleted files
    let mut had_deletes = false;
    if !removed.is_empty() {
        for path in removed {
            // Remove nodes and connected edges
            let mut graph = ctx.graph.write().await;
            if let Some(old_nodes) = nodes_by_path.get(path) {
                let count = old_nodes.len();
                for old_id in old_nodes {
                    let _ = graph.delete_node(*old_id);
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
            if !nodes_by_path.contains_key(path) {
                if file_count >= ctx.max_files {
                    tracing::warn!(
                        "Watcher reached max indexed file limit of {}",
                        ctx.max_files
                    );
                    continue;
                }
                file_count += 1;
            }
            actual_changed.push(path.clone());
        } else {
            // File was reported as modified but doesn't exist — treat as delete
            let mut graph = ctx.graph.write().await;
            if let Some(old_nodes) = nodes_by_path.get(path) {
                let count = old_nodes.len();
                for old_id in old_nodes {
                    let _ = graph.delete_node(*old_id);
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
                if let Some(file_nodes) = nodes_by_path.get(path) {
                    for node_id in file_nodes {
                        if let Ok(neighbors) =
                            graph.get_neighbors(*node_id, codegraph::Direction::Incoming)
                        {
                            for neighbor_id in neighbors {
                                if let Ok(neighbor) = graph.get_node(neighbor_id) {
                                    if let Some(dep_path) = neighbor.properties.get_string("path") {
                                        let dep = PathBuf::from(dep_path);
                                        if !changed.contains(&dep)
                                            && dep.exists()
                                            && is_watchable(
                                                &dep,
                                                &ctx.supported_extensions,
                                                filters,
                                            )
                                        {
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
                if let Some(old_nodes) = nodes_by_path.get(path) {
                    for old_id in old_nodes {
                        let _ = graph.delete_node(*old_id);
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
                    if let Some(old_nodes) = nodes_by_path.get(dep) {
                        for old_id in old_nodes {
                            let _ = graph.delete_node(*old_id);
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
            ctx.query_engine.prune_orphan_vectors().await;
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
    } else if had_deletes {
        ctx.query_engine.prune_orphan_vectors().await;
        ctx.query_engine.build_indexes().await;
    }
}

fn group_file_nodes(graph: &CodeGraph) -> HashMap<PathBuf, Vec<NodeId>> {
    let mut files: HashMap<PathBuf, Vec<NodeId>> = HashMap::new();
    for (&id, node) in graph.nodes_iter() {
        if let Some(path) = node.properties.get_string("path") {
            let path = Path::new(path);
            if let Some(nodes) = files.get_mut(path) {
                nodes.push(id);
            } else {
                files.insert(path.to_path_buf(), vec![id]);
            }
        }
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_query::SearchOptions;
    use crate::index_state::IndexState;
    use crate::indexer::Indexer;
    use tokio::sync::Mutex;

    fn context(max_files: usize) -> WatcherCtx {
        let graph = Arc::new(RwLock::new(CodeGraph::in_memory().unwrap()));
        WatcherCtx {
            query_engine: Arc::new(QueryEngine::new(Arc::clone(&graph))),
            graph,
            parsers: Arc::new(ParserRegistry::new()),
            supported_extensions: vec!["rs".into()],
            max_files,
        }
    }

    async fn has_symbol(ctx: &WatcherCtx, name: &str) -> bool {
        ctx.query_engine
            .symbol_search(name, &SearchOptions::new())
            .await
            .results
            .iter()
            .any(|result| result.symbol.name == name)
    }

    #[tokio::test]
    async fn watcher_updates_at_file_limit_and_rebuilds_after_deletion() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        let first = root.join("first.rs");
        let second = root.join("second.rs");
        std::fs::write(&first, "pub fn before_update() {}\n").unwrap();
        std::fs::write(&second, "pub fn second_file() {}\n").unwrap();
        let ctx = context(1);
        let config = IndexConfig {
            max_files: 1,
            ..IndexConfig::default()
        };
        let mut filters = [WorkspaceFilter::new(root, &config)];
        process_changes(&ctx, &[first.clone(), second.clone()], &[], &mut filters).await;
        assert!(has_symbol(&ctx, "before_update").await);
        assert!(!has_symbol(&ctx, "second_file").await);

        std::fs::write(&first, "pub fn after_update() {}\n").unwrap();
        process_changes(&ctx, std::slice::from_ref(&first), &[], &mut filters).await;
        assert!(has_symbol(&ctx, "after_update").await);
        assert!(!has_symbol(&ctx, "before_update").await);
        std::fs::remove_file(&first).unwrap();
        // FSEvents can report a vanished path as modified.
        process_changes(&ctx, &[first, second.clone()], &[], &mut filters).await;
        assert!(has_symbol(&ctx, "second_file").await);
        assert!(!has_symbol(&ctx, "after_update").await);
        std::fs::remove_file(&second).unwrap();
        process_changes(
            &ctx,
            &[],
            &[second, root.join("never-indexed.rs")],
            &mut filters,
        )
        .await;
        assert!(!has_symbol(&ctx, "second_file").await);
        assert_eq!(ctx.graph.read().await.node_count(), 0);
    }

    #[tokio::test]
    async fn initial_index_and_live_watcher_share_workspace_filters() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        std::fs::write(root.join(".gitignore"), "cache/\n").unwrap();
        std::fs::write(root.join(".codegraphignore"), "**/generated/**\n").unwrap();
        for directory in ["cache", "generated", "custom", "target"] {
            std::fs::create_dir(root.join(directory)).unwrap();
            std::fs::write(
                root.join(directory).join("ignored.rs"),
                "pub fn ignored() {}\n",
            )
            .unwrap();
        }
        let source = root.join("source.rs");
        std::fs::write(&source, "pub fn initial_source() {}\n").unwrap();
        let ctx = context(10);
        let mut config = IndexConfig::default();
        config.exclude_dirs.push("custom".into());
        let indexer = Indexer::new(
            Arc::clone(&ctx.parsers),
            Arc::new(Mutex::new(IndexState::new("watcher-test"))),
        );
        let (total, parsed, _, _, _) = indexer
            .index_directory(
                &ctx.graph,
                &root,
                &config,
                0,
                Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            )
            .await;
        assert_eq!((total, parsed), (1, 1));
        ctx.query_engine.build_indexes().await;
        assert!(has_symbol(&ctx, "initial_source").await);
        let watcher = McpFileWatcher::start(
            Arc::clone(&ctx.graph),
            Arc::clone(&ctx.parsers),
            Arc::clone(&ctx.query_engine),
            std::slice::from_ref(&root),
            config,
        )
        .unwrap();
        for directory in ["cache", "generated", "custom", "target"] {
            for index in 0..20 {
                std::fs::write(
                    root.join(directory).join(format!("churn{index}.rs")),
                    "pub fn unwanted_churn() {}\n",
                )
                .unwrap();
            }
        }
        std::fs::write(&source, "pub fn live_update() {}\n").unwrap();
        tokio::time::timeout(Duration::from_secs(15), async {
            while !has_symbol(&ctx, "live_update").await {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .expect("watcher did not index the allowed edit");
        assert!(!has_symbol(&ctx, "unwanted_churn").await);
        assert!(!has_symbol(&ctx, "ignored").await);
        assert!(!has_symbol(&ctx, "initial_source").await);
        assert_eq!(group_file_nodes(&*ctx.graph.read().await).len(), 1);
        drop(watcher);
    }
}
