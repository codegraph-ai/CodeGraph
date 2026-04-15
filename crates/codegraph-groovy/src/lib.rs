// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Groovy parser for CodeGraph

mod extractor;
mod mapper;
mod parser_impl;
mod visitor;

pub use parser_impl::GroovyParser;

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph::CodeGraph;
    use codegraph_parser_api::CodeParser;
    use std::path::Path;

    #[test]
    fn test_parser_creation() {
        let parser = GroovyParser::new();
        assert_eq!(parser.language(), "groovy");
    }

    #[test]
    fn test_parse_simple_source() {
        let parser = GroovyParser::new();
        let mut graph = CodeGraph::in_memory().unwrap();

        let source = r#"
class UserService {
    def greet(String name) {
        println "Hello, ${name}"
    }
}
"#;

        let result = parser.parse_source(source, Path::new("UserService.groovy"), &mut graph);
        assert!(result.is_ok());

        let file_info = result.unwrap();
        assert!(!file_info.classes.is_empty());
    }

    #[test]
    fn test_parse_with_import() {
        let parser = GroovyParser::new();
        let mut graph = CodeGraph::in_memory().unwrap();

        let source = r#"
import groovy.json.JsonSlurper

class JsonService {
    def parse(String json) {
        def slurper = new JsonSlurper()
        return slurper.parseText(json)
    }
}
"#;

        let result = parser.parse_source(source, Path::new("JsonService.groovy"), &mut graph);
        assert!(result.is_ok());
        let file_info = result.unwrap();
        assert!(!file_info.imports.is_empty());
        assert!(!file_info.classes.is_empty());
    }
}
