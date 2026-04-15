// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! # codegraph-perl
//!
//! Perl parser for CodeGraph - extracts code entities and relationships from Perl source files.
//!
//! ## Features
//!
//! - Parse Perl source files (.pl, .pm, .t)
//! - Extract subroutines and packages
//! - Track relationships (calls, imports via use/require)
//! - Full integration with codegraph-parser-api
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use codegraph_perl::PerlParser;
//! use codegraph_parser_api::CodeParser;
//! use codegraph::CodeGraph;
//! use std::path::Path;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut graph = CodeGraph::in_memory()?;
//! let parser = PerlParser::new();
//!
//! let source = r#"
//! sub greet {
//!     my ($name) = @_;
//!     print "Hello, $name!\n";
//! }
//! "#;
//!
//! let file_info = parser.parse_source(source, Path::new("hello.pl"), &mut graph)?;
//! println!("Parsed {} subroutines", file_info.functions.len());
//! # Ok(())
//! # }
//! ```
//!
//! **Author:** Andrey Vasilevsky \<anvanster@gmail.com\>
//! **License:** Apache-2.0

mod extractor;
mod mapper;
mod parser_impl;
pub(crate) mod ts_perl;
mod visitor;

// Re-export parser-api types for convenience
pub use codegraph_parser_api::{
    CodeParser, FileInfo, ParserConfig, ParserError, ParserMetrics, ProjectInfo,
};

// Export the Perl parser implementation
pub use parser_impl::PerlParser;

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph::CodeGraph;
    use codegraph_parser_api::CodeParser;
    use std::path::Path;

    #[test]
    fn test_parser_creation() {
        let parser = PerlParser::new();
        assert_eq!(parser.language(), "perl");
    }

    #[test]
    fn test_parse_simple_source() {
        let parser = PerlParser::new();
        let mut graph = CodeGraph::in_memory().unwrap();

        let source = r#"
sub greet {
    my ($name) = @_;
    print "Hello, $name!\n";
}
"#;

        let result = parser.parse_source(source, Path::new("hello.pl"), &mut graph);
        assert!(result.is_ok());

        let file_info = result.unwrap();
        assert_eq!(file_info.functions.len(), 1);
    }
}
