// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Extension point for CodeGraph Pro LSP commands.
//!
//! The community server uses `NoopProCommandProvider` (no premium commands).
//! The pro server injects a real implementation with additional LSP commands.

use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Shared state passed to pro command handlers.
#[derive(Clone)]
pub struct ProCommandContext {
    pub graph: Arc<tokio::sync::RwLock<codegraph::CodeGraph>>,
    pub query_engine: Arc<crate::ai_query::QueryEngine>,
    pub memory_manager: Arc<crate::memory::MemoryManager>,
    pub workspace_folders: Vec<std::path::PathBuf>,
}

/// Trait for injecting pro commands into the LSP workspace/executeCommand handler.
pub trait ProCommandProvider: Send + Sync + 'static {
    /// List additional command names provided by this extension.
    fn commands(&self) -> Vec<String>;

    /// Handle a command. Returns None if the command is not recognized.
    /// Takes ownership of ctx (Clone) so the future can be 'static.
    fn handle_command(
        &self,
        name: &str,
        args: Value,
        ctx: ProCommandContext,
    ) -> Option<Pin<Box<dyn Future<Output = Result<Option<Value>, String>> + Send>>>;

    /// Return the edition name.
    fn edition(&self) -> &str {
        "community"
    }

    /// Return the command namespace prefix (e.g., "codegraph" or "stellarion").
    /// All LSP commands will use this prefix: "{prefix}.getDependencyGraph", etc.
    /// Default: "codegraph"
    fn command_prefix(&self) -> &str {
        "codegraph"
    }
}

/// Default implementation — no premium commands.
pub struct NoopProCommandProvider;

impl ProCommandProvider for NoopProCommandProvider {
    fn commands(&self) -> Vec<String> {
        vec![]
    }

    fn handle_command(
        &self,
        _name: &str,
        _args: Value,
        _ctx: ProCommandContext,
    ) -> Option<Pin<Box<dyn Future<Output = Result<Option<Value>, String>> + Send>>> {
        None
    }
}
