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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn git_not_available_display() {
        let err = GitMiningError::GitNotAvailable;
        assert_eq!(err.to_string(), "Git is not available on this system");
    }

    #[test]
    fn not_a_repository_display_includes_path() {
        let err = GitMiningError::NotARepository(PathBuf::from("/tmp/nope"));
        assert_eq!(err.to_string(), "Path is not a git repository: /tmp/nope");
    }

    #[test]
    fn command_failed_display_includes_message() {
        let err = GitMiningError::CommandFailed("exit code 128".to_string());
        assert_eq!(err.to_string(), "Git command failed: exit code 128");
    }

    #[test]
    fn parse_error_display_includes_message() {
        let err = GitMiningError::ParseError("bad ref".to_string());
        assert_eq!(err.to_string(), "Failed to parse git output: bad ref");
    }

    #[test]
    fn io_error_from_impl_and_display() {
        let io_err = io::Error::new(io::ErrorKind::PermissionDenied, "denied");
        let err: GitMiningError = io_err.into();
        assert!(matches!(err, GitMiningError::IoError(_)));
        assert_eq!(err.to_string(), "I/O error: denied");
    }

    #[test]
    fn utf8_error_from_impl_and_display() {
        // 0xFF is not valid UTF-8, so from_utf8 yields a FromUtf8Error.
        let utf8_err = String::from_utf8(vec![0xFF]).unwrap_err();
        let err: GitMiningError = utf8_err.into();
        assert!(matches!(err, GitMiningError::Utf8Error(_)));
        assert!(err.to_string().starts_with("UTF-8 decoding error: "));
    }

    #[test]
    fn memory_error_from_impl_flattens_to_string() {
        let mem_err = codegraph_memory::MemoryError::not_found("missing key");
        let err: GitMiningError = mem_err.into();
        assert!(matches!(err, GitMiningError::MemoryError(_)));
        assert_eq!(
            err.to_string(),
            "Memory storage error: Memory not found: missing key"
        );
    }
}
