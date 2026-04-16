// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! CSS parser for CodeGraph

mod extractor;
mod mapper;
mod parser_impl;
mod visitor;

pub use parser_impl::CssParser;

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph::CodeGraph;
    use codegraph_parser_api::CodeParser;
    use std::path::Path;

    #[test]
    fn test_parser_creation() {
        let parser = CssParser::new();
        assert_eq!(parser.language(), "css");
    }

    #[test]
    fn test_parse_simple_source() {
        let parser = CssParser::new();
        let mut graph = CodeGraph::in_memory().unwrap();

        let source = r#"
.container {
    max-width: 1200px;
}
"#;

        let result = parser.parse_source(source, Path::new("styles.css"), &mut graph);
        assert!(result.is_ok());

        let file_info = result.unwrap();
        assert_eq!(file_info.functions.len(), 1);
    }
}
