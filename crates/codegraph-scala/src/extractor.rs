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

    fn extract_ok(source: &str) -> CodeIR {
        let config = ParserConfig::default();
        extract(source, Path::new("test.scala"), &config).expect("extract should succeed")
    }

    #[test]
    fn test_module_name_from_file_stem() {
        let ir = extract_ok("");
        let module = ir.module.expect("module should be set");
        assert_eq!(module.name, "test");
    }

    #[test]
    fn test_module_name_unknown_fallback() {
        let config = ParserConfig::default();
        let ir = extract("", Path::new(".."), &config).expect("extract should succeed");
        let module = ir.module.expect("module should be set");
        assert_eq!(module.name, "unknown");
    }

    #[test]
    fn test_module_path_and_language() {
        let ir = extract_ok("");
        let module = ir.module.expect("module should be set");
        assert_eq!(module.path, "test.scala");
        assert_eq!(module.language, "scala");
    }

    #[test]
    fn test_module_line_count() {
        let ir = extract_ok("def a(): Int = 1\ndef b(): Int = 2\n");
        let module = ir.module.expect("module should be set");
        assert_eq!(module.line_count, 2);
    }

    #[test]
    fn test_module_doc_comment_and_attributes_empty() {
        let ir = extract_ok("def add(a: Int, b: Int): Int = a + b\n");
        let module = ir.module.expect("module should be set");
        assert!(module.doc_comment.is_none());
        assert!(module.attributes.is_empty());
    }

    #[test]
    fn test_empty_source_yields_only_module() {
        let ir = extract_ok("");
        assert!(ir.module.is_some());
        assert!(ir.functions.is_empty());
        assert!(ir.classes.is_empty());
        assert!(ir.traits.is_empty());
        assert!(ir.imports.is_empty());
        assert!(ir.calls.is_empty());
        assert!(ir.inheritance.is_empty());
        assert!(ir.implementations.is_empty());
    }

    #[test]
    fn test_comment_only_source() {
        let ir = extract_ok("// just a comment\n");
        assert!(ir.module.is_some());
        assert!(ir.functions.is_empty());
        assert!(ir.classes.is_empty());
        assert!(ir.calls.is_empty());
    }

    #[test]
    fn test_object_flows_into_classes() {
        let ir = extract_ok("object Config {\n  val port = 8080\n}\n");
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "Config");
        assert!(ir.classes[0].attributes.iter().any(|a| a == "object"));
    }

    #[test]
    fn test_trait_flows_into_traits() {
        let ir = extract_ok("trait Greeter {\n  def greet(): String\n}\n");
        assert_eq!(ir.traits.len(), 1);
        assert_eq!(ir.traits[0].name, "Greeter");
    }

    #[test]
    fn test_class_extends_populates_inheritance() {
        let ir = extract_ok("class Dog extends Animal\n");
        assert_eq!(ir.inheritance.len(), 1);
        assert_eq!(ir.inheritance[0].child, "Dog");
        assert_eq!(ir.inheritance[0].parent, "Animal");
    }

    #[test]
    fn test_calls_populated_via_caller_callee() {
        let source = r#"
def helper(): Int = 1
def run(): Int = {
  helper()
}
"#;
        let ir = extract_ok(source);
        assert!(
            ir.calls
                .iter()
                .any(|c| c.caller == "run" && c.callee == "helper"),
            "expected a run->helper call, got {:?}",
            ir.calls
        );
    }

    #[test]
    fn test_mixed_source_populates_multiple_kinds() {
        let source = r#"
import scala.collection.mutable.ListBuffer
trait Named {
  def name(): String
}
class Dog extends Animal
def add(a: Int, b: Int): Int = a + b
"#;
        let ir = extract_ok(source);
        assert!(!ir.imports.is_empty(), "imports should be populated");
        assert!(!ir.traits.is_empty(), "traits should be populated");
        assert!(!ir.classes.is_empty(), "classes should be populated");
        assert!(!ir.functions.is_empty(), "functions should be populated");
        assert!(
            !ir.inheritance.is_empty(),
            "inheritance should be populated"
        );
    }

    #[test]
    fn test_multiple_functions_extracted() {
        let source = r#"
def one(): Int = 1
def two(): Int = 2
def three(): Int = 3
"#;
        let ir = extract_ok(source);
        assert_eq!(ir.functions.len(), 3);
        let names: Vec<&str> = ir.functions.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"one"));
        assert!(names.contains(&"two"));
        assert!(names.contains(&"three"));
    }

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
