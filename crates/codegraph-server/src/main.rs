// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! CodeGraph Server Entry Point
//!
//! This is the main entry point for the CodeGraph Server.
//! It supports two modes:
//! - LSP mode (default): Serves Language Server Protocol over stdio for editors
//! - MCP mode (--mcp): Serves Model Context Protocol over stdio for AI clients

use clap::Parser;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

// glibc 2.31 compat: __libc_single_threaded was added in glibc 2.32 but ONNX
// Runtime references it. Provide a fallback for SLES 15 SP4 and similar.
// On newer glibc the real symbol shadows this at runtime.
#[cfg(target_os = "linux")]
#[no_mangle]
pub static __libc_single_threaded: u8 = 0;

#[derive(Parser)]
#[command(name = "codegraph-server")]
#[command(about = "CodeGraph Language Server with MCP support")]
#[command(version = codegraph_server::metadata::VERSION)]
struct Args {
    /// Print build info (git hash, build time, rustc version) and exit
    #[arg(long)]
    info: bool,
    /// Run in MCP (Model Context Protocol) mode for AI clients
    #[arg(long)]
    mcp: bool,

    /// Run in LSP mode over stdio (default, kept for compatibility)
    #[arg(long)]
    stdio: bool,

    /// Workspace directories to index (can be specified multiple times for multi-project)
    #[arg(long, short)]
    workspace: Vec<PathBuf>,

    /// Directories to exclude from indexing (can be specified multiple times)
    #[arg(long, short)]
    exclude: Vec<String>,

    /// Maximum number of files to index (default: 5000)
    #[arg(long, default_value = "5000")]
    max_files: usize,

    /// Embedding model: bge-small (384d, 512 ctx, fast — default),
    /// jina-code-v2 (768d, 8K ctx, 6× slower), or granite-97m (384d, 32K ctx,
    /// IBM ModernBERT multilingual, ~3× slower than bge-small but no
    /// truncation on long function bodies).
    #[arg(long, default_value = "bge-small")]
    embedding_model: String,

    /// Embed full function body instead of just name+signature (captured at parse time, minimal overhead)
    #[arg(long, default_value = "true")]
    full_body_embedding: bool,
}

/// Re-entrancy guard for the panic hook. A second panic during hook
/// execution skips the hook body and aborts directly to avoid deadlock.
static PANIC_DEPTH: AtomicUsize = AtomicUsize::new(0);

/// Install a panic hook + signal listeners so the process exits cleanly
/// instead of leaving the RocksDB `LOCK` held by a wedged or panicking
/// instance.
///
/// Strategy: panic / SIGINT / SIGTERM all funnel into `process::exit`.
/// At process exit the kernel releases all fcntl / LockFileEx grants,
/// so the next launch sees only the `LOCK` *file* (no live holder),
/// which `RocksDBBackend::open_with_stale_lock_recovery` clears. WAL
/// durability is per-write, so any in-flight batch is either fully
/// applied or fully discarded on next open — `exit` skipping `Drop` is
/// a safe tradeoff here.
fn install_crash_handlers() {
    std::panic::set_hook(Box::new(|info| {
        if PANIC_DEPTH.fetch_add(1, Ordering::SeqCst) > 0 {
            eprintln!("codegraph-server: re-entrant panic — aborting");
            std::process::abort();
        }
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "<unknown>".into());
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| info.payload().downcast_ref::<String>().map(String::as_str))
            .unwrap_or("<non-string panic payload>");
        // Use eprintln in addition to tracing — the panic may fire before
        // the subscriber is installed (e.g. during arg parsing).
        eprintln!(
            "codegraph-server: panic at {location} — {payload}\n\
             Exiting so RocksDB releases its LOCK. Restart will auto-recover \
             via stale-LOCK detection in ~/.codegraph/graph.db."
        );
        tracing::error!("panic at {location} — {payload}; exiting");
        std::process::exit(1);
    }));
}

fn spawn_signal_listeners() {
    tokio::spawn(async {
        if tokio::signal::ctrl_c().await.is_ok() {
            tracing::info!("Ctrl-C received — shutting down");
            std::process::exit(0);
        }
    });

    #[cfg(unix)]
    tokio::spawn(async {
        use tokio::signal::unix::{signal, SignalKind};
        if let Ok(mut term) = signal(SignalKind::terminate()) {
            if term.recv().await.is_some() {
                tracing::info!("SIGTERM received — shutting down");
                std::process::exit(0);
            }
        }
    });
}

#[tokio::main]
async fn main() {
    install_crash_handlers();

    let args = Args::parse();

    if args.info {
        codegraph_server::metadata::print_metadata();
        return;
    }

    // Initialize logging
    let log_filter = if args.mcp {
        // MCP mode: more verbose logging to stderr
        "codegraph_server=debug,codegraph=info"
    } else {
        "codegraph_server=info"
    };

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| log_filter.into()),
        )
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();

    // Install AFTER the runtime starts (tokio::spawn requires it). Runs
    // before any RocksDB open so the LOCK release path is wired up first.
    spawn_signal_listeners();

    if args.mcp {
        // MCP mode
        let workspaces = if args.workspace.is_empty() {
            vec![std::env::current_dir().expect("Failed to get current directory")]
        } else {
            args.workspace
        };

        let embedding_model = match args.embedding_model.as_str() {
            "jina-code-v2" => codegraph_memory::CodeGraphEmbeddingModel::JinaCodeV2,
            "granite-97m" | "granite" | "granite-97m-multilingual-r2" => {
                codegraph_memory::CodeGraphEmbeddingModel::Granite97mMultilingualR2
            }
            _ => codegraph_memory::CodeGraphEmbeddingModel::BgeSmall,
        };

        tracing::info!("Starting CodeGraph MCP server");
        tracing::info!("Workspaces: {:?}", workspaces);
        tracing::info!("Embedding model: {}", embedding_model.display_name());
        tracing::info!("Full-body embedding: {}", args.full_body_embedding);
        if !args.exclude.is_empty() {
            tracing::info!("Excluding: {:?}", args.exclude);
        }

        let mut server = codegraph_server::mcp::McpServer::new(
            workspaces,
            args.exclude,
            args.max_files,
            embedding_model,
            args.full_body_embedding,
        );
        if let Err(e) = server.run().await {
            tracing::error!("MCP server error: {}", e);
            std::process::exit(1);
        }
    } else {
        // LSP mode (default)
        use codegraph_server::CodeGraphBackend;
        use tower_lsp::{LspService, Server};

        tracing::info!("Starting CodeGraph LSP server");

        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();

        let (service, socket) = LspService::new(CodeGraphBackend::new);

        Server::new(stdin, stdout, socket).serve(service).await;
    }
}
