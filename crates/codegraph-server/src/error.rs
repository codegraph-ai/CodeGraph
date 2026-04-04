// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Error types for the CodeGraph server.

use codegraph_parser_api::ParserError;
use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur in the LSP server.
#[derive(Debug, Error)]
pub enum LspError {
    #[error("Symbol not found at position")]
    SymbolNotFound,

    #[error("File not indexed: {0}")]
    FileNotIndexed(PathBuf),

    #[error("Parser error: {0}")]
    Parser(#[from] ParserError),

    #[error("Graph error: {0}")]
    Graph(String),

    #[error("Invalid URI: {0}")]
    InvalidUri(String),

    #[error("Unsupported language: {0}")]
    UnsupportedLanguage(String),

    #[error("Cache error: {0}")]
    Cache(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Node not found: {0}")]
    NodeNotFound(String),
}

impl From<LspError> for tower_lsp::jsonrpc::Error {
    fn from(err: LspError) -> Self {
        let code = match &err {
            LspError::SymbolNotFound => tower_lsp::jsonrpc::ErrorCode::InvalidParams,
            LspError::FileNotIndexed(_) => tower_lsp::jsonrpc::ErrorCode::InvalidParams,
            LspError::InvalidUri(_) => tower_lsp::jsonrpc::ErrorCode::InvalidParams,
            LspError::UnsupportedLanguage(_) => tower_lsp::jsonrpc::ErrorCode::InvalidParams,
            LspError::NodeNotFound(_) => tower_lsp::jsonrpc::ErrorCode::InvalidParams,
            _ => tower_lsp::jsonrpc::ErrorCode::InternalError,
        };

        tower_lsp::jsonrpc::Error {
            code,
            message: err.to_string().into(),
            data: None,
        }
    }
}

/// Result type alias for LSP operations.
pub type LspResult<T> = Result<T, LspError>;

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::jsonrpc::ErrorCode;

    #[test]
    fn test_lsp_error_display_symbol_not_found() {
        let err = LspError::SymbolNotFound;
        assert_eq!(err.to_string(), "Symbol not found at position");
    }

    #[test]
    fn test_lsp_error_display_file_not_indexed() {
        let path = PathBuf::from("/test/file.rs");
        let err = LspError::FileNotIndexed(path.clone());
        assert_eq!(
            err.to_string(),
            format!("File not indexed: {}", path.display())
        );
    }

    #[test]
    fn test_lsp_error_display_graph_error() {
        let err = LspError::Graph("test graph error".to_string());
        assert_eq!(err.to_string(), "Graph error: test graph error");
    }

    #[test]
    fn test_lsp_error_display_invalid_uri() {
        let err = LspError::InvalidUri("not-a-uri".to_string());
        assert_eq!(err.to_string(), "Invalid URI: not-a-uri");
    }

    #[test]
    fn test_lsp_error_display_unsupported_language() {
        let err = LspError::UnsupportedLanguage("cobol".to_string());
        assert_eq!(err.to_string(), "Unsupported language: cobol");
    }

    #[test]
    fn test_lsp_error_display_cache_error() {
        let err = LspError::Cache("cache miss".to_string());
        assert_eq!(err.to_string(), "Cache error: cache miss");
    }

    #[test]
    fn test_lsp_error_display_node_not_found() {
        let err = LspError::NodeNotFound("node_123".to_string());
        assert_eq!(err.to_string(), "Node not found: node_123");
    }

    #[test]
    fn test_jsonrpc_error_conversion_symbol_not_found() {
        let err: tower_lsp::jsonrpc::Error = LspError::SymbolNotFound.into();
        assert_eq!(err.code, ErrorCode::InvalidParams);
        assert!(err.message.contains("Symbol not found"));
    }

    #[test]
    fn test_jsonrpc_error_conversion_file_not_indexed() {
        let err: tower_lsp::jsonrpc::Error =
            LspError::FileNotIndexed(PathBuf::from("/test.rs")).into();
        assert_eq!(err.code, ErrorCode::InvalidParams);
        assert!(err.message.contains("File not indexed"));
    }

    #[test]
    fn test_jsonrpc_error_conversion_invalid_uri() {
        let err: tower_lsp::jsonrpc::Error = LspError::InvalidUri("bad-uri".to_string()).into();
        assert_eq!(err.code, ErrorCode::InvalidParams);
        assert!(err.message.contains("Invalid URI"));
    }

    #[test]
    fn test_jsonrpc_error_conversion_unsupported_language() {
        let err: tower_lsp::jsonrpc::Error =
            LspError::UnsupportedLanguage("brainfuck".to_string()).into();
        assert_eq!(err.code, ErrorCode::InvalidParams);
    }

    #[test]
    fn test_jsonrpc_error_conversion_node_not_found() {
        let err: tower_lsp::jsonrpc::Error = LspError::NodeNotFound("n1".to_string()).into();
        assert_eq!(err.code, ErrorCode::InvalidParams);
    }

    #[test]
    fn test_jsonrpc_error_conversion_graph_error_internal() {
        let err: tower_lsp::jsonrpc::Error = LspError::Graph("internal".to_string()).into();
        assert_eq!(err.code, ErrorCode::InternalError);
    }

    #[test]
    fn test_jsonrpc_error_conversion_cache_error_internal() {
        let err: tower_lsp::jsonrpc::Error = LspError::Cache("internal".to_string()).into();
        assert_eq!(err.code, ErrorCode::InternalError);
    }

    #[test]
    fn test_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let lsp_err: LspError = io_err.into();

        match lsp_err {
            LspError::Io(_) => {}
            _ => panic!("Expected LspError::Io"),
        }
    }

    #[test]
    fn test_io_error_jsonrpc_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let lsp_err: LspError = io_err.into();
        let jsonrpc_err: tower_lsp::jsonrpc::Error = lsp_err.into();

        assert_eq!(jsonrpc_err.code, ErrorCode::InternalError);
    }

    #[test]
    fn test_lsp_result_type_ok() {
        fn returns_ok() -> LspResult<i32> {
            Ok(42)
        }
        let result = returns_ok();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn test_lsp_result_type_err() {
        let result: LspResult<i32> = Err(LspError::SymbolNotFound);
        assert!(result.is_err());
    }
}
