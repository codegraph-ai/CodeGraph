// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for codegraph-php

use codegraph::CodeGraph;
use codegraph_php::{CodeParser, ParserConfig, PhpParser};
use std::path::Path;

fn fixtures_path() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures"))
}

#[test]
fn test_parse_simple_functions() {
    let parser = PhpParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_path = fixtures_path().join("simple.php");
    let result = parser.parse_file(&file_path, &mut graph);

    assert!(result.is_ok(), "Failed to parse simple.php: {:?}", result);
    let file_info = result.unwrap();

    // Should extract 3 functions: hello, add, main
    assert!(
        file_info.functions.len() >= 3,
        "Expected at least 3 functions, got {}",
        file_info.functions.len()
    );
}

#[test]
fn test_parse_classes() {
    let parser = PhpParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_path = fixtures_path().join("classes.php");
    let result = parser.parse_file(&file_path, &mut graph);

    assert!(result.is_ok(), "Failed to parse classes.php: {:?}", result);
    let file_info = result.unwrap();

    // Should extract Person, Animal, Dog classes
    assert!(
        file_info.classes.len() >= 3,
        "Expected at least 3 classes, got {}",
        file_info.classes.len()
    );

    // Should extract methods from the classes
    assert!(
        !file_info.functions.is_empty(),
        "Expected methods to be extracted"
    );
}

#[test]
fn test_parse_traits() {
    let parser = PhpParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_path = fixtures_path().join("traits.php");
    let result = parser.parse_file(&file_path, &mut graph);

    assert!(result.is_ok(), "Failed to parse traits.php: {:?}", result);
    let file_info = result.unwrap();

    // Should extract Loggable, Serializable traits
    assert!(
        file_info.traits.len() >= 2,
        "Expected at least 2 traits, got {}",
        file_info.traits.len()
    );

    // Should extract Logger, DataObject classes
    assert!(
        file_info.classes.len() >= 2,
        "Expected at least 2 classes, got {}",
        file_info.classes.len()
    );
}

#[test]
fn test_parse_interfaces() {
    let parser = PhpParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_path = fixtures_path().join("interfaces.php");
    let result = parser.parse_file(&file_path, &mut graph);

    assert!(
        result.is_ok(),
        "Failed to parse interfaces.php: {:?}",
        result
    );
    let file_info = result.unwrap();

    // Should extract Readable, Writable, ReadWritable, Countable interfaces
    assert!(
        file_info.traits.len() >= 4,
        "Expected at least 4 interfaces/traits, got {}",
        file_info.traits.len()
    );

    // Should extract FileStream, Counter classes
    assert!(
        file_info.classes.len() >= 2,
        "Expected at least 2 classes, got {}",
        file_info.classes.len()
    );
}

#[test]
fn test_parse_namespaces() {
    let parser = PhpParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_path = fixtures_path().join("namespaces.php");
    let result = parser.parse_file(&file_path, &mut graph);

    assert!(
        result.is_ok(),
        "Failed to parse namespaces.php: {:?}",
        result
    );
    let file_info = result.unwrap();

    // Should extract imports (use statements)
    assert!(
        file_info.imports.len() >= 4,
        "Expected at least 4 imports, got {}",
        file_info.imports.len()
    );

    // Should extract AuthController class with namespace
    assert_eq!(file_info.classes.len(), 1);
}

#[test]
fn test_parse_php8_features() {
    let parser = PhpParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_path = fixtures_path().join("php8_features.php");
    let result = parser.parse_file(&file_path, &mut graph);

    assert!(
        result.is_ok(),
        "Failed to parse php8_features.php: {:?}",
        result
    );
    let file_info = result.unwrap();

    // Should extract Status enum, Point, Config, Route, ApiController, ImmutableUser classes
    assert!(
        file_info.classes.len() >= 5,
        "Expected at least 5 classes (including enums), got {}",
        file_info.classes.len()
    );
}

#[test]
fn test_parse_source_directly() {
    let parser = PhpParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"<?php
namespace App;

class Example {
    public function test(): void {
        echo "Hello";
    }
}
"#;

    let result = parser.parse_source(source, Path::new("example.php"), &mut graph);

    assert!(result.is_ok(), "Failed to parse source: {:?}", result);
    let file_info = result.unwrap();

    assert_eq!(file_info.classes.len(), 1);
    assert!(!file_info.functions.is_empty());
}

#[test]
fn test_parser_language() {
    let parser = PhpParser::new();
    assert_eq!(parser.language(), "php");
}

#[test]
fn test_parser_file_extensions() {
    let parser = PhpParser::new();
    assert_eq!(parser.file_extensions(), &[".php"]);
}

#[test]
fn test_parser_can_parse() {
    let parser = PhpParser::new();
    assert!(parser.can_parse(Path::new("index.php")));
    assert!(parser.can_parse(Path::new("src/Controller.php")));
    assert!(!parser.can_parse(Path::new("main.py")));
    assert!(!parser.can_parse(Path::new("index.html")));
}

#[test]
fn test_parse_files_batch() {
    let parser = PhpParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let files = vec![
        fixtures_path().join("simple.php"),
        fixtures_path().join("classes.php"),
    ];

    let result = parser.parse_files(&files, &mut graph);

    assert!(result.is_ok(), "Failed to parse files: {:?}", result);
    let project_info = result.unwrap();

    assert_eq!(project_info.files.len(), 2);
    assert!(project_info.total_functions > 0);
    assert!(project_info.total_classes > 0);
}

#[test]
fn test_parser_metrics() {
    let mut parser = PhpParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    parser.reset_metrics();

    let file_path = fixtures_path().join("simple.php");
    let _ = parser.parse_file(&file_path, &mut graph);

    let metrics = parser.metrics();
    assert_eq!(metrics.files_attempted, 1);
    assert_eq!(metrics.files_succeeded, 1);
    assert_eq!(metrics.files_failed, 0);
}

#[test]
fn test_parser_with_config() {
    let config = ParserConfig::default()
        .with_max_file_size(1024 * 1024) // 1MB
        .with_parallel(false);

    let parser = PhpParser::with_config(config);
    assert_eq!(parser.config().max_file_size, 1024 * 1024);
    assert!(!parser.config().parallel);
}

#[test]
fn test_inheritance_edges() {
    let parser = PhpParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"<?php
class Animal {}
class Dog extends Animal {}
"#;

    let result = parser.parse_source(source, Path::new("test.php"), &mut graph);
    assert!(result.is_ok());

    let file_info = result.unwrap();
    assert_eq!(file_info.classes.len(), 2);

    // Verify we can find both classes
    for class_id in &file_info.classes {
        let node = graph.get_node(*class_id).unwrap();
        assert_eq!(node.node_type, codegraph::NodeType::Class);
    }
}

#[test]
fn test_implements_edges() {
    let parser = PhpParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"<?php
interface Printable {
    public function print(): void;
}

class Document implements Printable {
    public function print(): void {
        echo "Printing...";
    }
}
"#;

    let result = parser.parse_source(source, Path::new("test.php"), &mut graph);
    assert!(result.is_ok());

    let file_info = result.unwrap();
    assert_eq!(file_info.traits.len(), 1); // interface
    assert_eq!(file_info.classes.len(), 1); // class
}

#[test]
fn test_abstract_class() {
    let parser = PhpParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"<?php
abstract class BaseController {
    abstract public function handle(): void;

    public function respond(): void {
        echo "Response";
    }
}
"#;

    let result = parser.parse_source(source, Path::new("test.php"), &mut graph);
    assert!(result.is_ok());

    let file_info = result.unwrap();
    assert_eq!(file_info.classes.len(), 1);

    let class_node = graph.get_node(file_info.classes[0]).unwrap();
    assert_eq!(class_node.properties.get_bool("is_abstract"), Some(true));
}

#[test]
fn test_static_method() {
    let parser = PhpParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"<?php
class Helper {
    public static function format(string $value): string {
        return trim($value);
    }
}
"#;

    let result = parser.parse_source(source, Path::new("test.php"), &mut graph);
    assert!(result.is_ok());

    let file_info = result.unwrap();
    assert!(!file_info.functions.is_empty());

    let func_node = graph.get_node(file_info.functions[0]).unwrap();
    assert_eq!(func_node.properties.get_bool("is_static"), Some(true));
}

#[test]
fn test_visibility_modifiers() {
    let parser = PhpParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"<?php
class Example {
    private function privateMethod(): void {}
    protected function protectedMethod(): void {}
    public function publicMethod(): void {}
}
"#;

    let result = parser.parse_source(source, Path::new("test.php"), &mut graph);
    assert!(result.is_ok());

    let file_info = result.unwrap();
    assert_eq!(file_info.functions.len(), 3);

    let visibilities: Vec<_> = file_info
        .functions
        .iter()
        .filter_map(|id| {
            graph
                .get_node(*id)
                .ok()
                .and_then(|n| n.properties.get_string("visibility").map(|s| s.to_string()))
        })
        .collect();

    assert!(visibilities.contains(&"private".to_string()));
    assert!(visibilities.contains(&"protected".to_string()));
    assert!(visibilities.contains(&"public".to_string()));
}

#[test]
fn test_syntax_error() {
    let source = r#"<?php
function broken( {
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = PhpParser::new();

    let result = parser.parse_source(source, Path::new("test.php"), &mut graph);
    assert!(result.is_err());
}

#[test]
fn test_calls_edges() {
    use codegraph::{Direction, EdgeType};

    let parser = PhpParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"<?php
function helper() {
    return 42;
}

function caller() {
    helper();
}
"#;

    let result = parser.parse_source(source, Path::new("test.php"), &mut graph);
    assert!(result.is_ok());

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

    assert!(
        callees.contains(&"helper".to_string()),
        "Should have Calls edge to 'helper', got: {:?}",
        callees
    );
}
