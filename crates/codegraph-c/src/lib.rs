// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! # codegraph-c
//!
//! C parser for CodeGraph - extracts code entities and relationships from C source files.
//!
//! ## Features
//!
//! - Parse C source files (.c) and header files (.h)
//! - Extract functions, structs, unions, enums, and typedefs
//! - Track relationships (includes, function calls)
//! - Calculate cyclomatic complexity metrics
//! - **Tolerant parsing mode** for incomplete/kernel code
//! - **Macro preprocessing** for Linux kernel and system code
//! - **Layered processing pipeline** for better parsing of kernel code
//! - **Platform detection** with support for Linux, FreeBSD, Darwin
//! - Full integration with codegraph-parser-api
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use codegraph_c::CParser;
//! use codegraph_parser_api::CodeParser;
//! use codegraph::CodeGraph;
//! use std::path::Path;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut graph = CodeGraph::in_memory()?;
//! let parser = CParser::new();
//!
//! let file_info = parser.parse_file(Path::new("main.c"), &mut graph)?;
//! println!("Parsed {} functions", file_info.functions.len());
//! # Ok(())
//! # }
//! ```
//!
//! ## Tolerant Parsing
//!
//! For code with syntax errors or missing headers (like kernel code):
//!
//! ```rust,no_run
//! use codegraph_c::extractor::{extract_with_options, ExtractionOptions};
//! use std::path::Path;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let source = r#"
//! static __init int my_init(void) { return 0; }
//! "#;
//!
//! // Use kernel-optimized extraction
//! let options = ExtractionOptions::for_kernel_code();
//! let result = extract_with_options(source, Path::new("test.c"), &options)?;
//!
//! println!("Extracted {} functions (partial: {})", result.ir.functions.len(), result.is_partial);
//! # Ok(())
//! # }
//! ```
//!
//! ## Layered Pipeline
//!
//! For advanced processing with platform-specific optimizations:
//!
//! ```rust,no_run
//! use codegraph_c::pipeline::{Pipeline, PipelineConfig};
//!
//! let pipeline = Pipeline::new();
//! let config = PipelineConfig::for_kernel_code();
//!
//! let source = r#"
//! #include <linux/module.h>
//! MODULE_LICENSE("GPL");
//! static __init int my_init(void) { return 0; }
//! "#;
//!
//! let result = pipeline.process(source, &config);
//! println!("Platform: {} (confidence: {:.0}%)",
//!     result.platform.platform_id,
//!     result.platform.confidence * 100.0);
//! ```
//!
//! **Author:** Andrey Vasilevsky \<anvanster@gmail.com\>
//! **License:** Apache-2.0
//! **Repository:** <https://github.com/anvanster/codegraph>

pub mod extractor;
mod mapper;
mod parser_impl;
pub mod pipeline;
pub mod platform;
pub mod preprocessor;
pub mod visitor;

// Re-export parser-api types for convenience
pub use codegraph_parser_api::{
    CodeParser, FileInfo, ParserConfig, ParserError, ParserMetrics, ProjectInfo,
};

// Export the C parser implementation
pub use parser_impl::CParser;

// Export key types from submodules
pub use extractor::{ExtractionOptions, ExtractionResult};
pub use pipeline::{Pipeline, PipelineConfig, PipelineResult};
pub use platform::{DetectionResult, PlatformModule, PlatformRegistry};
pub use preprocessor::{CPreprocessor, MacroInfo, MacroKind};
pub use visitor::FunctionCall;
