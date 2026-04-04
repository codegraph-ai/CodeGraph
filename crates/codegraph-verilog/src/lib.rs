// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! # codegraph-verilog
//!
//! Verilog parser for CodeGraph - extracts code entities and relationships
//! from Verilog source files.
//!
//! ## Features
//!
//! - Parse Verilog source files (`.v`, `.vh`)
//! - Extract modules, functions, and tasks
//! - Track relationships (module instantiations, includes, package imports)
//! - Full integration with codegraph-parser-api
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use codegraph_verilog::VerilogParser;
//! use codegraph_parser_api::CodeParser;
//! use codegraph::CodeGraph;
//! use std::path::Path;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut graph = CodeGraph::in_memory()?;
//! let parser = VerilogParser::new();
//!
//! let file_info = parser.parse_file(Path::new("counter.v"), &mut graph)?;
//! println!("Parsed {} modules", file_info.classes.len());
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
mod ts_verilog;
mod visitor;

pub use codegraph_parser_api::{
    CodeParser, FileInfo, ParserConfig, ParserError, ParserMetrics, ProjectInfo,
};
pub use parser_impl::VerilogParser;
