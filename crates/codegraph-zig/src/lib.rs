// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Zig parser for CodeGraph

mod extractor;
mod mapper;
mod parser_impl;
mod ts_zig;
mod visitor;

pub use parser_impl::ZigParser;

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph::CodeGraph;
    use codegraph_parser_api::CodeParser;
    use std::path::Path;

    #[test]
    fn test_parser_creation() {
        let parser = ZigParser::new();
        assert_eq!(parser.language(), "zig");
    }

    #[test]
    fn test_parse_simple_source() {
        let parser = ZigParser::new();
        let mut graph = CodeGraph::in_memory().unwrap();

        let source = r#"
const std = @import("std");

pub fn add(a: i32, b: i32) i32 {
    return a + b;
}
"#;

        let result = parser.parse_source(source, Path::new("main.zig"), &mut graph);
        assert!(result.is_ok());
    }
}
