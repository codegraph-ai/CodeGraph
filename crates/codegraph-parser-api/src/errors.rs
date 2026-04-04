// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur during parsing
#[derive(Error, Debug)]
pub enum ParserError {
    /// Failed to read file
    #[error("IO error reading {0}: {1}")]
    IoError(PathBuf, #[source] std::io::Error),

    /// Syntax error in source code
    #[error("Syntax error in {0}:{1}:{2}: {3}")]
    SyntaxError(PathBuf, usize, usize, String),

    /// File too large
    #[error("File {0} exceeds maximum size ({1} bytes)")]
    FileTooLarge(PathBuf, usize),

    /// Parsing timeout
    #[error("Parsing {0} exceeded timeout")]
    Timeout(PathBuf),

    /// Graph insertion error
    #[error("Failed to insert into graph: {0}")]
    GraphError(String),

    /// Unsupported language feature
    #[error("Unsupported language feature in {0}: {1}")]
    UnsupportedFeature(PathBuf, String),

    /// Generic parsing error
    #[error("Parse error in {0}: {1}")]
    ParseError(PathBuf, String),
}

/// Result type for parser operations
pub type ParserResult<T> = Result<T, ParserError>;
