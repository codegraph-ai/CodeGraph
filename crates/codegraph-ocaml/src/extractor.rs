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

    fn extract_ok(source: &str, path: &str) -> CodeIR {
        let config = ParserConfig::default();
        extract(source, Path::new(path), &config).expect("extract should succeed")
    }

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

    #[test]
    fn test_module_name_from_file_stem() {
        let ir = extract_ok("let x = 1\n", "src/parser.ml");
        let module = ir.module.expect("module should be set");
        assert_eq!(module.name, "parser");
    }

    #[test]
    fn test_module_name_unknown_fallback() {
        // ".." has no file_stem, exercising the unwrap_or("unknown") branch.
        let ir = extract_ok("let x = 1\n", "..");
        let module = ir.module.expect("module should be set");
        assert_eq!(module.name, "unknown");
    }

    #[test]
    fn test_module_path_and_language() {
        let ir = extract_ok("let x = 1\n", "lib/util.ml");
        let module = ir.module.expect("module should be set");
        assert_eq!(module.path, "lib/util.ml");
        assert_eq!(module.language, "ocaml");
    }

    #[test]
    fn test_module_line_count() {
        let source = "let a = 1\nlet b = 2\nlet c = 3\n";
        let ir = extract_ok(source, "test.ml");
        let module = ir.module.expect("module should be set");
        assert_eq!(module.line_count, source.lines().count());
    }

    #[test]
    fn test_module_doc_comment_and_attributes_empty() {
        let ir = extract_ok("let x = 1\n", "test.ml");
        let module = ir.module.expect("module should be set");
        assert!(module.doc_comment.is_none());
        assert!(module.attributes.is_empty());
    }

    #[test]
    fn test_empty_source_yields_only_module() {
        let ir = extract_ok("", "empty.ml");
        assert!(ir.module.is_some());
        assert!(ir.functions.is_empty());
        assert!(ir.imports.is_empty());
        assert!(ir.calls.is_empty());
        assert_eq!(ir.module.unwrap().line_count, 0);
    }

    #[test]
    fn test_comment_only_source_yields_no_entities() {
        let ir = extract_ok("(* just a comment *)\n", "test.ml");
        assert!(ir.module.is_some());
        assert!(ir.functions.is_empty());
        assert!(ir.imports.is_empty());
    }

    #[test]
    fn test_calls_populated() {
        let source = r#"
let helper () = 1

let main () =
  helper ()
"#;
        let ir = extract_ok(source, "test.ml");
        assert_eq!(ir.functions.len(), 2);
        assert!(
            !ir.calls.is_empty(),
            "main calling helper should record a call relation"
        );
    }

    #[test]
    fn test_mixed_entities_flow_into_ir() {
        let source = r#"
open Printf

let greet name =
  printf "Hello, %s\n" name
"#;
        let ir = extract_ok(source, "test.ml");
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].name, "greet");
        assert_eq!(ir.imports.len(), 1);
    }

    #[test]
    fn test_multiple_functions() {
        let source = r#"
let a () = 1
let b () = 2
let c () = 3
"#;
        let ir = extract_ok(source, "test.ml");
        assert_eq!(ir.functions.len(), 3);
    }

    #[test]
    fn test_select_language_implementation() {
        // A .ml source file selects the implementation grammar (the else branch).
        let lang = select_language(Path::new("foo.ml"));
        assert_eq!(lang, tree_sitter_ocaml::LANGUAGE_OCAML.into());
        assert_ne!(lang, tree_sitter_ocaml::LANGUAGE_OCAML_INTERFACE.into());
    }

    #[test]
    fn test_select_language_interface() {
        // A .mli interface file selects the OCaml interface grammar.
        let lang = select_language(Path::new("foo.mli"));
        assert_eq!(lang, tree_sitter_ocaml::LANGUAGE_OCAML_INTERFACE.into());
        assert_ne!(lang, tree_sitter_ocaml::LANGUAGE_OCAML.into());
    }

    #[test]
    fn test_select_language_no_extension_defaults_to_implementation() {
        // A path with no extension falls through unwrap_or(false) to the implementation grammar.
        let lang = select_language(Path::new("Makefile"));
        assert_eq!(lang, tree_sitter_ocaml::LANGUAGE_OCAML.into());
    }
}
