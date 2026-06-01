// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Standalone watcher daemon support.
//!
//! The daemon keeps a workspace's in-memory graph fresh (file watcher + the
//! shared live-index path + the embed queue) and persists it to the shared
//! `~/.codegraph/graph.db` with the same brief open→write→close discipline the
//! LSP and MCP servers use — so the DB is never held busy and any process can
//! load a fresh snapshot without lock contention.
//!
//! This module currently provides the **heartbeat**: a small JSON file under
//! `~/.codegraph/daemons/<slug>.json` that advertises a live daemon for a
//! workspace. Consumers (an MCP session, the LSP server) read it to decide
//! whether a daemon already owns the namespace — if so, they load the persisted
//! snapshot and skip their own startup re-index, and defer writes to the daemon.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tower_lsp::lsp_types::{InitializeParams, InitializedParams, Url, WorkspaceFolder};
use tower_lsp::{Client, LanguageServer};

use crate::backend::CodeGraphBackend;
use crate::telemetry::{current_rss_mb, emit_tel};

/// How often a running daemon refreshes its heartbeat timestamp.
pub const HEARTBEAT_INTERVAL_SECS: u64 = 10;

/// A heartbeat older than this is treated as dead; consumers fall back to
/// self-indexing. Must comfortably exceed [`HEARTBEAT_INTERVAL_SECS`] so a
/// briefly-busy daemon isn't misread as gone.
pub const STALE_AFTER_SECS: u64 = 30;

/// On-disk advertisement of a live daemon owning a workspace's namespace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonHeartbeat {
    /// OS process id of the daemon.
    pub pid: u32,
    /// Absolute workspace root the daemon is watching.
    pub workspace: PathBuf,
    /// Project slug (graph.db namespace) the daemon owns.
    pub slug: String,
    /// Unix seconds when the daemon started.
    pub started_at: u64,
    /// Unix seconds of the last heartbeat refresh (liveness signal).
    pub heartbeat_at: u64,
    /// Unix seconds of the last successful (re)index/persist.
    pub last_index_at: u64,
}

impl DaemonHeartbeat {
    /// Create a heartbeat for the current process owning `slug` / `workspace`.
    pub fn new(workspace: PathBuf, slug: String) -> Self {
        let now = now_unix();
        Self {
            pid: std::process::id(),
            workspace,
            slug,
            started_at: now,
            heartbeat_at: now,
            last_index_at: 0,
        }
    }

    /// Refresh the liveness timestamp.
    pub fn touch(&mut self) {
        self.heartbeat_at = now_unix();
    }

    /// Record that a (re)index/persist just completed (also refreshes liveness).
    pub fn mark_indexed(&mut self) {
        let now = now_unix();
        self.heartbeat_at = now;
        self.last_index_at = now;
    }

    /// True if the heartbeat is recent enough to trust the daemon as live.
    pub fn is_fresh(&self) -> bool {
        now_unix().saturating_sub(self.heartbeat_at) < STALE_AFTER_SECS
    }

    /// Persist the heartbeat to `~/.codegraph/daemons/<slug>.json`.
    pub fn write(&self) -> std::io::Result<()> {
        let path = heartbeat_path(&self.slug)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        // Write-then-rename so a reader never sees a half-written file.
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, &path)
    }

    /// Read the heartbeat for `slug`, or `None` if absent/unreadable.
    pub fn read(slug: &str) -> Option<Self> {
        let path = heartbeat_path(slug).ok()?;
        let bytes = std::fs::read(path).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Remove this daemon's heartbeat file (called on clean shutdown).
    pub fn remove(slug: &str) -> std::io::Result<()> {
        let path = heartbeat_path(slug)?;
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }
}

/// Return the live daemon owning `slug`, if one is currently advertised and
/// fresh. This is the consumer entry point: `Some` ⇒ load the persisted
/// snapshot and skip re-indexing; `None` ⇒ no daemon, proceed normally.
///
/// A stale heartbeat (daemon crashed without cleanup) is removed opportunistically.
pub fn live_daemon_for(slug: &str) -> Option<DaemonHeartbeat> {
    let hb = DaemonHeartbeat::read(slug)?;
    if hb.is_fresh() {
        Some(hb)
    } else {
        let _ = DaemonHeartbeat::remove(slug);
        None
    }
}

/// `~/.codegraph/daemons/<slug>.json`.
fn heartbeat_path(slug: &str) -> std::io::Result<PathBuf> {
    Ok(daemons_dir()?.join(format!("{slug}.json")))
}

/// `~/.codegraph/daemons/` — sits alongside the shared `graph.db`.
fn daemons_dir() -> std::io::Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Cannot determine home directory",
            )
        })?;
    Ok(Path::new(&home).join(".codegraph").join("daemons"))
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// How often the daemon persists the graph + refreshes its heartbeat.
const PERSIST_INTERVAL_SECS: u64 = 15;

/// Emit a `daemon.heartbeat` telemetry sample every this many persist ticks
/// (≈ hourly at 15s) — bounds PostHog volume for long-lived daemons while still
/// sampling RSS/uptime for the Model A/B memory call.
const HEARTBEAT_TELEMETRY_TICKS: u64 = 240;

/// Configuration for a daemon run.
pub struct DaemonConfig {
    /// Workspace root to watch.
    pub workspace: PathBuf,
    /// Extra exclude globs (merged on top of the hardcoded defaults, exactly
    /// like the extension's `codegraph.excludePatterns`).
    pub exclude_patterns: Vec<String>,
    /// Path to the extension/package dir that contains the embedding model.
    /// When absent the daemon runs graph-only (no embeddings).
    pub extension_path: Option<PathBuf>,
    /// Embedding model id (e.g. `bge-small`). Defaults to the server default.
    pub embedding_model: Option<String>,
}

/// Run the watcher daemon for a workspace until a termination signal arrives.
///
/// Reuses the full LSP startup: [`CodeGraphBackend::initialize`] loads/indexes/
/// embeds/persists the workspace and [`CodeGraphBackend::initialized`] starts the
/// file watcher. The daemon then idles, persisting the graph and refreshing its
/// heartbeat on a fixed interval — the watcher and embed queue keep the
/// in-memory graph and vectors current between flushes.
pub async fn run(config: DaemonConfig) -> Result<(), String> {
    let workspace = config
        .workspace
        .canonicalize()
        .unwrap_or_else(|_| config.workspace.clone());
    let slug = crate::memory::project_slug(&workspace);

    // Refuse to start a second writer for a workspace a live daemon already owns.
    if let Some(existing) = live_daemon_for(&slug) {
        return Err(format!(
            "a daemon (pid {}) is already watching this workspace",
            existing.pid
        ));
    }

    let backend = CodeGraphBackend::new(detached_client());

    let init_opts = serde_json::json!({
        "extensionPath": config.extension_path,
        "indexOnStartup": true,
        "excludePatterns": config.exclude_patterns,
        "embeddingModel": config.embedding_model,
        "embedOnOpen": true,
    });
    let uri = Url::from_file_path(&workspace)
        .map_err(|_| format!("invalid workspace path: {workspace:?}"))?;
    let params = InitializeParams {
        workspace_folders: Some(vec![WorkspaceFolder {
            uri,
            name: slug.clone(),
        }]),
        initialization_options: Some(init_opts),
        ..Default::default()
    };

    backend
        .initialize(params)
        .await
        .map_err(|e| format!("daemon initialize failed: {e:?}"))?;
    backend.initialized(InitializedParams {}).await;

    // Advertise ourselves as the live writer for this workspace.
    let mut heartbeat = DaemonHeartbeat::new(workspace.clone(), slug.clone());
    heartbeat.mark_indexed();
    heartbeat
        .write()
        .map_err(|e| format!("failed to write heartbeat: {e}"))?;
    tracing::info!("CodeGraph daemon watching {workspace:?} (slug {slug})");

    emit_tel(serde_json::json!({
        "event": "daemon.start",
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "version": crate::metadata::VERSION,
        "rssMb": current_rss_mb(),
    }));

    let mut tick = tokio::time::interval(Duration::from_secs(PERSIST_INTERVAL_SECS));
    tick.tick().await; // consume the immediate first tick
    let mut ticks: u64 = 0;
    loop {
        tokio::select! {
            _ = tick.tick() => {
                backend.persist_workspace_graph().await;
                heartbeat.touch();
                let _ = heartbeat.write();
                ticks += 1;
                if ticks.is_multiple_of(HEARTBEAT_TELEMETRY_TICKS) {
                    emit_tel(serde_json::json!({
                        "event": "daemon.heartbeat",
                        "uptimeSeconds": now_unix().saturating_sub(heartbeat.started_at),
                        "rssMb": current_rss_mb(),
                    }));
                }
            }
            _ = shutdown_signal() => {
                tracing::info!("CodeGraph daemon shutting down");
                break;
            }
        }
    }

    emit_tel(serde_json::json!({
        "event": "daemon.stop",
        "uptimeSeconds": now_unix().saturating_sub(heartbeat.started_at),
        "rssMb": current_rss_mb(),
    }));

    // Clean shutdown: final persist, then stop advertising.
    backend.persist_workspace_graph().await;
    let _ = DaemonHeartbeat::remove(&slug);
    Ok(())
}

/// Resolve when the process receives SIGINT (Ctrl-C) or SIGTERM.
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = term.recv() => {}
                }
            }
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

/// Build a `tower_lsp::Client` not attached to any editor.
///
/// `CodeGraphBackend` and `FileWatcher` hold a `Client` only to surface
/// notifications to an editor; the daemon has none, so we capture a client from
/// a throwaway `LspService` and drop its socket. With the receiver gone, the
/// client's `log_message`/`show_message` calls fail fast instead of buffering
/// unboundedly over the daemon's lifetime. The daemon logs via `tracing`.
fn detached_client() -> Client {
    use std::sync::Mutex;
    use tower_lsp::jsonrpc::Result as RpcResult;
    use tower_lsp::lsp_types::InitializeResult;
    use tower_lsp::LspService;

    struct Stub;
    #[tower_lsp::async_trait]
    impl LanguageServer for Stub {
        async fn initialize(&self, _: InitializeParams) -> RpcResult<InitializeResult> {
            Ok(InitializeResult::default())
        }
        async fn shutdown(&self) -> RpcResult<()> {
            Ok(())
        }
    }

    let cell: Arc<Mutex<Option<Client>>> = Arc::new(Mutex::new(None));
    let sink = Arc::clone(&cell);
    // LspService::new invokes the factory synchronously, so the client is
    // captured before this returns; `_socket` (the receiver) drops here.
    let (_service, _socket) = LspService::new(move |client| {
        *sink.lock().unwrap() = Some(client);
        Stub
    });
    let client = cell
        .lock()
        .unwrap()
        .take()
        .expect("LspService invokes the service factory synchronously");
    client
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_heartbeat_is_live_stale_is_not() {
        let mut hb = DaemonHeartbeat::new(PathBuf::from("/tmp/ws"), "slug-x".to_string());
        assert!(hb.is_fresh());
        // Force the heartbeat well past the stale threshold.
        hb.heartbeat_at = now_unix().saturating_sub(STALE_AFTER_SECS + 5);
        assert!(!hb.is_fresh());
    }

    #[test]
    fn mark_indexed_advances_both_timestamps() {
        let mut hb = DaemonHeartbeat::new(PathBuf::from("/tmp/ws"), "slug-y".to_string());
        assert_eq!(hb.last_index_at, 0);
        hb.mark_indexed();
        assert!(hb.last_index_at > 0);
        assert_eq!(hb.last_index_at, hb.heartbeat_at);
    }
}
