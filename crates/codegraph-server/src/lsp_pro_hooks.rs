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

/// Boxed future returned by pro command handlers.
pub type ProCommandFuture = Pin<Box<dyn Future<Output = Result<Option<Value>, String>> + Send>>;

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
    ) -> Option<ProCommandFuture>;

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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ctx() -> ProCommandContext {
        let graph = Arc::new(tokio::sync::RwLock::new(
            codegraph::CodeGraph::in_memory().expect("in-memory graph"),
        ));
        let query_engine = Arc::new(crate::ai_query::QueryEngine::new(graph.clone()));
        let memory_manager = Arc::new(crate::memory::MemoryManager::new(None));
        ProCommandContext {
            graph,
            query_engine,
            memory_manager,
            workspace_folders: vec![std::path::PathBuf::from("/tmp/ws")],
        }
    }

    #[test]
    fn test_noop_commands_empty() {
        assert!(NoopProCommandProvider.commands().is_empty());
    }

    #[test]
    fn test_noop_default_edition_is_community() {
        // edition() uses the trait's default impl.
        assert_eq!(NoopProCommandProvider.edition(), "community");
    }

    #[test]
    fn test_noop_default_command_prefix() {
        // command_prefix() uses the trait's default impl.
        assert_eq!(NoopProCommandProvider.command_prefix(), "codegraph");
    }

    #[test]
    fn test_noop_handle_command_returns_none() {
        let ctx = make_ctx();
        let result = NoopProCommandProvider.handle_command("codegraph.anything", Value::Null, ctx);
        assert!(result.is_none());
    }

    #[test]
    fn test_context_clone_preserves_fields() {
        let ctx = make_ctx();
        let cloned = ctx.clone();
        // Arc fields share the same allocation after clone.
        assert!(Arc::ptr_eq(&ctx.graph, &cloned.graph));
        assert!(Arc::ptr_eq(&ctx.query_engine, &cloned.query_engine));
        assert!(Arc::ptr_eq(&ctx.memory_manager, &cloned.memory_manager));
        assert_eq!(ctx.workspace_folders, cloned.workspace_folders);
    }
}
