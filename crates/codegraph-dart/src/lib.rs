// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Dart parser for CodeGraph

mod extractor;
mod mapper;
mod parser_impl;
mod ts_dart;
mod visitor;

pub use parser_impl::DartParser;

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph::CodeGraph;
    use codegraph_parser_api::CodeParser;
    use std::path::Path;

    #[test]
    fn test_parser_creation() {
        let parser = DartParser::new();
        assert_eq!(parser.language(), "dart");
    }

    #[test]
    fn test_parse_simple_source() {
        let parser = DartParser::new();
        let mut graph = CodeGraph::in_memory().unwrap();

        let source = r#"
class Calculator {
  int add(int a, int b) {
    return a + b;
  }
}
"#;

        let result = parser.parse_source(source, Path::new("calculator.dart"), &mut graph);
        assert!(result.is_ok());

        let file_info = result.unwrap();
        assert_eq!(file_info.classes.len(), 1);
    }
}
