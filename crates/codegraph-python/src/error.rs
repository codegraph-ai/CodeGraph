// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use std::io;
use std::path::PathBuf;
use thiserror::Error;

/// Result type alias for parser operations
pub type Result<T> = std::result::Result<T, ParseError>;

/// Errors that can occur during Python parsing
#[derive(Error, Debug)]
pub enum ParseError {
    /// I/O error reading a file
    #[error("Failed to read file {path}: {source}")]
    IoError { path: PathBuf, source: io::Error },

    /// File exceeds maximum size limit
    #[error(
        "File {path} exceeds maximum size limit of {max_size} bytes (actual: {actual_size} bytes)"
    )]
    FileTooLarge {
        path: PathBuf,
        max_size: usize,
        actual_size: usize,
    },

    /// Python syntax error
    #[error("Syntax error in {file} at line {line}, column {column}: {message}")]
    SyntaxError {
        file: String,
        line: usize,
        column: usize,
        message: String,
    },

    /// Error from graph database operations
    #[error("Graph operation failed: {0}")]
    GraphError(String),

    /// Invalid parser configuration
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    /// Unsupported Python language feature
    #[error("Unsupported Python feature in {file}: {feature}")]
    UnsupportedFeature { file: String, feature: String },
}

impl ParseError {
    /// Create an IoError from a path and io::Error
    pub fn io_error(path: impl Into<PathBuf>, source: io::Error) -> Self {
        ParseError::IoError {
            path: path.into(),
            source,
        }
    }

    /// Create a FileTooLarge error
    pub fn file_too_large(path: impl Into<PathBuf>, max_size: usize, actual_size: usize) -> Self {
        ParseError::FileTooLarge {
            path: path.into(),
            max_size,
            actual_size,
        }
    }

    /// Create a SyntaxError
    pub fn syntax_error(
        file: impl Into<String>,
        line: usize,
        column: usize,
        message: impl Into<String>,
    ) -> Self {
        ParseError::SyntaxError {
            file: file.into(),
            line,
            column,
            message: message.into(),
        }
    }

    /// Create a GraphError
    pub fn graph_error(message: impl Into<String>) -> Self {
        ParseError::GraphError(message.into())
    }

    /// Create an InvalidConfig error
    pub fn invalid_config(message: impl Into<String>) -> Self {
        ParseError::InvalidConfig(message.into())
    }

    /// Create an UnsupportedFeature error
    pub fn unsupported_feature(file: impl Into<String>, feature: impl Into<String>) -> Self {
        ParseError::UnsupportedFeature {
            file: file.into(),
            feature: feature.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_error_constructor_fills_fields_and_accepts_str_path() {
        let src = io::Error::new(io::ErrorKind::PermissionDenied, "denied");
        let err = ParseError::io_error("/tmp/foo.py", src);
        match err {
            ParseError::IoError { path, source } => {
                assert_eq!(path, PathBuf::from("/tmp/foo.py"));
                assert_eq!(source.kind(), io::ErrorKind::PermissionDenied);
            }
            other => panic!("expected IoError, got {other:?}"),
        }
    }

    #[test]
    fn io_error_display_includes_path_and_source() {
        let src = io::Error::new(io::ErrorKind::NotFound, "missing");
        let err = ParseError::io_error(PathBuf::from("a/b.py"), src);
        assert_eq!(err.to_string(), "Failed to read file a/b.py: missing");
    }

    #[test]
    fn file_too_large_constructor_and_display() {
        let err = ParseError::file_too_large("big.py", 1000, 4096);
        match &err {
            ParseError::FileTooLarge {
                path,
                max_size,
                actual_size,
            } => {
                assert_eq!(path, &PathBuf::from("big.py"));
                assert_eq!(*max_size, 1000);
                assert_eq!(*actual_size, 4096);
            }
            other => panic!("expected FileTooLarge, got {other:?}"),
        }
        assert_eq!(
            err.to_string(),
            "File big.py exceeds maximum size limit of 1000 bytes (actual: 4096 bytes)"
        );
    }

    #[test]
    fn syntax_error_constructor_and_display() {
        let err = ParseError::syntax_error("mod.py", 12, 5, "unexpected token");
        match &err {
            ParseError::SyntaxError {
                file,
                line,
                column,
                message,
            } => {
                assert_eq!(file, "mod.py");
                assert_eq!(*line, 12);
                assert_eq!(*column, 5);
                assert_eq!(message, "unexpected token");
            }
            other => panic!("expected SyntaxError, got {other:?}"),
        }
        assert_eq!(
            err.to_string(),
            "Syntax error in mod.py at line 12, column 5: unexpected token"
        );
    }

    #[test]
    fn graph_error_constructor_and_display() {
        let err = ParseError::graph_error("node insert failed");
        assert!(matches!(err, ParseError::GraphError(ref m) if m == "node insert failed"));
        assert_eq!(
            err.to_string(),
            "Graph operation failed: node insert failed"
        );
    }

    #[test]
    fn invalid_config_constructor_and_display() {
        let err = ParseError::invalid_config("bad max_size");
        assert!(matches!(err, ParseError::InvalidConfig(ref m) if m == "bad max_size"));
        assert_eq!(err.to_string(), "Invalid configuration: bad max_size");
    }

    #[test]
    fn unsupported_feature_constructor_and_display() {
        let err = ParseError::unsupported_feature("legacy.py", "print statement");
        match &err {
            ParseError::UnsupportedFeature { file, feature } => {
                assert_eq!(file, "legacy.py");
                assert_eq!(feature, "print statement");
            }
            other => panic!("expected UnsupportedFeature, got {other:?}"),
        }
        assert_eq!(
            err.to_string(),
            "Unsupported Python feature in legacy.py: print statement"
        );
    }

    #[test]
    fn result_alias_carries_parse_error() {
        let r: Result<u32> = Err(ParseError::graph_error("boom"));
        assert!(r.is_err());
        let ok: Result<u32> = Ok(7);
        assert_eq!(ok.unwrap(), 7);
    }
}
