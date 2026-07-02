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

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error as _;

    #[test]
    fn io_error_display_and_source() {
        let src = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
        let err = ParserError::IoError(PathBuf::from("src/main.rs"), src);
        assert_eq!(err.to_string(), "IO error reading src/main.rs: missing");
        // The wrapped io::Error is exposed as the error source.
        let source = err.source().expect("IoError should carry a source");
        assert_eq!(source.to_string(), "missing");
    }

    #[test]
    fn syntax_error_display() {
        let err = ParserError::SyntaxError(
            PathBuf::from("lib.rs"),
            12,
            5,
            "unexpected token".to_string(),
        );
        assert_eq!(
            err.to_string(),
            "Syntax error in lib.rs:12:5: unexpected token"
        );
    }

    #[test]
    fn file_too_large_display() {
        let err = ParserError::FileTooLarge(PathBuf::from("big.rs"), 1048576);
        assert_eq!(
            err.to_string(),
            "File big.rs exceeds maximum size (1048576 bytes)"
        );
    }

    #[test]
    fn timeout_display() {
        let err = ParserError::Timeout(PathBuf::from("slow.rs"));
        assert_eq!(err.to_string(), "Parsing slow.rs exceeded timeout");
    }

    #[test]
    fn graph_error_display() {
        let err = ParserError::GraphError("node id collision".to_string());
        assert_eq!(
            err.to_string(),
            "Failed to insert into graph: node id collision"
        );
    }

    #[test]
    fn unsupported_feature_display() {
        let err = ParserError::UnsupportedFeature(
            PathBuf::from("mod.rs"),
            "async generators".to_string(),
        );
        assert_eq!(
            err.to_string(),
            "Unsupported language feature in mod.rs: async generators"
        );
    }

    #[test]
    fn parse_error_display() {
        let err = ParserError::ParseError(PathBuf::from("a.rs"), "bad node".to_string());
        assert_eq!(err.to_string(), "Parse error in a.rs: bad node");
    }

    #[test]
    fn non_io_variants_have_no_source() {
        // Only IoError carries a #[source]; the rest expose no nested cause.
        assert!(ParserError::Timeout(PathBuf::from("x.rs"))
            .source()
            .is_none());
        assert!(ParserError::GraphError("g".to_string()).source().is_none());
    }

    #[test]
    fn all_remaining_non_io_variants_have_no_source() {
        // The four variants not covered by non_io_variants_have_no_source also
        // omit #[source], so none exposes a nested cause.
        assert!(
            ParserError::SyntaxError(PathBuf::from("a.rs"), 1, 2, "e".to_string())
                .source()
                .is_none()
        );
        assert!(ParserError::FileTooLarge(PathBuf::from("b.rs"), 42)
            .source()
            .is_none());
        assert!(
            ParserError::UnsupportedFeature(PathBuf::from("c.rs"), "f".to_string())
                .source()
                .is_none()
        );
        assert!(
            ParserError::ParseError(PathBuf::from("d.rs"), "p".to_string())
                .source()
                .is_none()
        );
    }

    #[test]
    fn io_error_source_downcasts_to_concrete_io_error() {
        // The #[source] must preserve the concrete io::Error, not just its
        // string, so consumers can match on ErrorKind (e.g. NotFound).
        let src = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let err = ParserError::IoError(PathBuf::from("locked.rs"), src);
        let source = err.source().expect("IoError should carry a source");
        let io_err = source
            .downcast_ref::<std::io::Error>()
            .expect("source should downcast to io::Error");
        assert_eq!(io_err.kind(), std::io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn parser_result_alias_carries_error() {
        fn wrap(v: u32) -> ParserResult<u32> {
            Ok(v)
        }
        assert_eq!(wrap(7).expect("Ok value"), 7);
        let err: ParserResult<u32> = Err(ParserError::GraphError("boom".to_string()));
        assert!(matches!(err, Err(ParserError::GraphError(_))));
    }
}
