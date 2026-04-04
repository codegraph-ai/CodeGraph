// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for the Swift parser

use codegraph::CodeGraph;
use codegraph_parser_api::CodeParser;
use codegraph_swift::SwiftParser;
use std::path::Path;

fn get_fixtures_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

#[test]
fn test_parse_simple_file() {
    let parser = SwiftParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let fixture_path = get_fixtures_dir().join("simple.swift");
    let result = parser.parse_file(&fixture_path, &mut graph);

    assert!(
        result.is_ok(),
        "Failed to parse simple.swift: {:?}",
        result.err()
    );
    let file_info = result.unwrap();

    // Should have 1 class (Person)
    assert_eq!(file_info.classes.len(), 1, "Expected 1 class");

    // Should have at least 2 functions (init, greet, greetWorld)
    assert!(
        file_info.functions.len() >= 2,
        "Expected at least 2 functions"
    );

    // Should have 1 import (Foundation)
    assert_eq!(file_info.imports.len(), 1, "Expected 1 import");
}

#[test]
fn test_parse_shapes_file() {
    let parser = SwiftParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let fixture_path = get_fixtures_dir().join("shapes.swift");
    let result = parser.parse_file(&fixture_path, &mut graph);

    assert!(
        result.is_ok(),
        "Failed to parse shapes.swift: {:?}",
        result.err()
    );
    let file_info = result.unwrap();

    // Should have classes: Shape, Circle, Rectangle + struct Container + enum Color = 5
    assert!(
        file_info.classes.len() >= 5,
        "Expected at least 5 classes/structs/enums, got {}",
        file_info.classes.len()
    );

    // Should have 2 protocols (Drawable, Resizable)
    assert_eq!(
        file_info.traits.len(),
        2,
        "Expected 2 protocols, got {}",
        file_info.traits.len()
    );

    // Should have 1 import (Foundation)
    assert_eq!(file_info.imports.len(), 1, "Expected 1 import");

    // Check metrics
    let metrics = parser.metrics();
    assert_eq!(metrics.files_succeeded, 1, "Expected 1 successful file");
}

#[test]
fn test_parse_source_directly() {
    let parser = SwiftParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"
import UIKit

class ViewController: UIViewController {
    override func viewDidLoad() {
        super.viewDidLoad()
    }
}
"#;

    let result = parser.parse_source(source, Path::new("ViewController.swift"), &mut graph);
    assert!(result.is_ok());

    let file_info = result.unwrap();
    assert_eq!(file_info.classes.len(), 1);
    assert_eq!(file_info.imports.len(), 1);
}

#[test]
fn test_parse_protocols_and_extensions() {
    let parser = SwiftParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"
protocol DataProvider {
    func fetchData() -> [String]
}

class NetworkProvider: DataProvider {
    func fetchData() -> [String] {
        return ["data"]
    }
}

extension NetworkProvider {
    func clearCache() {
        // Clear cache
    }
}
"#;

    let result = parser.parse_source(source, Path::new("provider.swift"), &mut graph);
    assert!(result.is_ok());

    let file_info = result.unwrap();
    assert_eq!(file_info.traits.len(), 1, "Expected 1 protocol");
    // NetworkProvider class (extensions don't create new class entries but may add methods)
    assert!(!file_info.classes.is_empty(), "Expected at least 1 class");
}

#[test]
fn test_parse_generics() {
    let parser = SwiftParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"
class Stack<Element> {
    private var items: [Element] = []

    func push(_ item: Element) {
        items.append(item)
    }

    func pop() -> Element? {
        return items.popLast()
    }
}

struct Pair<A, B> {
    var first: A
    var second: B
}
"#;

    let result = parser.parse_source(source, Path::new("generics.swift"), &mut graph);
    assert!(result.is_ok());

    let file_info = result.unwrap();
    assert_eq!(file_info.classes.len(), 2, "Expected 2 generic types");
}

#[test]
fn test_parse_enums() {
    let parser = SwiftParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"
enum Direction {
    case north
    case south
    case east
    case west
}

enum Result<T, E> {
    case success(T)
    case failure(E)
}
"#;

    let result = parser.parse_source(source, Path::new("enums.swift"), &mut graph);
    assert!(result.is_ok());

    let file_info = result.unwrap();
    assert_eq!(file_info.classes.len(), 2, "Expected 2 enums");
}

#[test]
fn test_parser_can_parse() {
    let parser = SwiftParser::new();

    assert!(parser.can_parse(Path::new("main.swift")));
    assert!(parser.can_parse(Path::new("ViewController.swift")));
    assert!(parser.can_parse(Path::new("/path/to/file.swift")));

    assert!(!parser.can_parse(Path::new("main.rs")));
    assert!(!parser.can_parse(Path::new("main.py")));
    assert!(!parser.can_parse(Path::new("main.java")));
}

#[test]
fn test_parser_language_info() {
    let parser = SwiftParser::new();

    assert_eq!(parser.language(), "swift");
    assert!(parser.file_extensions().contains(&".swift"));
}

#[test]
fn test_parse_async_functions() {
    let parser = SwiftParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"
func fetchData() async throws -> Data {
    // Async function
    return Data()
}

class NetworkClient {
    func request(url: String) async -> Response {
        // Async method
        return Response()
    }
}
"#;

    let result = parser.parse_source(source, Path::new("async.swift"), &mut graph);
    assert!(result.is_ok());

    let file_info = result.unwrap();
    // Should find at least the free function and the class method
    assert!(
        !file_info.functions.is_empty(),
        "Expected at least 1 function"
    );
}

#[test]
fn test_parse_inheritance() {
    let parser = SwiftParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"
class Animal {
    func speak() {
        print("...")
    }
}

class Dog: Animal {
    override func speak() {
        print("Woof!")
    }
}

class Cat: Animal {
    override func speak() {
        print("Meow!")
    }
}
"#;

    let result = parser.parse_source(source, Path::new("inheritance.swift"), &mut graph);
    assert!(result.is_ok());

    let file_info = result.unwrap();
    assert_eq!(file_info.classes.len(), 3, "Expected 3 classes");
}

#[test]
fn test_syntax_error() {
    let source = r#"
func broken( {
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = SwiftParser::new();

    let result = parser.parse_source(source, Path::new("test.swift"), &mut graph);
    assert!(result.is_err());
}

#[test]
fn test_calls_edges() {
    use codegraph::{Direction, EdgeType};

    let parser = SwiftParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"
func helper() -> Int {
    return 42
}

func caller() {
    helper()
}
"#;

    let result = parser.parse_source(source, Path::new("test.swift"), &mut graph);
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
