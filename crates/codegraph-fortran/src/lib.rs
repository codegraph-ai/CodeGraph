// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! # codegraph-fortran
//!
//! Fortran parser for CodeGraph - extracts code entities and relationships
//! from Fortran source files.
//!
//! ## Features
//!
//! - Parse Fortran source files (`.f`, `.f90`, `.f95`, `.f03`, `.f08`, `.for`, `.ftn`)
//! - Extract program units (program, module, submodule, block_data)
//! - Extract subroutines and functions
//! - Track relationships (USE imports, CALL statements, function calls)
//! - Full integration with codegraph-parser-api
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use codegraph_fortran::FortranParser;
//! use codegraph_parser_api::CodeParser;
//! use codegraph::CodeGraph;
//! use std::path::Path;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut graph = CodeGraph::in_memory()?;
//! let parser = FortranParser::new();
//!
//! let file_info = parser.parse_file(Path::new("hello.f90"), &mut graph)?;
//! println!("Parsed {} program units", file_info.classes.len());
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
mod ts_fortran;
mod visitor;

pub use codegraph_parser_api::{
    CodeParser, FileInfo, ParserConfig, ParserError, ParserMetrics, ProjectInfo,
};
pub use parser_impl::FortranParser;
