// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST extraction for Rust source code using tree-sitter
//!
//! This module parses Rust source code and extracts entities and relationships
//! into a CodeIR representation.

use codegraph_parser_api::{CodeIR, ModuleEntity, ParserConfig, ParserError};
use std::path::Path;
use tree_sitter::Parser;

use crate::visitor::RustVisitor;

/// Extract code entities and relationships from Rust source code
///
/// # Arguments
/// * `source` - Rust source code as a string
/// * `file_path` - Path to the source file
/// * `config` - Parser configuration
///
/// # Returns
/// CodeIR containing all extracted entities and relationships
pub fn extract(
    source: &str,
    file_path: &Path,
    config: &ParserConfig,
) -> Result<CodeIR, ParserError> {
    // Initialize tree-sitter parser
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .map_err(|e| ParserError::ParseError(file_path.to_path_buf(), e.to_string()))?;

    // Parse the source code
    let tree = parser.parse(source, None).ok_or_else(|| {
        ParserError::ParseError(file_path.to_path_buf(), "Failed to parse".to_string())
    })?;

    let root_node = tree.root_node();
    if root_node.has_error() {
        // Find the first error node for better error reporting
        let mut cursor = root_node.walk();
        let error_node = root_node.children(&mut cursor).find(|n| n.is_error());

        if let Some(err) = error_node {
            return Err(ParserError::SyntaxError(
                file_path.to_path_buf(),
                err.start_position().row + 1,
                err.start_position().column,
                "Syntax error".to_string(),
            ));
        }

        return Err(ParserError::SyntaxError(
            file_path.to_path_buf(),
            0,
            0,
            "Syntax error".to_string(),
        ));
    }

    // Create IR for this file
    let mut ir = CodeIR::new(file_path.to_path_buf());

    // Create module entity for the file
    let module_name = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    let module = ModuleEntity {
        name: module_name.clone(),
        path: file_path.display().to_string(),
        language: "rust".to_string(),
        line_count: source.lines().count(),
        doc_comment: extract_file_doc(source, &tree),
        attributes: Vec::new(),
    };

    ir.module = Some(module);

    // Create visitor and walk the AST
    let mut visitor = RustVisitor::new(source.as_bytes(), config.clone());
    visitor.visit_node(root_node);

    // Transfer extracted entities to IR
    ir.functions = visitor.functions;
    ir.classes = visitor.classes;
    ir.traits = visitor.traits;
    ir.imports = visitor.imports;
    ir.calls = visitor.calls;
    ir.implementations = visitor.implementations;
    ir.inheritance = visitor.inheritance;
    ir.type_references = visitor.type_references;

    Ok(ir)
}

/// Extract documentation from the file level (if any)
fn extract_file_doc(source: &str, tree: &tree_sitter::Tree) -> Option<String> {
    let root = tree.root_node();
    let mut cursor = root.walk();
    let mut docs = Vec::new();

    // Look for inner doc comments (//!) at the start of the file
    for child in root.children(&mut cursor) {
        if child.kind() == "line_comment" {
            let text = child.utf8_text(source.as_bytes()).unwrap_or("");
            if let Some(rest) = text.strip_prefix("//!") {
                docs.push(rest.trim().to_string());
            }
        } else if child.kind() != "attribute_item" {
            // Stop looking once we hit non-doc content
            break;
        }
    }

    if docs.is_empty() {
        None
    } else {
        Some(docs.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_simple_function() {
        let source = r#"
fn hello() {
    println!("Hello, world!");
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.rs"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].name, "hello");
        assert_eq!(ir.functions[0].line_start, 2);
    }

    #[test]
    fn test_extract_struct() {
        let source = r#"
pub struct MyStruct {
    field1: String,
    field2: i32,
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.rs"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "MyStruct");
        assert_eq!(ir.classes[0].line_start, 2);
    }

    #[test]
    fn test_extract_trait() {
        let source = r#"
pub trait MyTrait {
    fn method(&self);
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.rs"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.traits.len(), 1);
        assert_eq!(ir.traits[0].name, "MyTrait");
    }

    #[test]
    fn test_syntax_error() {
        let source = "fn hello( { "; // Invalid syntax
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.rs"), &config);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ParserError::SyntaxError(_, _, _, _)
        ));
    }

    #[test]
    fn test_extract_enum() {
        let source = r#"
pub enum Color {
    Red,
    Green,
    Blue,
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.rs"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "Color");
    }

    #[test]
    fn test_extract_impl_block() {
        let source = r#"
struct Calculator;

impl Calculator {
    fn add(&self, a: i32, b: i32) -> i32 {
        a + b
    }
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.rs"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        // Method should be extracted
        assert!(!ir.functions.is_empty() || !ir.classes[0].methods.is_empty());
    }

    #[test]
    fn test_extract_async_function() {
        let source = r#"
async fn fetch_data() -> String {
    "data".to_string()
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.rs"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].name, "fetch_data");
        assert!(ir.functions[0].is_async);
    }

    #[test]
    fn test_extract_use_statement() {
        let source = r#"
use std::collections::HashMap;
use std::fs;

fn test() {}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.rs"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.imports.len(), 2);
    }

    #[test]
    fn test_extract_multiple_entities() {
        let source = r#"
use std::fmt;

pub trait Display {
    fn display(&self) -> String;
}

pub struct Person {
    name: String,
    age: u32,
}

impl Display for Person {
    fn display(&self) -> String {
        format!("{}: {}", self.name, self.age)
    }
}

pub fn greet(p: &Person) {
    println!("{}", p.display());
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.rs"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.traits.len(), 1);
        assert_eq!(ir.traits[0].name, "Display");
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "Person");
        assert!(!ir.functions.is_empty()); // greet function
        assert!(!ir.imports.is_empty()); // use std::fmt
    }

    #[test]
    fn test_extract_generic_struct() {
        let source = r#"
pub struct Container<T> {
    value: T,
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.rs"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "Container");
    }

    #[test]
    fn test_extract_module_info() {
        let source = r#"
fn test() {
    println!("test");
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("module.rs"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert!(ir.module.is_some());
        let module = ir.module.unwrap();
        assert_eq!(module.name, "module");
        assert_eq!(module.language, "rust");
        assert!(module.line_count > 0);
    }

    #[test]
    fn test_extract_visibility_modifiers() {
        let source = r#"
pub fn public_fn() {}
fn private_fn() {}
pub(crate) fn crate_fn() {}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.rs"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.functions.len(), 3);
    }

    #[test]
    fn test_extract_trait_implementation() {
        let source = r#"
pub trait Speak {
    fn speak(&self);
}

pub struct Dog;

impl Speak for Dog {
    fn speak(&self) {
        println!("Woof!");
    }
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.rs"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.traits.len(), 1);
        assert_eq!(ir.classes.len(), 1);
        assert!(!ir.implementations.is_empty() || !ir.classes[0].implemented_traits.is_empty());
    }

    #[test]
    fn test_extract_test_function() {
        let source = r#"
#[test]
fn test_something() {
    assert_eq!(2 + 2, 4);
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.rs"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.functions.len(), 1);
        assert!(ir.functions[0].is_test);
    }

    #[test]
    fn test_accurate_line_numbers() {
        let source = "fn first() {}\n\nfn second() {}";
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.rs"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.functions.len(), 2);
        assert_eq!(ir.functions[0].name, "first");
        assert_eq!(ir.functions[0].line_start, 1);
        assert_eq!(ir.functions[1].name, "second");
        assert_eq!(ir.functions[1].line_start, 3);
    }
}
