// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Swift parser for CodeGraph
//!
//! This crate provides Swift language support for the CodeGraph code analysis tool.
//! It uses tree-sitter-swift to parse Swift source files and extract code entities
//! and relationships.
//!
//! # Example
//!
//! ```rust
//! use codegraph::CodeGraph;
//! use codegraph_swift::SwiftParser;
//! use codegraph_parser_api::CodeParser;
//! use std::path::Path;
//!
//! let parser = SwiftParser::new();
//! let mut graph = CodeGraph::in_memory().unwrap();
//!
//! let source = r#"
//! class Person {
//!     var name: String
//!     init(name: String) {
//!         self.name = name
//!     }
//! }
//! "#;
//!
//! let file_info = parser.parse_source(source, Path::new("Person.swift"), &mut graph).unwrap();
//! assert!(!file_info.classes.is_empty());
//! ```
//!
//! **Author:** Andrey Vasilevsky \<anvanster@gmail.com\>
//! **License:** Apache-2.0
//! **Repository:** <https://github.com/anvanster/codegraph>

mod extractor;
mod mapper;
mod parser_impl;
mod visitor;

pub use parser_impl::SwiftParser;
