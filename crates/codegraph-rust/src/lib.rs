// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! # codegraph-rust
//!
//! Rust parser for CodeGraph - extracts code entities and relationships from Rust source files.
//!
//! ## Features
//!
//! - Parse Rust source files and extract functions, structs, enums, traits, and modules
//! - Track relationships (function calls, use statements, trait implementations)
//! - Support for Rust-specific constructs (impl blocks, associated functions, generics)
//! - Configurable behavior via ParserConfig
//! - Full integration with codegraph-parser-api
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use codegraph_rust::RustParser;
//! use codegraph_parser_api::CodeParser;
//! use codegraph::CodeGraph;
//! use std::path::Path;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut graph = CodeGraph::in_memory()?;
//! let parser = RustParser::new();
//!
//! let file_info = parser.parse_file(Path::new("src/main.rs"), &mut graph)?;
//! println!("Parsed {} functions", file_info.functions.len());
//! # Ok(())
//! # }
//! ```
//!
//! **Author:** Andrey Vasilevsky \<anvanster@gmail.com\>
//! **License:** Apache-2.0
//! **Repository:** <https://github.com/anvanster/codegraph>

mod extractor;
mod mapper;
mod parser_impl;
mod visitor;

// Re-export parser-api types for convenience
pub use codegraph_parser_api::{
    CodeParser, FileInfo, ParserConfig, ParserError, ParserMetrics, ProjectInfo,
};

// Export the Rust parser implementation
pub use parser_impl::RustParser;
