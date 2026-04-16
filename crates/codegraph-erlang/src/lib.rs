// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Erlang parser for CodeGraph

mod extractor;
mod mapper;
mod parser_impl;
mod visitor;

pub use parser_impl::ErlangParser;

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph::CodeGraph;
    use codegraph_parser_api::CodeParser;
    use std::path::Path;

    #[test]
    fn test_parser_creation() {
        let parser = ErlangParser::new();
        assert_eq!(parser.language(), "erlang");
    }

    #[test]
    fn test_parse_simple_source() {
        let parser = ErlangParser::new();
        let mut graph = CodeGraph::in_memory().unwrap();

        let source = r#"-module(hello).
-export([greet/1]).

greet(Name) ->
    io:format("Hello, ~s~n", [Name]).
"#;

        let result = parser.parse_source(source, Path::new("hello.erl"), &mut graph);
        assert!(result.is_ok());

        let file_info = result.unwrap();
        assert_eq!(file_info.functions.len(), 1);
    }
}
