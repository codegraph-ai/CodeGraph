// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Elm parser for CodeGraph

mod extractor;
mod mapper;
mod parser_impl;
mod visitor;

pub use parser_impl::ElmParser;

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph::CodeGraph;
    use codegraph_parser_api::CodeParser;
    use std::path::Path;

    #[test]
    fn test_parser_creation() {
        let parser = ElmParser::new();
        assert_eq!(parser.language(), "elm");
    }

    #[test]
    fn test_parse_simple_source() {
        let parser = ElmParser::new();
        let mut graph = CodeGraph::in_memory().unwrap();

        let source = r#"module Main exposing (main)

import Html exposing (Html, text)

main : Html msg
main =
    text "Hello, World!"
"#;

        let result = parser.parse_source(source, Path::new("Main.elm"), &mut graph);
        assert!(result.is_ok());

        let file_info = result.unwrap();
        assert_eq!(file_info.functions.len(), 1);
    }
}
