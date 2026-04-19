// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! # codegraph-dockerfile
//!
//! Dockerfile parser for CodeGraph - extracts directives from Dockerfile / Containerfile
//! sources for IaC (Infrastructure as Code) security scanning.
//!
//! ## Features
//!
//! - Parse Dockerfile / Containerfile / `*.dockerfile` files (with or without extension)
//! - Extract directives (FROM, USER, EXPOSE, ENV, ARG, ADD, COPY, RUN, CMD, ENTRYPOINT,
//!   VOLUME, WORKDIR, etc.) as Function entities
//! - Each directive's body is captured in `body_prefix` so the IaC scanner can detect
//!   patterns like `USER root`, `:latest` images, hardcoded ports, secrets, etc.
//! - Full integration with codegraph-parser-api
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use codegraph_dockerfile::DockerfileParser;
//! use codegraph_parser_api::CodeParser;
//! use codegraph::CodeGraph;
//! use std::path::Path;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut graph = CodeGraph::in_memory()?;
//! let parser = DockerfileParser::new();
//!
//! let source = r#"
//! FROM python:3.11
//! USER root
//! EXPOSE 8080
//! COPY . /app
//! CMD ["python", "app.py"]
//! "#;
//!
//! let file_info = parser.parse_source(source, Path::new("Dockerfile"), &mut graph)?;
//! println!("Parsed {} directives", file_info.functions.len());
//! # Ok(())
//! # }
//! ```
//!
//! **Author:** Andrey Vasilevsky \<anvanster@gmail.com\>
//! **License:** Apache-2.0

mod extractor;
mod mapper;
mod parser_impl;
pub(crate) mod ts_dockerfile;
mod visitor;

/// Expose the underlying tree-sitter Language for diagnostics / external use.
#[doc(hidden)]
pub fn ts_dockerfile_language() -> tree_sitter::Language {
    ts_dockerfile::language()
}

// Re-export parser-api types for convenience
pub use codegraph_parser_api::{
    CodeParser, FileInfo, ParserConfig, ParserError, ParserMetrics, ProjectInfo,
};

// Export the Dockerfile parser implementation
pub use parser_impl::DockerfileParser;

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph::CodeGraph;
    use codegraph_parser_api::CodeParser;
    use std::path::Path;

    #[test]
    fn test_parser_creation() {
        let parser = DockerfileParser::new();
        assert_eq!(parser.language(), "dockerfile");
    }

    #[test]
    fn test_parse_simple_source() {
        let parser = DockerfileParser::new();
        let mut graph = CodeGraph::in_memory().unwrap();

        let source = "FROM python:3.11\nUSER root\nCMD [\"python\", \"app.py\"]\n";

        let result = parser.parse_source(source, Path::new("Dockerfile"), &mut graph);
        assert!(result.is_ok(), "parse failed: {:?}", result.err());
    }
}
