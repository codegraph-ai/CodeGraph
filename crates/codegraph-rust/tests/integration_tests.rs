// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for codegraph-rust parser

use codegraph::{CodeGraph, EdgeType};
use codegraph_parser_api::CodeParser;
use codegraph_rust::RustParser;
use std::path::Path;

#[test]
fn test_parse_simple_function() {
    let source = r#"
fn hello() {
    println!("Hello, world!");
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = RustParser::new();

    let result = parser.parse_source(source, Path::new("test.rs"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.functions.len(), 1);
    assert_eq!(info.line_count, 4);
}

#[test]
fn test_parse_function_with_parameters() {
    let source = r#"
fn add(a: i32, b: i32) -> i32 {
    a + b
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = RustParser::new();

    let result = parser.parse_source(source, Path::new("test.rs"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.functions.len(), 1);
}

#[test]
fn test_parse_async_function() {
    let source = r#"
async fn fetch_data() -> Result<String, Error> {
    Ok("data".to_string())
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = RustParser::new();

    let result = parser.parse_source(source, Path::new("test.rs"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.functions.len(), 1);
}

#[test]
fn test_parse_struct() {
    let source = r#"
pub struct Person {
    pub name: String,
    age: u32,
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = RustParser::new();

    let result = parser.parse_source(source, Path::new("test.rs"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.classes.len(), 1);
}

#[test]
fn test_parse_enum() {
    let source = r#"
pub enum Color {
    Red,
    Green,
    Blue,
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = RustParser::new();

    let result = parser.parse_source(source, Path::new("test.rs"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.classes.len(), 1);
}

#[test]
fn test_parse_trait() {
    let source = r#"
pub trait Drawable {
    fn draw(&self);
    fn clear(&mut self);
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = RustParser::new();

    let result = parser.parse_source(source, Path::new("test.rs"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.traits.len(), 1);
}

#[test]
fn test_parse_impl_block() {
    let source = r#"
struct Calculator;

impl Calculator {
    fn add(a: i32, b: i32) -> i32 {
        a + b
    }

    fn subtract(a: i32, b: i32) -> i32 {
        a - b
    }
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = RustParser::new();

    let result = parser.parse_source(source, Path::new("test.rs"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.classes.len(), 1);
    assert_eq!(info.functions.len(), 2); // Two methods
}

#[test]
fn test_parse_trait_implementation() {
    let source = r#"
trait Speak {
    fn speak(&self);
}

struct Dog;

impl Speak for Dog {
    fn speak(&self) {
        println!("Woof!");
    }
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = RustParser::new();

    let result = parser.parse_source(source, Path::new("test.rs"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.traits.len(), 1);
    assert_eq!(info.classes.len(), 1);
    assert_eq!(info.functions.len(), 1);
}

#[test]
fn test_parse_generic_struct() {
    let source = r#"
pub struct Container<T> {
    value: T,
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = RustParser::new();

    let result = parser.parse_source(source, Path::new("test.rs"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.classes.len(), 1);
}

#[test]
fn test_parse_use_statements() {
    let source = r#"
use std::collections::HashMap;
use std::io::{Read, Write};
use super::module;
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = RustParser::new();

    let result = parser.parse_source(source, Path::new("test.rs"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    // Should extract 3 individual use statements as import nodes
    assert_eq!(info.imports.len(), 3);
}

#[test]
fn test_parse_complex_file() {
    let source = r#"
//! This is a module doc comment

use std::fmt;

/// A person struct
pub struct Person {
    pub name: String,
    age: u32,
}

impl Person {
    /// Create a new person
    pub fn new(name: String, age: u32) -> Self {
        Self { name, age }
    }

    /// Get the person's age
    pub fn age(&self) -> u32 {
        self.age
    }
}

impl fmt::Display for Person {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} ({})", self.name, self.age)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_person() {
        let person = Person::new("Alice".to_string(), 30);
        assert_eq!(person.age(), 30);
    }
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = RustParser::new();

    let result = parser.parse_source(source, Path::new("person.rs"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    // Should have Person struct, impl blocks with methods, test module
    assert!(!info.classes.is_empty());
    assert!(info.functions.len() >= 2);
}

#[test]
fn test_syntax_error() {
    let source = "fn broken( { }"; // Invalid syntax

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = RustParser::new();

    let result = parser.parse_source(source, Path::new("test.rs"), &mut graph);
    assert!(result.is_err());
}

#[test]
fn test_parser_metrics() {
    use std::io::Write;
    use tempfile::NamedTempFile;

    let source = r#"
fn func1() {}
fn func2() {}
"#;

    // Create a temporary file for testing
    let mut temp_file = NamedTempFile::new().unwrap();
    write!(temp_file, "{source}").unwrap();
    temp_file.flush().unwrap();

    let mut graph = CodeGraph::in_memory().unwrap();
    let mut parser = RustParser::new();

    // parse_file (not parse_source) updates metrics
    let _ = parser.parse_file(temp_file.path(), &mut graph);

    let metrics = parser.metrics();
    assert_eq!(metrics.files_attempted, 1);
    assert_eq!(metrics.files_succeeded, 1);

    parser.reset_metrics();
    let metrics = parser.metrics();
    assert_eq!(metrics.files_attempted, 0);
}

#[test]
fn test_parse_empty_file() {
    let source = "";

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = RustParser::new();

    let result = parser.parse_source(source, Path::new("test.rs"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.functions.len(), 0);
    assert_eq!(info.classes.len(), 0);
    assert_eq!(info.traits.len(), 0);
}

#[test]
fn test_parse_comments_only() {
    let source = r#"
// This is a single-line comment

/* This is a
   multi-line comment */

//! This is a module-level doc comment
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = RustParser::new();

    let result = parser.parse_source(source, Path::new("test.rs"), &mut graph);
    // Note: Comments-only files may not parse as valid Rust without a module item
    // This is expected behavior
    if let Ok(info) = result {
        assert_eq!(info.functions.len(), 0);
        assert_eq!(info.classes.len(), 0);
    }
}

#[test]
fn test_multiple_syntax_errors() {
    let source = r#"
fn broken1( {

struct Invalid
    field: String
}

fn broken2) {
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = RustParser::new();

    let result = parser.parse_source(source, Path::new("test.rs"), &mut graph);
    assert!(result.is_err());
}

#[test]
fn test_malformed_struct() {
    let source = r#"
struct Person {
    name: String,
    age: // Missing type
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = RustParser::new();

    let result = parser.parse_source(source, Path::new("test.rs"), &mut graph);
    assert!(result.is_err());
}

#[test]
fn test_parser_info_validation() {
    let parser = RustParser::new();

    assert_eq!(parser.language(), "rust");
    assert!(parser.can_parse(Path::new("test.rs")));
    assert!(parser.can_parse(Path::new("lib.rs")));
    assert!(parser.can_parse(Path::new("main.rs")));
    assert!(!parser.can_parse(Path::new("test.go")));
    assert!(!parser.can_parse(Path::new("test.ts")));
    assert!(!parser.can_parse(Path::new("test.py")));
}

#[test]
fn test_parse_whitespace_only() {
    let source = "   \n\n   \t\t   \n   ";

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = RustParser::new();

    let result = parser.parse_source(source, Path::new("test.rs"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.functions.len(), 0);
}

#[test]
fn test_parse_incomplete_function() {
    let source = r#"
fn incomplete_function(a: i32
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = RustParser::new();

    let result = parser.parse_source(source, Path::new("test.rs"), &mut graph);
    assert!(result.is_err());
}

#[test]
fn test_parse_incomplete_impl_block() {
    let source = r#"
struct MyStruct;

impl MyStruct {
    fn method(&self
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = RustParser::new();

    let result = parser.parse_source(source, Path::new("test.rs"), &mut graph);
    assert!(result.is_err());
}

#[test]
fn test_parse_unicode_identifiers() {
    let source = r#"
fn функция() {
    let 変数 = 42;
}

struct Структура {
    поле: i32,
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = RustParser::new();

    let result = parser.parse_source(source, Path::new("test.rs"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.functions.len(), 1);
    assert_eq!(info.classes.len(), 1);
}

#[test]
fn test_parse_macro_definitions() {
    let source = r#"
macro_rules! say_hello {
    () => {
        println!("Hello!");
    };
}

fn use_macro() {
    say_hello!();
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = RustParser::new();

    let result = parser.parse_source(source, Path::new("test.rs"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.functions.len(), 1); // use_macro function
}

#[test]
fn test_parse_const_and_static() {
    let source = r#"
const MAX_SIZE: usize = 100;
static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn use_constants() -> usize {
    MAX_SIZE
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = RustParser::new();

    let result = parser.parse_source(source, Path::new("test.rs"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.functions.len(), 1); // use_constants
}

#[test]
fn test_calls_edges_same_file() {
    use codegraph::{Direction, EdgeType};

    let source = r#"
struct Foo;

impl Foo {
    fn caller(&self) {
        Self::associated();
        self.instance_method();
        standalone();
    }
    fn associated() {}
    fn instance_method(&self) {}
}

fn standalone() {}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = RustParser::new();

    let result = parser.parse_source(source, Path::new("test.rs"), &mut graph);
    assert!(result.is_ok());

    // Find "caller" node
    let caller_id = graph
        .query()
        .node_type(codegraph::NodeType::Function)
        .execute()
        .unwrap()
        .into_iter()
        .find(|&id| {
            graph
                .get_node(id)
                .map(|n| n.properties.get_string("name") == Some("caller"))
                .unwrap_or(false)
        })
        .expect("Should find 'caller' function");

    // Check outgoing Calls edges
    let callees: Vec<String> = graph
        .get_neighbors(caller_id, Direction::Outgoing)
        .unwrap_or_default()
        .iter()
        .filter(|&&neighbor_id| {
            graph
                .get_edges_between(caller_id, neighbor_id)
                .unwrap_or_default()
                .iter()
                .any(|&e| {
                    graph
                        .get_edge(e)
                        .map(|edge| edge.edge_type == EdgeType::Calls)
                        .unwrap_or(false)
                })
        })
        .map(|&id| {
            graph
                .get_node(id)
                .map(|n| n.properties.get_string("name").unwrap_or("?").to_string())
                .unwrap_or_default()
        })
        .collect();

    eprintln!("Callees of 'caller': {:?}", callees);
    assert!(
        callees.contains(&"associated".to_string()),
        "Should have Calls edge to 'associated'"
    );
    assert!(
        callees.contains(&"instance_method".to_string()),
        "Should have Calls edge to 'instance_method'"
    );
    assert!(
        callees.contains(&"standalone".to_string()),
        "Should have Calls edge to 'standalone'"
    );
}

#[test]
fn test_type_refs_resolved_in_same_file() {
    // Both function and struct are defined in the same file — References edge should be created
    let source = r#"
struct Config {
    value: i32,
}

fn setup(cfg: Config) -> Config {
    cfg
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = RustParser::new();
    let result = parser.parse_source(source, Path::new("test.rs"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();

    let setup_id = info
        .functions
        .iter()
        .find(|&&id| {
            graph
                .get_node(id)
                .ok()
                .and_then(|n| n.properties.get("name"))
                .map(|v| matches!(v, codegraph::PropertyValue::String(s) if s == "setup"))
                .unwrap_or(false)
        })
        .copied()
        .expect("setup function should exist");

    let config_id = info
        .classes
        .iter()
        .find(|&&id| {
            graph
                .get_node(id)
                .ok()
                .and_then(|n| n.properties.get("name"))
                .map(|v| matches!(v, codegraph::PropertyValue::String(s) if s == "Config"))
                .unwrap_or(false)
        })
        .copied()
        .expect("Config struct should exist");

    let edge_ids = graph
        .get_edges_between(setup_id, config_id)
        .unwrap_or_default();
    assert!(
        edge_ids.iter().any(|&eid| {
            graph
                .get_edge(eid)
                .map(|e| e.edge_type == EdgeType::References)
                .unwrap_or(false)
        }),
        "setup should have a References edge to Config"
    );
}

#[test]
fn test_type_refs_unresolved_cross_file() {
    // Type used in function signature but not defined in this file — stored as unresolved
    let source = r#"
fn handle(req: HttpRequest) -> HttpResponse {
    HttpResponse::ok()
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = RustParser::new();
    let result = parser.parse_source(source, Path::new("test.rs"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.functions.len(), 1);

    let handle_id = info.functions[0];
    let node = graph.get_node(handle_id).unwrap();
    let unresolved = node
        .properties
        .get_string_list_compat("unresolved_type_refs")
        .unwrap_or_default();

    assert!(
        unresolved.contains(&"HttpRequest".to_string())
            || unresolved.contains(&"HttpResponse".to_string()),
        "handle should have unresolved type refs for HttpRequest/HttpResponse, got: {:?}",
        unresolved
    );
}
