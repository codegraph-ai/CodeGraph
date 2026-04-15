// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST extraction for Scala source code

use codegraph_parser_api::{CodeIR, ModuleEntity, ParserConfig, ParserError};
use std::path::Path;
use tree_sitter::Parser;

use crate::visitor::ScalaVisitor;

/// Extract code entities and relationships from Scala source code
pub(crate) fn extract(
    source: &str,
    file_path: &Path,
    _config: &ParserConfig,
) -> Result<CodeIR, ParserError> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_scala::LANGUAGE.into())
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
        language: "scala".to_string(),
        line_count: source.lines().count(),
        doc_comment: None,
        attributes: Vec::new(),
    });

    let mut visitor = ScalaVisitor::new(source.as_bytes());
    visitor.visit_node(root_node);

    ir.functions = visitor.functions;
    ir.classes = visitor.classes;
    ir.traits = visitor.traits;
    ir.imports = visitor.imports;
    ir.calls = visitor.calls;
    ir.inheritance = visitor.inheritance;
    ir.implementations = visitor.implementations;

    Ok(ir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_simple_function() {
        let source = r#"
def add(a: Int, b: Int): Int = a + b
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.scala"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].name, "add");
    }

    #[test]
    fn test_extract_class() {
        let source = r#"
class Person(val name: String, val age: Int)
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.scala"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "Person");
    }

    #[test]
    fn test_extract_import() {
        let source = r#"
import scala.collection.mutable.ListBuffer
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.scala"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.imports.len(), 1);
    }
}
