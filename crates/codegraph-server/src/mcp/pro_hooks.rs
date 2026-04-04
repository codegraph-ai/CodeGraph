// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Extension point for CodeGraph Pro features.
//!
//! The community server uses `NoopProProvider` (no premium tools).
//! The pro server injects a real implementation with additional tools.

use serde_json::Value;
use std::future::Future;
use std::pin::Pin;

/// Information about a tool provided by the pro extension.
#[derive(Debug, Clone)]
pub struct ProToolInfo {
    pub name: String,
    pub description: String,
    pub schema: Value,
}

/// Trait for injecting pro tools into the MCP server.
pub trait ProToolProvider: Send + Sync {
    /// List additional tools provided by this extension.
    fn tools(&self) -> Vec<ProToolInfo>;

    /// Handle a tool call. Returns None if the tool is not recognized by this provider.
    fn handle_tool<'a>(
        &'a self,
        name: &'a str,
        args: Value,
        backend: &'a super::server::McpBackend,
    ) -> Option<Pin<Box<dyn Future<Output = Result<Value, String>> + Send + 'a>>>;

    /// Return the edition name for capability reporting.
    fn edition(&self) -> &str {
        "community"
    }
}

/// Default implementation — no premium tools.
pub struct NoopProProvider;

impl ProToolProvider for NoopProProvider {
    fn tools(&self) -> Vec<ProToolInfo> {
        vec![]
    }

    fn handle_tool<'a>(
        &'a self,
        _name: &'a str,
        _args: Value,
        _backend: &'a super::server::McpBackend,
    ) -> Option<Pin<Box<dyn Future<Output = Result<Value, String>> + Send + 'a>>> {
        None
    }
}
