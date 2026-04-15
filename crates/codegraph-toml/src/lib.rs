// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! # codegraph-toml
//!
//! TOML parser for CodeGraph — extracts searchable entities from TOML config files.
//!
//! ## Mapping
//!
//! | TOML construct         | CodeGraph entity        |
//! |------------------------|-------------------------|
//! | `[table]` header       | `Class` node            |
//! | `[[array-of-tables]]`  | `Class` node            |
//! | `key = value` pair     | `Function` node (proxy) |
//! | File                   | `CodeFile` node         |
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use codegraph_toml::TomlParser;
//! use codegraph_parser_api::CodeParser;
//! use codegraph::CodeGraph;
//! use std::path::Path;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut graph = CodeGraph::in_memory()?;
//! let parser = TomlParser::new();
//!
//! let file_info = parser.parse_file(Path::new("Cargo.toml"), &mut graph)?;
//! println!("Parsed {} sections", file_info.classes.len());
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
pub(crate) mod ts_toml;
mod visitor;

pub use codegraph_parser_api::{
    CodeParser, FileInfo, ParserConfig, ParserError, ParserMetrics, ProjectInfo,
};

pub use parser_impl::TomlParser;
