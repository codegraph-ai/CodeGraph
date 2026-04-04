// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! # codegraph-python
//!
//! Python parser plugin for CodeGraph - extracts code entities and relationships
//! from Python source files.
//!
//! ## Features
//!
//! - Parse single Python files or entire projects
//! - Extract functions, classes, methods with full metadata
//! - Track relationships (calls, imports, inheritance)
//! - Configurable behavior (visibility filtering, parallel processing)
//! - Safe: No panics, graceful error handling
//!
//! ## Quick Start (New API - v0.2.0+)
//!
//! ```rust,no_run
//! use codegraph_python::PythonParser;
//! use codegraph_parser_api::CodeParser;
//! use codegraph::CodeGraph;
//! use std::path::Path;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut graph = CodeGraph::in_memory()?;
//! let parser = PythonParser::new();
//!
//! let file_info = parser.parse_file(Path::new("example.py"), &mut graph)?;
//! println!("Parsed {} functions", file_info.functions.len());
//! # Ok(())
//! # }
//! ```
//!
//! ## Legacy API (Deprecated in v0.2.0)
//!
//! ```rust,no_run,ignore
//! use codegraph_python::Parser;
//!
//! // This API is deprecated - use PythonParser with CodeParser trait instead
//! let parser = Parser::new();
//! ```
//!
//! **Author:** Andrey Vasilevsky \<anvanster@gmail.com\>
//! **License:** Apache-2.0
//! **Repository:** <https://github.com/anvanster/codegraph>

// Keep old config and error for backward compatibility with deprecated API
pub mod config;
pub mod error;

mod builder;
mod extractor;
mod parser;
mod parser_impl;
mod visitor;

// Re-export parser-api types for convenience
pub use codegraph_parser_api::{
    CodeParser, FileInfo as ApiFileInfo, ParserConfig as ApiParserConfig, ParserError,
    ProjectInfo as ApiProjectInfo,
};

// Export new parser implementation
pub use parser_impl::PythonParser;

// Legacy exports (deprecated)
pub use config::ParserConfig;
pub use error::{ParseError, Result};

#[deprecated(
    since = "0.2.0",
    note = "Use PythonParser with CodeParser trait instead"
)]
pub use parser::{FileInfo, Parser, ProjectInfo};
