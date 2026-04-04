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
