// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST extraction for Fortran source code

use codegraph_parser_api::{CodeIR, ModuleEntity, ParserConfig, ParserError};
use std::path::Path;
use tree_sitter::Parser;

use crate::visitor::FortranVisitor;

/// Extract code entities and relationships from Fortran source code
pub fn extract(
    source: &str,
    file_path: &Path,
    _config: &ParserConfig,
) -> Result<CodeIR, ParserError> {
    let mut parser = Parser::new();
    let language = crate::ts_fortran::language();
    parser
        .set_language(&language)
        .map_err(|e| ParserError::ParseError(file_path.to_path_buf(), e.to_string()))?;

    let tree = parser.parse(source, None).ok_or_else(|| {
        ParserError::ParseError(file_path.to_path_buf(), "Failed to parse".to_string())
    })?;

    // Note: NOT checking root_node.has_error() — Fortran code often uses
    // preprocessor directives (#include, #ifdef) and dialect extensions that
    // produce partial error nodes in the grammar while still containing
    // extractable entities.
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
        language: "fortran".to_string(),
        line_count: source.lines().count(),
        doc_comment: None,
        attributes: Vec::new(),
    });

    let mut visitor = FortranVisitor::new(source.as_bytes());
    visitor.visit_node(root_node);

    ir.classes = visitor.program_units;
    ir.functions = visitor.functions;
    ir.imports = visitor.imports;
    ir.calls = visitor.calls;

    Ok(ir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_simple_program() {
        let source = "program hello\n  implicit none\n  print *, 'hello'\nend program hello\n";
        let config = ParserConfig::default();
        let result = extract(source, Path::new("hello.f90"), &config);

        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name.to_lowercase(), "hello");
    }

    #[test]
    fn test_extract_module() {
        let source = "module mathlib\n  implicit none\ncontains\n  function add(a, b) result(c)\n    integer, intent(in) :: a, b\n    integer :: c\n    c = a + b\n  end function add\nend module mathlib\n";
        let config = ParserConfig::default();
        let result = extract(source, Path::new("mathlib.f90"), &config);

        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name.to_lowercase(), "mathlib");
        assert!(!ir.functions.is_empty(), "Expected at least one function");
    }

    #[test]
    fn test_extract_subroutine() {
        let source = "subroutine greet(name)\n  character(*), intent(in) :: name\n  print *, 'Hello', name\nend subroutine greet\n";
        let config = ParserConfig::default();
        let result = extract(source, Path::new("greet.f90"), &config);

        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let ir = result.unwrap();
        assert!(!ir.functions.is_empty(), "Expected subroutine");
        assert_eq!(ir.functions[0].name.to_lowercase(), "greet");
    }

    #[test]
    fn test_extract_use_statement() {
        let source = "program main\n  use iso_fortran_env\n  implicit none\nend program main\n";
        let config = ParserConfig::default();
        let result = extract(source, Path::new("main.f90"), &config);

        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let ir = result.unwrap();
        assert!(!ir.imports.is_empty(), "Expected import");
        assert_eq!(ir.imports[0].imported.to_lowercase(), "iso_fortran_env");
    }

    #[test]
    fn test_extract_call() {
        let source = "program main\n  implicit none\n  call greet('world')\nend program main\n";
        let config = ParserConfig::default();
        let result = extract(source, Path::new("main.f90"), &config);

        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let ir = result.unwrap();
        assert!(!ir.calls.is_empty(), "Expected call");
        assert_eq!(ir.calls[0].callee.to_lowercase(), "greet");
    }

    #[test]
    fn test_extract_module_info() {
        let source = "program test\nend program test\n";
        let config = ParserConfig::default();
        let result = extract(source, Path::new("my_prog.f90"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert!(ir.module.is_some());
        let module = ir.module.unwrap();
        assert_eq!(module.name, "my_prog");
        assert_eq!(module.language, "fortran");
        assert!(module.line_count > 0);
    }

    #[test]
    fn test_extract_with_syntax_error_does_not_fail() {
        // Deliberately broken Fortran — should not return Err
        let source = "program broken\n  integer :: x\n  if (x > 0\nend program broken\n";
        let config = ParserConfig::default();
        let result = extract(source, Path::new("broken.f90"), &config);
        // With has_error() check removed, this should succeed (not Err)
        // even if the AST is partial and few entities are extracted
        assert!(result.is_ok(), "Should not fail on syntax errors");
    }

    #[test]
    fn test_extract_with_preprocessor_directives() {
        // Fortran with C preprocessor directives (common in scientific code)
        let source = concat!(
            "#ifdef USE_MPI\n",
            "  use mpi\n",
            "#endif\n",
            "program preproc\n",
            "  implicit none\n",
            "  call do_work()\n",
            "end program preproc\n",
        );
        let config = ParserConfig::default();
        let result = extract(source, Path::new("preproc.f90"), &config);
        // Should not fail entirely due to preprocessor directives
        assert!(
            result.is_ok(),
            "Should handle preprocessor directives: {:?}",
            result.err()
        );
    }
}
