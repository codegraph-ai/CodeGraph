// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST extraction for OCaml source code

use codegraph_parser_api::{CodeIR, ModuleEntity, ParserConfig, ParserError};
use std::path::Path;
use tree_sitter::Parser;

use crate::visitor::OcamlVisitor;

/// Select the correct tree-sitter language based on file extension.
/// .mli files use the OCaml interface grammar.
fn select_language(file_path: &Path) -> tree_sitter::Language {
    let is_interface = file_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e == "mli")
        .unwrap_or(false);

    if is_interface {
        tree_sitter_ocaml::LANGUAGE_OCAML_INTERFACE.into()
    } else {
        tree_sitter_ocaml::LANGUAGE_OCAML.into()
    }
}

/// Extract code entities and relationships from OCaml source code
pub(crate) fn extract(
    source: &str,
    file_path: &Path,
    _config: &ParserConfig,
) -> Result<CodeIR, ParserError> {
    let mut parser = Parser::new();
    let language = select_language(file_path);
    parser
        .set_language(&language)
        .map_err(|e| ParserError::ParseError(file_path.to_path_buf(), e.to_string()))?;

    let tree = parser.parse(source, None).ok_or_else(|| {
        ParserError::ParseError(file_path.to_path_buf(), "Failed to parse".to_string())
    })?;

    let root_node = tree.root_node();

    let mut ir = CodeIR::new(file_path.to_path_buf());

    let module_name = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();
    ir.module = Some(ModuleEntity {
        name: module_name,
        path: file_path.display().to_string(),
        language: "ocaml".to_string(),
        line_count: source.lines().count(),
        doc_comment: None,
        attributes: Vec::new(),
    });

    let mut visitor = OcamlVisitor::new(source.as_bytes());
    visitor.visit_node(root_node);

    ir.functions = visitor.functions;
    ir.imports = visitor.imports;
    ir.calls = visitor.calls;

    Ok(ir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_simple_function() {
        let source = r#"
let hello () =
  print_endline "Hello, world!"
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.ml"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].name, "hello");
    }

    #[test]
    fn test_extract_open() {
        let source = r#"
open Printf
open List
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.ml"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.imports.len(), 2);
    }
}
