// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Scala parser for CodeGraph

mod extractor;
mod mapper;
mod parser_impl;
mod visitor;

pub use parser_impl::ScalaParser;

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph::CodeGraph;
    use codegraph_parser_api::CodeParser;
    use std::path::Path;

    #[test]
    fn test_parser_creation() {
        let parser = ScalaParser::new();
        assert_eq!(parser.language(), "scala");
    }

    #[test]
    fn test_parse_simple_source() {
        let parser = ScalaParser::new();
        let mut graph = CodeGraph::in_memory().unwrap();

        let source = r#"
class Calculator {
  def add(a: Int, b: Int): Int = a + b
}
"#;

        let result = parser.parse_source(source, Path::new("Calculator.scala"), &mut graph);
        assert!(result.is_ok());

        let file_info = result.unwrap();
        assert_eq!(file_info.classes.len(), 1);
    }
}
