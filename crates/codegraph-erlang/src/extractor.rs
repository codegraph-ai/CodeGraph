// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST extraction for Erlang source code

use codegraph_parser_api::{CodeIR, ModuleEntity, ParserConfig, ParserError};
use std::path::Path;
use tree_sitter::Parser;

use crate::visitor::ErlangVisitor;

/// Extract code entities and relationships from Erlang source code
pub(crate) fn extract(
    source: &str,
    file_path: &Path,
    _config: &ParserConfig,
) -> Result<CodeIR, ParserError> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_erlang::LANGUAGE.into())
        .map_err(|e| ParserError::ParseError(file_path.to_path_buf(), e.to_string()))?;

    let tree = parser.parse(source, None).ok_or_else(|| {
        ParserError::ParseError(file_path.to_path_buf(), "Failed to parse".to_string())
    })?;

    let root_node = tree.root_node();

    let mut ir = CodeIR::new(file_path.to_path_buf());

    // Derive module name: prefer `-module(Name).` attribute, fall back to filename stem
    let mut visitor = ErlangVisitor::new(source.as_bytes());
    visitor.visit_node(root_node);

    let module_name = visitor
        .module_name
        .clone()
        .or_else(|| {
            file_path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "unknown".to_string());

    ir.module = Some(ModuleEntity {
        name: module_name,
        path: file_path.display().to_string(),
        language: "erlang".to_string(),
        line_count: source.lines().count(),
        doc_comment: None,
        attributes: Vec::new(),
    });

    ir.functions = visitor.functions;
    ir.classes = visitor.classes;
    ir.traits = visitor.traits;
    ir.imports = visitor.imports;
    ir.calls = visitor.calls;

    Ok(ir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_simple_function() {
        let source = r#"-module(hello).
-export([hello/0]).

hello() ->
    io:format("Hello, world!~n").
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.erl"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].name, "hello");
        assert_eq!(ir.functions[0].visibility, "public");
    }

    #[test]
    fn test_extract_module_name() {
        let source = "-module(myapp).\n\nfoo() -> ok.\n";
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.erl"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.module.as_ref().unwrap().name, "myapp");
    }

    #[test]
    fn test_extract_record() {
        let source = "-module(m).\n-record(person, {name, age}).\n\nfoo() -> ok.\n";
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.erl"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "person");
    }

    #[test]
    fn test_extract_import() {
        let source = "-module(m).\n-import(lists, [map/2]).\n\nfoo() -> ok.\n";
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.erl"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.imports.len(), 1);
        assert_eq!(ir.imports[0].imported, "lists");
    }

    #[test]
    fn test_extract_behaviour() {
        let source = "-module(m).\n-behaviour(gen_server).\n\ninit([]) -> {ok, #{}}.\n";
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.erl"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.traits.len(), 1);
        assert_eq!(ir.traits[0].name, "gen_server");
    }
}
