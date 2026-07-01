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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_provider_lists_no_tools() {
        // The community server injects zero premium tools; the whole point
        // of the noop provider is that `tools()` is empty so the MCP tool
        // surface carries only the community tools.
        assert!(NoopProProvider.tools().is_empty());
    }

    #[test]
    fn noop_provider_reports_community_edition() {
        // `edition()` is left unoverridden, so it falls through to the
        // trait's default which reports "community" for capability
        // reporting. Pin the exact string the server advertises.
        assert_eq!(NoopProProvider.edition(), "community");
    }

    #[test]
    fn pro_tool_info_carries_its_three_fields_through_clone() {
        // ProToolInfo is populated by the pro server and read by the
        // community server's tool-listing code, so its derived Clone must
        // preserve all three fields verbatim (name/description/schema).
        let info = ProToolInfo {
            name: "codegraph/premium".to_string(),
            description: "premium tool".to_string(),
            schema: serde_json::json!({ "type": "object" }),
        };
        let cloned = info.clone();
        assert_eq!(cloned.name, "codegraph/premium");
        assert_eq!(cloned.description, "premium tool");
        assert_eq!(cloned.schema, serde_json::json!({ "type": "object" }));
    }

    #[test]
    fn pro_tool_info_debug_includes_the_name() {
        // The derived Debug is used in tracing/diagnostics; confirm it
        // renders the tool name rather than an opaque struct address.
        let info = ProToolInfo {
            name: "codegraph/premium".to_string(),
            description: String::new(),
            schema: Value::Null,
        };
        assert!(format!("{info:?}").contains("codegraph/premium"));
    }
}
