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

    /// Scope the MCP tool surface to a named profile.
    ///
    /// `all` (default) exposes every tool (community + pro). Narrower profiles
    /// reduce the agent's prompt-context cost on chatty sessions:
    ///   - `core`     — search + symbol info + AI context (8 tools)
    ///   - `graph`    — callers/callees/deps/impact/traverse (16 tools)
    ///   - `memory`   — codegraph_memory_* only (7 tools)
    ///   - `security` — pro security tools only (empty on community)
    ///
    /// Also reads `CODEGRAPH_TOOL_PROFILE` env var when the flag is unset.
    /// MCP mode only — LSP mode ignores this.
    #[arg(long)]
    profile: Option<String>,

    /// Skip embedding generation — build the graph and serve structural
    /// tools only. The ONNX model is never loaded (faster startup, lower
    /// memory, ~10-50× faster indexing). Semantic search and similarity
    /// tools are unavailable. Ideal for CI / one-shot graph queries.
    #[arg(long)]
    graph_only: bool,

    /// One-shot mode: index the workspace, run a single tool, print its
    /// JSON result to stdout, and exit. No MCP stdio handshake. Pair with
    /// --tool-args for arguments and --graph-only for CI speed. Example:
    ///   codegraph-server --graph-only --run-tool codegraph_pr_context \
    ///     --tool-args '{"baseBranch":"main","format":"markdown"}'
    #[arg(long)]
    run_tool: Option<String>,

    /// JSON arguments for --run-tool (default: {}).
    #[arg(long, default_value = "{}")]
    tool_args: String,
}

/// Re-entrancy guard for the panic hook. A second panic during hook
/// execution skips the hook body and aborts directly to avoid deadlock.
static PANIC_DEPTH: AtomicUsize = AtomicUsize::new(0);

/// Best-effort: write a tiny crash breadcrumb to `~/.codegraph/` that the
/// VS Code extension reads on the next start to classify the crash cause.
/// Contains only an enum `class` + `site` bucket — never source, path
/// content, or the panic message text. Must never panic (runs inside the
/// panic hook), so every fallible step is swallowed.
fn write_crash_breadcrumb(kind: &str, class: &str, site: &str) {
    let home = match std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        Some(h) => h,
        None => return,
    };
    let dir = std::path::PathBuf::from(home).join(".codegraph");
    let _ = std::fs::create_dir_all(&dir);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let pid = std::process::id();
    // Every field value is a known ASCII enum — no JSON escaping required.
    let json = format!(
        "{{\"schema\":1,\"ts\":{ts},\"pid\":{pid},\"kind\":\"{kind}\",\"class\":\"{class}\",\"site\":\"{site}\"}}"
    );
    let _ = std::fs::write(dir.join(format!("last-crash.{pid}.json")), json);
}

/// Map a panic message + location into a coarse (class, site) pair. Both
/// are fixed enums; the raw message is never persisted or transmitted.
fn classify_panic(payload: &str, location: &str) -> (&'static str, &'static str) {
    let p = payload.to_ascii_lowercase();
    let class = if p.contains("lock") || p.contains("rocksdb") {
        "rocksdb_lock"
    } else if p.contains("utf-8")
        || p.contains("utf8")
        || p.contains("byte index")
        || p.contains("char boundary")
    {
        "utf8_parse"
    } else if p.contains("poison") {
        "mutex_poison"
    } else if p.contains("memory allocation")
        || p.contains("capacity overflow")
        || p.contains("cannot allocate")
    {
        "oom"
    } else if p.contains("out of bounds") || p.contains("out of range") || p.contains("slice index")
    {
        "bounds"
    } else {
        "panic_other"
    };
    let l = location.to_ascii_lowercase();
    let site = if l.contains("codegraph-memory") || l.contains("codegraph_memory") {
        "memory"
    } else if l.contains("mcp") {
        "mcp"
    } else if l.contains("rocks") || l.contains("backend") || l.contains("storage") {
        "storage"
    } else if l.contains("parser") || l.contains("tree-sitter") || l.contains("tree_sitter") {
        "parser"
    } else if l.contains("codegraph-server") {
        "server"
    } else {
        "other"
    };
    (class, site)
}

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
        // Drop a sanitized breadcrumb (enum class only) so the extension
        // can report WHY we crashed, not just that we did.
        let (class, site) = classify_panic(payload, &location);
        write_crash_breadcrumb("panic", class, site);
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
            codegraph_server::crash_phase::clear();
            std::process::exit(0);
        }
    });

    #[cfg(unix)]
    tokio::spawn(async {
        use tokio::signal::unix::{signal, SignalKind};
        if let Ok(mut term) = signal(SignalKind::terminate()) {
            if term.recv().await.is_some() {
                tracing::info!("SIGTERM received — shutting down");
                codegraph_server::crash_phase::clear();
                std::process::exit(0);
            }
        }
    });
}

#[tokio::main]
async fn main() {
    install_crash_handlers();
    codegraph_server::crash_phase::mark("startup");

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

    // One-shot tool mode: index, run a single tool, print JSON, exit.
    if let Some(tool_name) = args.run_tool.clone() {
        let workspaces = if args.workspace.is_empty() {
            vec![std::env::current_dir().expect("Failed to get current directory")]
        } else {
            args.workspace.clone()
        };
        let embedding_model = match args.embedding_model.as_str() {
            "jina-code-v2" => codegraph_memory::CodeGraphEmbeddingModel::JinaCodeV2,
            "granite-97m" | "granite" | "granite-97m-multilingual-r2" => {
                codegraph_memory::CodeGraphEmbeddingModel::Granite97mMultilingualR2
            }
            _ => codegraph_memory::CodeGraphEmbeddingModel::BgeSmall,
        };
        let tool_args: serde_json::Value = serde_json::from_str(&args.tool_args)
            .unwrap_or_else(|e| {
                eprintln!("Invalid --tool-args JSON: {e}");
                std::process::exit(2);
            });

        let mut server = codegraph_server::mcp::McpServer::new(
            workspaces,
            args.exclude.clone(),
            args.max_files,
            embedding_model,
            args.full_body_embedding,
        )
        .with_graph_only(args.graph_only);

        match server.run_single_tool(&tool_name, Some(tool_args)).await {
            Ok(result) => {
                // If the tool returned a markdown field, print it raw —
                // CI pipes it straight into a PR comment. Otherwise print JSON.
                if let Some(md) = result.get("markdown").and_then(|v| v.as_str()) {
                    println!("{md}");
                } else {
                    println!("{}", serde_json::to_string_pretty(&result).unwrap_or_default());
                }
            }
            Err(e) => {
                eprintln!("Tool '{tool_name}' failed: {e}");
                std::process::exit(1);
            }
        }
        return;
    }

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

        // Resolve tool profile: --profile takes precedence over env var,
        // both fall through to All when unset/unknown.
        let profile_str = args
            .profile
            .clone()
            .or_else(|| std::env::var("CODEGRAPH_TOOL_PROFILE").ok())
            .unwrap_or_default();
        let tool_profile = codegraph_server::mcp::tools::ToolProfile::from_str_or_all(&profile_str);
        if !profile_str.is_empty() {
            tracing::info!("Tool profile: {:?} (from '{}')", tool_profile, profile_str);
        }

        let mut server = codegraph_server::mcp::McpServer::new(
            workspaces,
            args.exclude,
            args.max_files,
            embedding_model,
            args.full_body_embedding,
        )
        .with_tool_profile(tool_profile)
        .with_graph_only(args.graph_only);
        codegraph_server::crash_phase::mark("serving");
        if let Err(e) = server.run().await {
            tracing::error!("MCP server error: {}", e);
            std::process::exit(1);
        }
        codegraph_server::crash_phase::clear();
    } else {
        // LSP mode (default)
        use codegraph_server::CodeGraphBackend;
        use tower_lsp::{LspService, Server};

        tracing::info!("Starting CodeGraph LSP server");

        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();

        let (service, socket) = LspService::new(CodeGraphBackend::new);

        codegraph_server::crash_phase::mark("serving");
        Server::new(stdin, stdout, socket).serve(service).await;
        codegraph_server::crash_phase::clear();
    }
}

#[cfg(test)]
mod crash_breadcrumb_tests {
    use super::{classify_panic, write_crash_breadcrumb};

    #[test]
    fn classify_panic_maps_message_class() {
        assert_eq!(
            classify_panic("Result::unwrap() on Err: RocksDB LOCK held", "x").0,
            "rocksdb_lock"
        );
        assert_eq!(
            classify_panic("byte index 7 is not a char boundary", "x").0,
            "utf8_parse"
        );
        assert_eq!(classify_panic("PoisonError { .. }", "x").0, "mutex_poison");
        assert_eq!(
            classify_panic("memory allocation of 9999 bytes failed", "x").0,
            "oom"
        );
        assert_eq!(
            classify_panic("index out of bounds: the len is 3", "x").0,
            "bounds"
        );
        assert_eq!(classify_panic("something unexpected", "x").0, "panic_other");
    }

    #[test]
    fn classify_panic_maps_site() {
        assert_eq!(
            classify_panic("x", "crates/codegraph-memory/src/embed.rs:1").1,
            "memory"
        );
        assert_eq!(
            classify_panic("x", "crates/codegraph-server/src/mcp/server.rs:1").1,
            "mcp"
        );
        assert_eq!(
            classify_panic("x", "crates/codegraph-server/src/main.rs:1").1,
            "server"
        );
    }

    #[test]
    fn breadcrumb_roundtrip_writes_parseable_json() {
        // Isolate HOME to a temp dir so we never touch the real ~/.codegraph.
        let tmp = std::env::temp_dir().join(format!("cg-crash-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let prev_home = std::env::var_os("HOME");
        let prev_up = std::env::var_os("USERPROFILE");
        std::env::set_var("HOME", &tmp);
        std::env::set_var("USERPROFILE", &tmp);

        write_crash_breadcrumb("panic", "rocksdb_lock", "storage");

        let dir = tmp.join(".codegraph");
        let entry = std::fs::read_dir(&dir)
            .expect("breadcrumb dir exists")
            .filter_map(|e| e.ok())
            .find(|e| e.file_name().to_string_lossy().starts_with("last-crash."))
            .expect("a breadcrumb file was written");
        let content = std::fs::read_to_string(entry.path()).expect("readable");
        let v: serde_json::Value = serde_json::from_str(&content).expect("valid JSON");
        assert_eq!(v["kind"], "panic");
        assert_eq!(v["class"], "rocksdb_lock");
        assert_eq!(v["site"], "storage");
        assert_eq!(v["schema"], 1);

        match prev_home {
            Some(h) => std::env::set_var("HOME", h),
            None => std::env::remove_var("HOME"),
        }
        match prev_up {
            Some(h) => std::env::set_var("USERPROFILE", h),
            None => std::env::remove_var("USERPROFILE"),
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
