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
pub mod custom_requests;
pub mod domain;
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
pub mod watcher;

pub use backend::CodeGraphBackend;
pub use error::LspError;
pub use git_mining::{GitMiner, MiningConfig, MiningResult};
pub use memory::MemoryManager;
pub use parser_registry::ParserRegistry;
