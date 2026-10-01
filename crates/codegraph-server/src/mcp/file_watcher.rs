// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Filesystem watcher for MCP server — auto-indexes files on change.
//!
//! Watches all indexed workspace directories for file create/modify/delete
//! events. Debounces rapid changes (2s), then incrementally updates the
//! graph: removes old nodes, re-parses changed files, re-indexes dependents,
//! resolves cross-file imports, and rebuilds search indexes.

use crate::ai_query::QueryEngine;
use crate::indexer::{IndexConfig, WorkspaceFilter};
use crate::parser_registry::ParserRegistry;
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
}

/// Shared context for the watcher's async task.
struct WatcherCtx {
    graph: Arc<RwLock<CodeGraph>>,
    parsers: Arc<ParserRegistry>,
    query_engine: Arc<QueryEngine>,
    supported_extensions: Vec<String>,
    /// The indexer's exclusion rule, so a file kept out of the initial index
    /// cannot be let back in by a later write to it.
    filter: WorkspaceFilter,
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
        config: &IndexConfig,
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
            filter: WorkspaceFilter::new(directories, config),
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
                                    if !is_watchable(path, &ctx.supported_extensions, &ctx.filter) {
                                        continue;
                                    }
                                    let path = &ctx.filter.indexed_form(path);
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
fn is_watchable(path: &Path, supported_extensions: &[String], filter: &WorkspaceFilter) -> bool {
    if path.is_dir() || !filter.admits(path) {
        return false;
    }

    // Check extension
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        supported_extensions.iter().any(|se| se == ext)
    } else {
        false
    }
}

/// Every node in the graph, grouped by the file it came from.
///
/// Built once per batch. The previous code ran a full-graph
/// `query().property("path", ..)` scan for each deleted file, each vanished
/// file, each changed file's dependents, each changed file and each dependent,
/// so a burst of generated files multiplied whole-graph scans by the size of
/// the burst (issue #23). One pass answers all of them.
fn nodes_by_path(graph: &CodeGraph) -> HashMap<String, Vec<NodeId>> {
    let mut map: HashMap<String, Vec<NodeId>> = HashMap::new();
    for (id, node) in graph.iter_nodes() {
        if let Some(path) = node.properties.get_string("path") {
            map.entry(path.to_string()).or_default().push(id);
        }
    }
    map
}

fn path_key(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

/// Process accumulated file changes: re-index changed files, remove deleted files.
async fn process_changes(ctx: &WatcherCtx, changed: &[PathBuf], removed: &[PathBuf]) {
    tracing::info!(
        "[file-watcher] Processing {} changes ({} modified, {} deleted)",
        changed.len() + removed.len(),
        changed.len(),
        removed.len()
    );

    let by_path = {
        let graph = ctx.graph.read().await;
        nodes_by_path(&graph)
    };

    // macOS FSEvents sometimes reports a delete as a modification, so a
    // "changed" path that no longer exists is a delete.
    let (changed, vanished): (Vec<PathBuf>, Vec<PathBuf>) =
        changed.iter().cloned().partition(|p| p.exists());

    let mut had_deletes = false;
    for path in removed.iter().chain(vanished.iter()) {
        // A file the index never held has nothing to remove. Skipping it here
        // keeps a burst of deletes of excluded or generated files from taking
        // the write lock at all.
        let Some(ids) = by_path.get(&path_key(path)) else {
            continue;
        };
        let mut graph = ctx.graph.write().await;
        for id in ids {
            let _ = graph.delete_node(*id);
        }
        had_deletes = true;
        tracing::info!(
            "[file-watcher] Removed {} nodes for deleted {:?}",
            ids.len(),
            path
        );
    }

    // Dependents are found before any changed file is re-parsed, while its old
    // nodes - and so their incoming edges - are still in the graph.
    let mut dependents: HashSet<PathBuf> = HashSet::new();
    if !changed.is_empty() {
        let graph = ctx.graph.read().await;
        for path in &changed {
            for node_id in by_path.get(&path_key(path)).into_iter().flatten() {
                let Ok(neighbors) = graph.get_neighbors(*node_id, codegraph::Direction::Incoming)
                else {
                    continue;
                };
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

    // One write lock per file covering both the removal and the re-parse. The
    // old code took them separately, leaving a window where a concurrent
    // search saw the file's symbols missing entirely.
    let mut indexed = 0;
    for path in changed.iter().chain(dependents.iter()) {
        let mut graph = ctx.graph.write().await;
        for id in by_path.get(&path_key(path)).into_iter().flatten() {
            let _ = graph.delete_node(*id);
        }
        if ctx.parsers.parse_file(path, &mut graph).is_ok() {
            indexed += 1;
        }
    }
    if !dependents.is_empty() {
        tracing::info!("[file-watcher] Re-indexed {} dependents", dependents.len());
    }

    // Deletions change the graph as much as edits do, so they rebuild the
    // indexes too. This used to sit inside the changed-files branch, so a batch
    // of pure deletions left the text index holding entries for nodes that no
    // longer existed until some later edit rebuilt it. Search did not show it -
    // results are resolved against the graph, where the nodes were already
    // gone - so this is index hygiene, not a visible correctness fix.
    if indexed == 0 && !had_deletes {
        return;
    }
    {
        let mut graph = ctx.graph.write().await;
        GraphUpdater::resolve_cross_file_imports(&mut graph);
    }
    ctx.query_engine.build_indexes().await;

    // Deleting a file and re-parsing one both leave vectors behind for node IDs
    // that no longer exist. One prune after all of it covers both; the old
    // per-file call ran before the nodes were deleted, so it could not see the
    // file it was meant for, and never ran at all after a re-parse.
    ctx.query_engine.prune_orphan_vectors().await;

    let reembed: Vec<String> = changed
        .iter()
        .chain(dependents.iter())
        .map(|p| path_key(p))
        .collect();
    ctx.query_engine.update_files_vectors(&reembed).await;

    tracing::info!(
        "[file-watcher] Indexed {} files (incl. dependents), indexes rebuilt",
        indexed
    );
}
