// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Ruby parser for CodeGraph
//!
//! This crate provides Ruby source code parsing capabilities, extracting
//! code entities and their relationships for building code graphs.
//!
//! **Author:** Andrey Vasilevsky \<anvanster@gmail.com\>
//! **License:** Apache-2.0
//! **Repository:** <https://github.com/anvanster/codegraph>

mod extractor;
mod mapper;
mod parser_impl;
mod visitor;

pub use parser_impl::RubyParser;

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph::CodeGraph;
    use codegraph_parser_api::CodeParser;
    use std::path::Path;

    #[test]
    fn test_parser_creation() {
        let parser = RubyParser::new();
        assert_eq!(parser.language(), "ruby");
    }

    #[test]
    fn test_parse_simple_source() {
        let parser = RubyParser::new();
        let mut graph = CodeGraph::in_memory().unwrap();

        let source = r#"
class Calculator
  def add(a, b)
    a + b
  end
end
"#;

        let result = parser.parse_source(source, Path::new("calculator.rb"), &mut graph);
        assert!(result.is_ok());

        let file_info = result.unwrap();
        assert_eq!(file_info.classes.len(), 1);
        // Verify the class node exists in the graph
        let class_node = graph.get_node(file_info.classes[0]).unwrap();
        assert_eq!(
            class_node.properties.get("name"),
            Some(&codegraph::PropertyValue::String("Calculator".to_string()))
        );
    }
}
