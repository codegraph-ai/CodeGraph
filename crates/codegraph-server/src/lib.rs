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

pub mod ai_query;
pub mod backend;
pub mod lsp_pro_hooks;
pub mod branch_watcher;
pub mod cache;
pub mod custom_requests;
pub mod domain;
pub mod error;
pub mod git_mining;
pub mod handlers;
pub mod index;
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
