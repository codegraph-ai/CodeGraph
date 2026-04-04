// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Error types for git mining operations.

use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur during git mining operations.
#[derive(Error, Debug)]
pub enum GitMiningError {
    #[error("Git is not available on this system")]
    GitNotAvailable,

    #[error("Path is not a git repository: {0}")]
    NotARepository(PathBuf),

    #[error("Git command failed: {0}")]
    CommandFailed(String),

    #[error("Failed to parse git output: {0}")]
    ParseError(String),

    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("UTF-8 decoding error: {0}")]
    Utf8Error(#[from] std::string::FromUtf8Error),

    #[error("Memory storage error: {0}")]
    MemoryError(String),
}

impl From<codegraph_memory::MemoryError> for GitMiningError {
    fn from(err: codegraph_memory::MemoryError) -> Self {
        GitMiningError::MemoryError(err.to_string())
    }
}
