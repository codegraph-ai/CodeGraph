// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! COBOL parser for CodeGraph
//!
//! This crate provides COBOL language support for the CodeGraph code analysis tool.
//! It parses standard COBOL constructs (programs, paragraphs, sections, COPY statements,
//! and CALL statements) using the vendored tree-sitter-cobol grammar.
//!
//! Supported file extensions: `.cob`, `.cbl`, `.cobol`, `.cpy`
//!
//! **Author:** Andrey Vasilevsky \<anvanster@gmail.com\>
//! **License:** Apache-2.0
//! **Repository:** <https://github.com/anvanster/codegraph>

mod extractor;
mod mapper;
mod parser_impl;
mod ts_cobol;
mod visitor;

pub use parser_impl::CobolParser;
