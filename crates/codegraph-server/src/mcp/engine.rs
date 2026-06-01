// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Model B socket engine.
//!
//! One resident process holds the heavy state — N project graphs plus a SINGLE
//! shared ONNX model — and serves the MCP tool surface to many thin clients over
//! a Unix socket. Each agent session is a `--connect` relay (~20 MB, no graph,
//! no model), so per-session RAM stops scaling with session count.
//!
//! Multi-root: a connection's first line is an attach frame
//! (`{"cg_attach":{"workspace":"<abs>"}}`) naming its workspace; the engine
//! lazily loads that workspace's backend (reusing the shared model) and routes
//! the connection's requests to it via the `&self` `handle_request_shared`.

use std::path::PathBuf;

use codegraph_memory::CodeGraphEmbeddingModel;

use super::server::McpServer;

/// Configuration for a running engine.
pub struct EngineConfig {
    pub socket_path: PathBuf,
    pub embedding_model: CodeGraphEmbeddingModel,
    pub exclude_dirs: Vec<String>,
    pub max_files: usize,
    pub full_body_embedding: bool,
    /// Workspaces to pre-load at startup (optional; others load on attach).
    pub seeds: Vec<PathBuf>,
}

/// `~/.codegraph/fastembed_cache` — where the embedding model is cached.
#[cfg(unix)]
fn model_cache_dir() -> PathBuf {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".codegraph")
        .join("fastembed_cache")
}

#[cfg(unix)]
mod imp {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::{UnixListener, UnixStream};
    use tokio::sync::Mutex;

    use codegraph_memory::VectorEngine;

    type Registry = Arc<Mutex<HashMap<PathBuf, Arc<McpServer>>>>;

    struct Engine {
        cfg: EngineConfig,
        registry: Registry,
        /// One model shared by every workspace; `None` if the model couldn't be
        /// loaded (low memory) — workspaces then run graph-only.
        shared_engine: Option<Arc<VectorEngine>>,
    }

    /// Load (or fetch from the registry) the backend for `workspace`, reusing the
    /// shared model. Builds outside the registry lock so a slow first index of
    /// one workspace doesn't block attaches to others.
    async fn get_or_load(engine: &Arc<Engine>, workspace: PathBuf) -> Arc<McpServer> {
        let ws = workspace
            .canonicalize()
            .unwrap_or_else(|_| workspace.clone());
        if let Some(s) = engine.registry.lock().await.get(&ws).cloned() {
            return s;
        }

        tracing::info!("Engine: loading workspace {}", ws.display());
        let mut server = McpServer::new(
            vec![ws.clone()],
            engine.cfg.exclude_dirs.clone(),
            engine.cfg.max_files,
            engine.cfg.embedding_model,
            engine.cfg.full_body_embedding,
        );
        if let Some(shared) = &engine.shared_engine {
            server.set_shared_engine(Arc::clone(shared)).await;
        }
        server.ensure_indexed().await;
        let server = Arc::new(server);

        let mut reg = engine.registry.lock().await;
        // Another connection may have loaded it while we built — prefer theirs.
        reg.entry(ws).or_insert_with(|| Arc::clone(&server));
        server
    }

    /// First line of a connection: an attach frame selects the workspace.
    /// Returns `(workspace, leftover_request_line)` — if the first line was a
    /// plain JSON-RPC request instead, it's returned to be dispatched.
    fn parse_attach(line: &str) -> (Option<PathBuf>, Option<String>) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            if let Some(ws) = v
                .get("cg_attach")
                .and_then(|a| a.get("workspace"))
                .and_then(|w| w.as_str())
            {
                return (Some(PathBuf::from(ws)), None);
            }
        }
        (None, Some(line.to_string()))
    }

    async fn handle_conn(engine: Arc<Engine>, stream: UnixStream) {
        let (read_half, mut write_half) = stream.into_split();
        let mut lines = BufReader::new(read_half).lines();

        // Resolve the connection's workspace from the attach frame (or fall back
        // to the first seed, or the engine's cwd).
        let first = match lines.next_line().await {
            Ok(Some(l)) => l,
            _ => return,
        };
        let (attach_ws, pending) = parse_attach(&first);
        let workspace = attach_ws
            .or_else(|| engine.cfg.seeds.first().cloned())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let server = get_or_load(&engine, workspace).await;

        // If the first line was actually a request (no attach frame), dispatch it.
        let mut queued = pending;
        loop {
            let line = match queued.take() {
                Some(l) => l,
                None => match lines.next_line().await {
                    Ok(Some(l)) => l,
                    Ok(None) | Err(_) => break,
                },
            };
            if line.trim().is_empty() {
                continue;
            }
            let req: crate::mcp::JsonRpcRequest = match serde_json::from_str(&line) {
                Ok(r) => r,
                Err(e) => {
                    tracing::debug!("Engine: skipping malformed JSON-RPC line: {e}");
                    continue;
                }
            };
            if let Some(resp) = server.handle_request_shared(req).await {
                match serde_json::to_vec(&resp) {
                    Ok(mut bytes) => {
                        bytes.push(b'\n');
                        if write_half.write_all(&bytes).await.is_err() {
                            break;
                        }
                        let _ = write_half.flush().await;
                    }
                    Err(e) => tracing::warn!("Engine: failed to serialize response: {e}"),
                }
            }
        }
    }

    pub async fn serve(cfg: EngineConfig) -> Result<(), String> {
        // One model for the whole engine. Gate on free memory the same way the
        // per-workspace path does, so a constrained box runs graph-only instead
        // of OOM-crashing on the model load.
        let shared_engine = {
            let mut sys = sysinfo::System::new();
            sys.refresh_memory();
            if sys.available_memory() < 1_500_000_000 {
                tracing::warn!("Engine: <1.5 GB free — running graph-only (no shared model)");
                None
            } else {
                match VectorEngine::with_model(model_cache_dir(), cfg.embedding_model) {
                    Ok(e) => {
                        tracing::info!("Engine: shared embedding model loaded");
                        Some(Arc::new(e))
                    }
                    Err(e) => {
                        tracing::warn!("Engine: model load failed ({e}); running graph-only");
                        None
                    }
                }
            }
        };

        let socket_path = cfg.socket_path.clone();
        let seeds = cfg.seeds.clone();
        let engine = Arc::new(Engine {
            cfg,
            registry: Arc::new(Mutex::new(HashMap::new())),
            shared_engine,
        });

        // Pre-load any seed workspaces.
        for ws in seeds {
            let _ = get_or_load(&engine, ws).await;
        }

        let _ = std::fs::remove_file(&socket_path);
        if let Some(parent) = socket_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let listener = UnixListener::bind(&socket_path)
            .map_err(|e| format!("Failed to bind {}: {e}", socket_path.display()))?;
        tracing::info!("Engine listening on {}", socket_path.display());

        loop {
            let (stream, _) = listener
                .accept()
                .await
                .map_err(|e| format!("accept failed: {e}"))?;
            let engine = Arc::clone(&engine);
            tokio::spawn(handle_conn(engine, stream));
        }
    }

    pub async fn connect(socket_path: &std::path::Path, workspace: PathBuf) -> Result<(), String> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let stream = UnixStream::connect(socket_path).await.map_err(|e| {
            format!(
                "Failed to connect to engine at {}: {e}",
                socket_path.display()
            )
        })?;
        let (mut sock_read, mut sock_write) = stream.into_split();

        // Bind this connection to the workspace before relaying.
        let ws = workspace.canonicalize().unwrap_or(workspace);
        let attach = format!(
            "{}\n",
            serde_json::json!({ "cg_attach": { "workspace": ws.to_string_lossy() } })
        );
        if sock_write.write_all(attach.as_bytes()).await.is_err() {
            return Err("failed to send attach frame".to_string());
        }

        // Agent stdin -> engine.
        let up = tokio::spawn(async move {
            let mut stdin = tokio::io::stdin();
            let _ = tokio::io::copy(&mut stdin, &mut sock_write).await;
        });
        // Engine -> agent stdout, flushed per chunk.
        let down = tokio::spawn(async move {
            let mut stdout = tokio::io::stdout();
            let mut buf = vec![0u8; 16 * 1024];
            loop {
                match sock_read.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if stdout.write_all(&buf[..n]).await.is_err() {
                            break;
                        }
                        let _ = stdout.flush().await;
                    }
                }
            }
        });

        tokio::select! {
            _ = up => {}
            _ = down => {}
        }
        Ok(())
    }
}

#[cfg(unix)]
pub use imp::{connect, serve};

#[cfg(not(unix))]
pub async fn serve(_cfg: EngineConfig) -> Result<(), String> {
    Err("the socket engine is not yet supported on this platform".to_string())
}

#[cfg(not(unix))]
pub async fn connect(_socket_path: &std::path::Path, _workspace: PathBuf) -> Result<(), String> {
    Err("the socket engine is not yet supported on this platform".to_string())
}
