// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! # codegraph-typescript
//!
//! TypeScript/JavaScript parser for CodeGraph - extracts code entities and relationships.
//!
//! ## Features
//!
//! - Parse TypeScript and JavaScript files
//! - Extract functions, classes, interfaces, and modules
//! - Track relationships (calls, imports, inheritance, implementations)
//! - Support for modern JS/TS features (arrow functions, async/await, decorators)
//! - Full integration with codegraph-parser-api
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use codegraph_typescript::TypeScriptParser;
//! use codegraph_parser_api::CodeParser;
//! use codegraph::CodeGraph;
//! use std::path::Path;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut graph = CodeGraph::in_memory()?;
//! let parser = TypeScriptParser::new();
//!
//! let file_info = parser.parse_file(Path::new("src/index.ts"), &mut graph)?;
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

// Export the TypeScript parser implementation
pub use parser_impl::TypeScriptParser;
