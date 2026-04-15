// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! R parser for CodeGraph

mod extractor;
mod mapper;
mod parser_impl;
mod ts_r;
mod visitor;

pub use parser_impl::RParser;

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph::CodeGraph;
    use codegraph_parser_api::CodeParser;
    use std::path::Path;

    #[test]
    fn test_parser_creation() {
        let parser = RParser::new();
        assert_eq!(parser.language(), "r");
    }

    #[test]
    fn test_parse_simple_source() {
        let parser = RParser::new();
        let mut graph = CodeGraph::in_memory().unwrap();

        let source = r#"
add <- function(a, b) {
    a + b
}
"#;

        let result = parser.parse_source(source, Path::new("math.R"), &mut graph);
        assert!(result.is_ok());

        let file_info = result.unwrap();
        assert_eq!(file_info.functions.len(), 1);
    }
}
