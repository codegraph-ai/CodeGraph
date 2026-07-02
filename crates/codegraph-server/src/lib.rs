// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! CodeGraph Server Library
//!
//! This crate implements the CodeGraph MCP/LSP server,
//! providing cross-language code intelligence through graph-based analysis.
//!
//! ## Transports
//!
//! The server supports two transports:
//! - **LSP** (default): Standard Language Server Protocol for IDE integration
//! - **MCP** (`--mcp` flag): Model Context Protocol for AI client integration

// glibc 2.31 compat (test builds): the production shim lives in main.rs
// for the binary target. `cargo test --lib` builds a separate test
// executable that doesn't include main.rs, so ONNX Runtime's reference
// to `__libc_single_threaded` (added in glibc 2.32) goes unresolved
// when linking tests on SLES 15-SP4. This duplicate is gated on
// `cfg(test)` so the binary target never sees two definitions.
#[cfg(all(target_os = "linux", test))]
#[no_mangle]
pub static __libc_single_threaded: u8 = 0;

pub mod ai_query;
pub mod backend;
pub mod branch_watcher;
pub mod cache;
pub mod crash_phase;
pub mod custom_requests;
pub mod daemon;
pub mod domain;
pub mod embed_queue;
pub mod error;
pub mod git_mining;
pub mod handlers;
pub mod index;
pub mod index_state;
pub mod indexer;
pub mod lsp_pro_hooks;
pub mod mcp;
pub mod memory;
pub mod metadata;
pub mod parser_registry;
pub mod runtime_deps;
pub mod telemetry;
pub mod watcher;

pub use backend::CodeGraphBackend;
pub use error::LspError;
pub use git_mining::{GitMiner, MiningConfig, MiningResult};
pub use memory::MemoryManager;
pub use parser_registry::ParserRegistry;

#[cfg(test)]
pub(crate) mod test_env {
    use std::sync::{Mutex, MutexGuard};

    // Environment variables are process-global, so every test in this binary
    // that mutates them must serialize through this one lock; per-module
    // locks cannot serialize against each other.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    pub(crate) fn lock() -> MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }
}
