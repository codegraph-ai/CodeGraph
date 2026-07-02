// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AI Context Provider — thin LSP adapter over domain::ai_context.

use crate::backend::CodeGraphBackend;
use crate::domain::ai_context::AiContextResult;
use serde::{Deserialize, Serialize};
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{Position, Range, Url};

// ==========================================
// AI Context Request Types (public — used externally)
// ==========================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AIContextParams {
    pub uri: String,
    /// Line number (0-indexed) - used for MCP compatibility
    #[serde(default)]
    pub line: Option<u32>,
    /// Position for LSP compatibility
    #[serde(default)]
    pub position: Option<Position>,
    /// Context intent: "explain", "modify", "debug", "test"
    #[serde(alias = "context_type")]
    pub intent: Option<String>,
    pub max_tokens: Option<usize>,
}

/// Re-export domain result type as the LSP response type.
/// Both use the same serialization shape (camelCase JSON).
pub type AIContextResponse = AiContextResult;

/// Location info used by metrics and other handlers.
/// Uses tower_lsp Range type for compatibility with LSP location helpers.
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LocationInfo {
    pub uri: String,
    pub range: Range,
}

// ==========================================
// LSP Handler (thin adapter)
// ==========================================

impl CodeGraphBackend {
    pub async fn handle_get_ai_context(
        &self,
        params: AIContextParams,
    ) -> Result<AIContextResponse> {
        let uri = Url::parse(&params.uri)
            .map_err(|_| tower_lsp::jsonrpc::Error::invalid_params("Invalid URI"))?;

        let path = uri
            .to_file_path()
            .map_err(|_| tower_lsp::jsonrpc::Error::invalid_params("Invalid file path"))?;

        let line = if let Some(l) = params.line {
            l
        } else if let Some(pos) = params.position {
            pos.line
        } else {
            0
        };

        let intent = params.intent.as_deref().unwrap_or("explain");
        let max_tokens = params.max_tokens.unwrap_or(4000);
        let path_str = path.to_string_lossy().to_string();

        let graph = self.graph.read().await;

        crate::domain::ai_context::get_ai_context(&graph, &path_str, line, intent, max_tokens)
            .ok_or_else(|| {
                tower_lsp::jsonrpc::Error::invalid_params(
                    "No symbols found in file. Try indexing the workspace first.",
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::ai_context::{detect_layer, generate_usage_description};

    #[test]
    fn test_detect_layer_controllers() {
        assert_eq!(
            detect_layer("/src/controllers/user.ts"),
            Some("controller".to_string())
        );
        assert_eq!(
            detect_layer("/src/api/users.ts"),
            Some("controller".to_string())
        );
        assert_eq!(
            detect_layer("/app/routes/index.ts"),
            Some("controller".to_string())
        );
    }

    #[test]
    fn test_detect_layer_services() {
        assert_eq!(
            detect_layer("/src/services/auth.ts"),
            Some("service".to_string())
        );
        assert_eq!(
            detect_layer("/src/usecases/login.ts"),
            Some("service".to_string())
        );
    }

    #[test]
    fn test_detect_layer_domain() {
        assert_eq!(
            detect_layer("/src/models/user.ts"),
            Some("domain".to_string())
        );
        assert_eq!(
            detect_layer("/src/entities/order.ts"),
            Some("domain".to_string())
        );
        assert_eq!(
            detect_layer("/src/domain/product.ts"),
            Some("domain".to_string())
        );
    }

    #[test]
    fn test_detect_layer_repository() {
        assert_eq!(
            detect_layer("/src/repositories/user_repo.ts"),
            Some("repository".to_string())
        );
        assert_eq!(
            detect_layer("/src/repos/order.ts"),
            Some("repository".to_string())
        );
    }

    #[test]
    fn test_detect_layer_infrastructure() {
        assert_eq!(
            detect_layer("/src/database/connection.ts"),
            Some("persistence".to_string())
        );
        assert_eq!(
            detect_layer("/src/adapters/redis.ts"),
            Some("infrastructure".to_string())
        );
    }

    #[test]
    fn test_detect_layer_utility() {
        assert_eq!(
            detect_layer("/src/utils/helpers.ts"),
            Some("utility".to_string())
        );
        assert_eq!(detect_layer("/lib/format.ts"), Some("utility".to_string()));
    }

    #[test]
    fn test_detect_layer_tests() {
        assert_eq!(
            detect_layer("/src/__tests__/user.test.ts"),
            Some("test".to_string())
        );
        assert_eq!(
            detect_layer("/tests/integration/api.ts"),
            Some("test".to_string())
        );
    }

    #[test]
    fn test_detect_layer_by_filename() {
        assert_eq!(
            detect_layer("/src/user_controller.ts"),
            Some("controller".to_string())
        );
        assert_eq!(
            detect_layer("/src/auth_service.ts"),
            Some("service".to_string())
        );
        assert_eq!(
            detect_layer("/src/user_repository.ts"),
            Some("repository".to_string())
        );
    }

    #[test]
    fn test_detect_layer_unknown() {
        assert_eq!(detect_layer("/src/main.ts"), None);
        assert_eq!(detect_layer("/app.ts"), None);
    }

    #[test]
    fn test_generate_usage_description_basic() {
        let desc =
            generate_usage_description("process_order", "validate_data", "validate_data(input)");
        assert!(desc.contains("`process_order`"));
        assert!(desc.contains("`validate_data`"));
    }

    #[test]
    fn test_generate_usage_description_async() {
        let desc = generate_usage_description("handler", "fetch_user", "await fetch_user(id)");
        assert!(desc.contains("(async)"));
    }

    #[test]
    fn test_generate_usage_description_error_handling() {
        let desc = generate_usage_description(
            "process",
            "parse_config",
            "try { parse_config() } catch(e) { }",
        );
        assert!(desc.contains("error handling"));
    }

    #[test]
    fn test_generate_usage_description_conditional() {
        let desc = generate_usage_description("run", "check", "if (check(x)) { do_thing() }");
        assert!(desc.contains("conditionally"));
    }

    #[test]
    fn test_generate_usage_description_empty_caller() {
        let desc = generate_usage_description("", "my_function", "my_function()");
        assert!(desc.contains("Usage of `my_function`"));
    }

    mod handler {
        use super::super::{AIContextParams, CodeGraphBackend};
        use crate::ai_query::QueryEngine;
        use codegraph::CodeGraph;
        use std::sync::Arc;
        use tokio::sync::RwLock;
        use tower_lsp::lsp_types::Position;

        /// Create a test backend with an in-memory graph and empty symbol index.
        async fn create_test_backend() -> CodeGraphBackend {
            let graph = Arc::new(RwLock::new(
                CodeGraph::in_memory().expect("Failed to create in-memory graph"),
            ));
            let query_engine = Arc::new(QueryEngine::new(Arc::clone(&graph)));
            CodeGraphBackend::new_for_test(graph, query_engine)
        }

        fn params(uri: &str) -> AIContextParams {
            AIContextParams {
                uri: uri.to_string(),
                line: None,
                position: None,
                intent: None,
                max_tokens: None,
            }
        }

        #[tokio::test]
        async fn test_handle_get_ai_context_unparseable_uri() {
            let backend = create_test_backend().await;
            // A string that is not a parseable URL fails at Url::parse.
            let err = backend
                .handle_get_ai_context(params("not a valid uri"))
                .await
                .expect_err("unparseable URI should fail");
            assert_eq!(err.code, tower_lsp::jsonrpc::ErrorCode::InvalidParams);
            assert!(err.message.contains("Invalid URI"));
        }

        #[tokio::test]
        async fn test_handle_get_ai_context_non_file_scheme() {
            let backend = create_test_backend().await;
            // A well-formed http:// URL parses but has no file path.
            let err = backend
                .handle_get_ai_context(params("http://example.com/foo.rs"))
                .await
                .expect_err("non-file URI should fail");
            assert_eq!(err.code, tower_lsp::jsonrpc::ErrorCode::InvalidParams);
            assert!(err.message.contains("Invalid file path"));
        }

        #[tokio::test]
        async fn test_handle_get_ai_context_no_symbols() {
            let backend = create_test_backend().await;
            // Valid file URI, but the empty graph resolves no nearest node.
            let err = backend
                .handle_get_ai_context(params("file:///tmp/does_not_exist.rs"))
                .await
                .expect_err("empty graph should yield no symbols");
            assert_eq!(err.code, tower_lsp::jsonrpc::ErrorCode::InvalidParams);
            assert!(err.message.contains("No symbols found"));
        }

        #[tokio::test]
        async fn test_handle_get_ai_context_position_line_fallback() {
            let backend = create_test_backend().await;
            // With `line` unset, the handler falls back to `position.line`; the
            // empty graph still yields the no-symbols error, exercising that branch.
            let mut p = params("file:///tmp/does_not_exist.rs");
            p.position = Some(Position {
                line: 42,
                character: 0,
            });
            let err = backend
                .handle_get_ai_context(p)
                .await
                .expect_err("empty graph should yield no symbols");
            assert_eq!(err.code, tower_lsp::jsonrpc::ErrorCode::InvalidParams);
            assert!(err.message.contains("No symbols found"));
        }
    }
}
