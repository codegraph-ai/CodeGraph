// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! C++ parser for CodeGraph
//!
//! This crate provides a C++ language parser that extracts code entities
//! (classes, structs, functions, namespaces) and their relationships from C++ source files.
//!
//! # Example
//!
//! ```rust
//! use codegraph::CodeGraph;
//! use codegraph_cpp::CppParser;
//! use codegraph_parser_api::CodeParser;
//! use std::path::Path;
//!
//! let parser = CppParser::new();
//! let mut graph = CodeGraph::in_memory().unwrap();
//!
//! let source = r#"
//!     class MyClass {
//!     public:
//!         void myMethod() {}
//!     };
//! "#;
//!
//! let file_info = parser.parse_source(source, Path::new("example.cpp"), &mut graph).unwrap();
//! println!("Found {} classes", file_info.classes.len());
//! ```
//!
//! **Author:** Andrey Vasilevsky \<anvanster@gmail.com\>
//! **License:** Apache-2.0
//! **Repository:** <https://github.com/anvanster/codegraph>

mod extractor;
mod mapper;
mod parser_impl;
mod visitor;

pub use parser_impl::CppParser;
