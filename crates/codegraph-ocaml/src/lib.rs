// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! OCaml parser for CodeGraph

mod extractor;
mod mapper;
mod parser_impl;
mod visitor;

pub use parser_impl::OcamlParser;

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph::CodeGraph;
    use codegraph_parser_api::CodeParser;
    use std::path::Path;

    #[test]
    fn test_parser_creation() {
        let parser = OcamlParser::new();
        assert_eq!(parser.language(), "ocaml");
    }

    #[test]
    fn test_parse_simple_source() {
        let parser = OcamlParser::new();
        let mut graph = CodeGraph::in_memory().unwrap();

        let source = r#"
let greet name =
  Printf.printf "Hello, %s\n" name
"#;

        let result = parser.parse_source(source, Path::new("hello.ml"), &mut graph);
        assert!(result.is_ok());

        let file_info = result.unwrap();
        assert_eq!(file_info.functions.len(), 1);
    }

    #[test]
    fn test_parse_mli_file() {
        let parser = OcamlParser::new();
        let mut graph = CodeGraph::in_memory().unwrap();

        let source = r#"
val greet : string -> unit
"#;

        let result = parser.parse_source(source, Path::new("hello.mli"), &mut graph);
        assert!(result.is_ok());
    }
}
