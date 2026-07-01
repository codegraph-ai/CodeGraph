// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST extraction for Dart source code

use codegraph_parser_api::{CodeIR, ModuleEntity, ParserConfig, ParserError};
use std::path::Path;
use tree_sitter::Parser;

use crate::ts_dart;
use crate::visitor::DartVisitor;

/// Extract code entities and relationships from Dart source code
pub(crate) fn extract(
    source: &str,
    file_path: &Path,
    _config: &ParserConfig,
) -> Result<CodeIR, ParserError> {
    let mut parser = Parser::new();
    parser
        .set_language(&ts_dart::language())
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
        language: "dart".to_string(),
        line_count: source.lines().count(),
        doc_comment: None,
        attributes: Vec::new(),
    });

    let mut visitor = DartVisitor::new(source.as_bytes());
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

    fn extract_ok(source: &str, path: &str) -> CodeIR {
        let config = ParserConfig::default();
        extract(source, Path::new(path), &config).expect("extract should succeed")
    }

    #[test]
    fn test_extract_simple_function() {
        let source = r#"
void hello() {
  print("Hello, world!");
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.dart"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].name, "hello");
    }

    #[test]
    fn test_extract_class() {
        let source = r#"
class Person {
  String name;
  int age;
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.dart"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "Person");
    }

    #[test]
    fn test_extract_import() {
        let source = r#"
import 'dart:io';
import 'package:flutter/material.dart';
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.dart"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.imports.len(), 2);
    }

    #[test]
    fn module_name_from_file_stem() {
        let ir = extract_ok("void f() {}\n", "widgets.dart");
        let module = ir.module.expect("module should be set");
        assert_eq!(module.name, "widgets");
    }

    #[test]
    fn module_name_falls_back_to_unknown_without_stem() {
        let ir = extract_ok("void f() {}\n", "..");
        let module = ir.module.expect("module should be set");
        assert_eq!(module.name, "unknown");
    }

    #[test]
    fn module_records_path_language_and_line_count() {
        let source = "void a() {}\nvoid b() {}\n";
        let ir = extract_ok(source, "sample.dart");
        let module = ir.module.expect("module should be set");
        assert_eq!(module.path, "sample.dart");
        assert_eq!(module.language, "dart");
        assert_eq!(module.line_count, source.lines().count());
    }

    #[test]
    fn module_doc_comment_and_attributes_are_empty() {
        let ir = extract_ok("void f() {}\n", "test.dart");
        let module = ir.module.expect("module should be set");
        assert!(module.doc_comment.is_none());
        assert!(module.attributes.is_empty());
    }

    #[test]
    fn empty_source_yields_only_a_module() {
        let ir = extract_ok("", "empty.dart");
        assert!(ir.module.is_some());
        assert!(ir.functions.is_empty());
        assert!(ir.classes.is_empty());
        assert!(ir.traits.is_empty());
        assert!(ir.imports.is_empty());
        assert!(ir.calls.is_empty());
    }

    #[test]
    fn comment_only_source_yields_no_entities() {
        let ir = extract_ok("// just a comment\n", "comment.dart");
        assert!(ir.module.is_some());
        assert!(ir.functions.is_empty());
        assert!(ir.classes.is_empty());
    }

    #[test]
    fn mixin_flows_into_traits() {
        let ir = extract_ok("mixin Logger {\n  void log() {}\n}\n", "test.dart");
        assert_eq!(ir.traits.len(), 1);
        assert_eq!(ir.traits[0].name, "Logger");
    }

    #[test]
    fn class_extends_flows_into_inheritance() {
        let ir = extract_ok("class Dog extends Animal {\n}\n", "test.dart");
        assert_eq!(ir.inheritance.len(), 1);
    }

    #[test]
    fn class_implements_flows_into_implementations() {
        let ir = extract_ok("class Service implements Runnable {\n}\n", "test.dart");
        assert_eq!(ir.implementations.len(), 1);
    }

    #[test]
    fn mixed_source_populates_each_entity_kind() {
        let source = r#"
import 'dart:io';

class Animal {}

class Dog extends Animal {}

mixin Logger {
  void log() {}
}

void main() {}
"#;
        let ir = extract_ok(source, "test.dart");
        assert!(!ir.imports.is_empty());
        assert!(!ir.classes.is_empty());
        assert!(!ir.traits.is_empty());
        assert!(!ir.functions.is_empty());
        assert!(!ir.inheritance.is_empty());
    }

    #[test]
    fn multiple_functions_are_all_extracted() {
        let source = "void a() {}\nvoid b() {}\nvoid c() {}\n";
        let ir = extract_ok(source, "test.dart");
        assert_eq!(ir.functions.len(), 3);
    }
}
