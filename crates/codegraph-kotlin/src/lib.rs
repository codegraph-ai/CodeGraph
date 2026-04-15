// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! # codegraph-kotlin
//!
//! Kotlin parser for CodeGraph - extracts code entities and relationships from Kotlin source files.
//!
//! ## Features
//!
//! - Parse Kotlin source files (.kt, .kts)
//! - Extract functions, classes, interfaces, objects, and data classes
//! - Track relationships (calls, imports, inheritance, implementations)
//! - Full integration with codegraph-parser-api
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use codegraph_kotlin::KotlinParser;
//! use codegraph_parser_api::CodeParser;
//! use codegraph::CodeGraph;
//! use std::path::Path;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut graph = CodeGraph::in_memory()?;
//! let parser = KotlinParser::new();
//!
//! let file_info = parser.parse_file(Path::new("Main.kt"), &mut graph)?;
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
pub(crate) mod ts_kotlin;
mod visitor;

// Re-export parser-api types for convenience
pub use codegraph_parser_api::{
    CodeParser, FileInfo, ParserConfig, ParserError, ParserMetrics, ProjectInfo,
};

// Export the Kotlin parser implementation
pub use parser_impl::KotlinParser;
