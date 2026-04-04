// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Tcl/SDC/UPF parser for CodeGraph
//!
//! This crate provides Tcl language support for the CodeGraph code analysis tool.
//! It parses standard Tcl constructs (procedures, namespaces, imports) and
//! additionally classifies EDA/VLSI commands (SDC constraints, design file
//! reads/writes, Synopsys tool flow commands, OpenROAD patterns).
//!
//! Supported file extensions: `.tcl`, `.sdc`, `.upf`
//!
//! **Author:** Andrey Vasilevsky \<anvanster@gmail.com\>
//! **License:** Apache-2.0
//! **Repository:** <https://github.com/anvanster/codegraph>

mod eda;
mod extractor;
mod mapper;
mod parser_impl;
mod sdc;
mod ts_tcl;
mod visitor;

pub use parser_impl::TclParser;
