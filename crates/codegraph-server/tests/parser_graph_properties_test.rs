// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Graph property integration tests for all parsers.
//!
//! Verifies that parser output written to graph nodes includes expected properties.
//! Catches bugs where extractors compute values but mappers/ir_to_graph skip them.
//!
//! Pattern: parse real source → iterate graph nodes → assert properties exist and are correct.

use codegraph::{CodeGraph, NodeType};
use codegraph_parser_api::{CodeParser, ParserConfig};
use std::path::Path;

// ── Helpers ──

fn graph_with_source(parser: &dyn CodeParser, source: &str, filename: &str) -> CodeGraph {
    let mut graph = CodeGraph::in_memory().expect("graph");
    parser
        .parse_source(source, Path::new(filename), &mut graph)
        .expect("parse failed");
    graph
}

fn find_function<'a>(graph: &'a CodeGraph, name: &str) -> Option<&'a codegraph::Node> {
    graph
        .iter_nodes()
        .find(|(_, n)| {
            n.node_type == NodeType::Function && n.properties.get_string("name") == Some(name)
        })
        .map(|(_, n)| n)
}

fn find_class<'a>(graph: &'a CodeGraph, name: &str) -> Option<&'a codegraph::Node> {
    graph
        .iter_nodes()
        .find(|(_, n)| {
            (n.node_type == NodeType::Class || n.node_type == NodeType::Interface)
                && n.properties.get_string("name") == Some(name)
        })
        .map(|(_, n)| n)
}

/// Assert a function node has core properties that ALL parsers should set.
fn assert_core_function_properties(node: &codegraph::Node, name: &str) {
    assert_eq!(
        node.properties.get_string("name"),
        Some(name),
        "name property mismatch"
    );
    assert!(
        node.properties.get_string("path").is_some(),
        "{}: missing 'path' property",
        name
    );
    assert!(
        node.properties.get_int("line_start").is_some(),
        "{}: missing 'line_start' property",
        name
    );
    assert!(
        node.properties.get_int("line_end").is_some(),
        "{}: missing 'line_end' property",
        name
    );
    assert!(
        node.properties.get_string("visibility").is_some(),
        "{}: missing 'visibility' property",
        name
    );
    assert!(
        node.properties.get_string("signature").is_some(),
        "{}: missing 'signature' property",
        name
    );
}

/// Assert complexity is present and > 1 for a function with branches.
fn assert_has_complexity(node: &codegraph::Node, name: &str) {
    let complexity = node.properties.get_int("complexity");
    assert!(
        complexity.is_some(),
        "{}: missing 'complexity' property",
        name
    );
    assert!(
        complexity.unwrap() > 1,
        "{}: complexity should be > 1 for branching function, got {}",
        name,
        complexity.unwrap()
    );
    assert!(
        node.properties.get_string("complexity_grade").is_some(),
        "{}: missing 'complexity_grade'",
        name
    );
    assert!(
        node.properties.get_int("complexity_branches").is_some(),
        "{}: missing 'complexity_branches'",
        name
    );
}

/// Assert body_prefix is present and non-empty.
fn assert_has_body_prefix(node: &codegraph::Node, name: &str) {
    let body = node.properties.get_string("body_prefix");
    assert!(body.is_some(), "{}: missing 'body_prefix' property", name);
    assert!(
        !body.unwrap().is_empty(),
        "{}: 'body_prefix' is empty",
        name
    );
}

// ── Python ──

#[test]
fn python_graph_properties() {
    let source = r#"
def simple(x):
    return x + 1

def branching(x, y):
    if x > 0:
        for item in y:
            if item > 10:
                return item
    elif x < 0:
        return -1
    else:
        return 0
"#;
    let parser = codegraph_python::PythonParser::new();
    let graph = graph_with_source(&parser, source, "test.py");

    let simple = find_function(&graph, "simple").expect("simple not found");
    assert_core_function_properties(simple, "simple");
    assert_has_body_prefix(simple, "simple");

    let branching = find_function(&graph, "branching").expect("branching not found");
    assert_core_function_properties(branching, "branching");
    assert_has_complexity(branching, "branching");
    assert_has_body_prefix(branching, "branching");
}

// ── Rust ──

#[test]
fn rust_graph_properties() {
    let source = r#"
pub fn simple(x: i32) -> i32 {
    x + 1
}

fn branching(x: i32, items: &[i32]) -> i32 {
    if x > 0 {
        for item in items {
            if *item > 10 {
                return *item;
            }
        }
        0
    } else if x < 0 {
        -1
    } else {
        0
    }
}
"#;
    let parser = codegraph_rust::RustParser::with_config(ParserConfig::default());
    let graph = graph_with_source(&parser, source, "test.rs");

    let simple = find_function(&graph, "simple").expect("simple not found");
    assert_core_function_properties(simple, "simple");
    assert_has_body_prefix(simple, "simple");
    assert_eq!(
        simple.properties.get_string("visibility"),
        Some("public"),
        "simple should be public"
    );

    let branching = find_function(&graph, "branching").expect("branching not found");
    assert_core_function_properties(branching, "branching");
    assert_has_complexity(branching, "branching");
    assert_has_body_prefix(branching, "branching");
}

// ── TypeScript ──

#[test]
fn typescript_graph_properties() {
    let source = r#"
export function simple(x: number): number {
    return x + 1;
}

function branching(x: number, items: number[]): number {
    if (x > 0) {
        for (const item of items) {
            if (item > 10) {
                return item;
            }
        }
        return 0;
    } else if (x < 0) {
        return -1;
    } else {
        return 0;
    }
}

export class MyService {
    getName(): string {
        return "service";
    }
}
"#;
    let parser = codegraph_typescript::TypeScriptParser::with_config(ParserConfig::default());
    let graph = graph_with_source(&parser, source, "test.ts");

    let simple = find_function(&graph, "simple").expect("simple not found");
    assert_core_function_properties(simple, "simple");
    assert_has_body_prefix(simple, "simple");

    let branching = find_function(&graph, "branching").expect("branching not found");
    assert_core_function_properties(branching, "branching");
    assert_has_complexity(branching, "branching");
    assert_has_body_prefix(branching, "branching");

    let class = find_class(&graph, "MyService").expect("MyService not found");
    assert!(
        class.properties.get_string("name") == Some("MyService"),
        "class name mismatch"
    );
}

// ── Go ──

#[test]
fn go_graph_properties() {
    let source = r#"
package main

func simple(x int) int {
    return x + 1
}

func branching(x int, items []int) int {
    if x > 0 {
        for _, item := range items {
            if item > 10 {
                return item
            }
        }
        return 0
    } else if x < 0 {
        return -1
    }
    return 0
}
"#;
    let parser = codegraph_go::GoParser::with_config(ParserConfig::default());
    let graph = graph_with_source(&parser, source, "test.go");

    let simple = find_function(&graph, "simple").expect("simple not found");
    assert_core_function_properties(simple, "simple");
    assert_has_body_prefix(simple, "simple");

    let branching = find_function(&graph, "branching").expect("branching not found");
    assert_core_function_properties(branching, "branching");
    assert_has_complexity(branching, "branching");
    assert_has_body_prefix(branching, "branching");
}

// ── C ──

#[test]
fn c_graph_properties() {
    let source = r#"
int simple(int x) {
    return x + 1;
}

int branching(int x, int *items, int len) {
    if (x > 0) {
        for (int i = 0; i < len; i++) {
            if (items[i] > 10) {
                return items[i];
            }
        }
        return 0;
    } else if (x < 0) {
        return -1;
    } else {
        return 0;
    }
}
"#;
    let parser = codegraph_c::CParser::with_config(ParserConfig::default());
    let graph = graph_with_source(&parser, source, "test.c");

    let simple = find_function(&graph, "simple").expect("simple not found");
    assert_core_function_properties(simple, "simple");
    assert_has_body_prefix(simple, "simple");

    let branching = find_function(&graph, "branching").expect("branching not found");
    assert_core_function_properties(branching, "branching");
    assert_has_complexity(branching, "branching");
    assert_has_body_prefix(branching, "branching");
}

// ── Java ──

#[test]
fn java_graph_properties() {
    let source = r#"
public class App {
    public int simple(int x) {
        return x + 1;
    }

    public int branching(int x, int[] items) {
        if (x > 0) {
            for (int item : items) {
                if (item > 10) {
                    return item;
                }
            }
            return 0;
        } else if (x < 0) {
            return -1;
        } else {
            return 0;
        }
    }
}
"#;
    let parser = codegraph_java::JavaParser::with_config(ParserConfig::default());
    let graph = graph_with_source(&parser, source, "App.java");

    let simple = find_function(&graph, "simple").expect("simple not found");
    assert_core_function_properties(simple, "simple");
    assert_has_body_prefix(simple, "simple");

    let branching = find_function(&graph, "branching").expect("branching not found");
    assert_core_function_properties(branching, "branching");
    assert_has_complexity(branching, "branching");
    assert_has_body_prefix(branching, "branching");

    let class = find_class(&graph, "App").expect("App class not found");
    assert!(class.properties.get_string("name") == Some("App"));
}

// ── C++ ──

#[test]
fn cpp_graph_properties() {
    let source = r#"
int simple(int x) {
    return x + 1;
}

int branching(int x) {
    if (x > 0) {
        return 1;
    } else if (x < 0) {
        return -1;
    } else {
        return 0;
    }
}

class MyClass {
public:
    void doSomething() {}
};
"#;
    let parser = codegraph_cpp::CppParser::with_config(ParserConfig::default());
    let graph = graph_with_source(&parser, source, "test.cpp");

    let simple = find_function(&graph, "simple").expect("simple not found");
    assert_core_function_properties(simple, "simple");
    assert_has_body_prefix(simple, "simple");

    let branching = find_function(&graph, "branching").expect("branching not found");
    assert_has_complexity(branching, "branching");
}

// ── PHP ──

#[test]
fn php_graph_properties() {
    let source = r#"<?php
function simple($x) {
    return $x + 1;
}

function branching($x, $items) {
    if ($x > 0) {
        foreach ($items as $item) {
            if ($item > 10) {
                return $item;
            }
        }
        return 0;
    } elseif ($x < 0) {
        return -1;
    }
    return 0;
}
"#;
    let parser = codegraph_php::PhpParser::with_config(ParserConfig::default());
    let graph = graph_with_source(&parser, source, "test.php");

    let simple = find_function(&graph, "simple").expect("simple not found");
    assert_core_function_properties(simple, "simple");
    assert_has_body_prefix(simple, "simple");

    let branching = find_function(&graph, "branching").expect("branching not found");
    assert_has_complexity(branching, "branching");
}

// ── Ruby ──

#[test]
fn ruby_graph_properties() {
    let source = r#"
def simple(x)
  x + 1
end

def branching(x, items)
  if x > 0
    items.each do |item|
      if item > 10
        return item
      end
    end
    0
  elsif x < 0
    -1
  else
    0
  end
end
"#;
    let parser = codegraph_ruby::RubyParser::with_config(ParserConfig::default());
    let graph = graph_with_source(&parser, source, "test.rb");

    let simple = find_function(&graph, "simple").expect("simple not found");
    assert_core_function_properties(simple, "simple");
    assert_has_body_prefix(simple, "simple");
}

// ── Kotlin ──

#[test]
fn kotlin_graph_properties() {
    let source = r#"
fun simple(x: Int): Int {
    return x + 1
}

fun branching(x: Int, items: List<Int>): Int {
    if (x > 0) {
        for (item in items) {
            if (item > 10) {
                return item
            }
        }
        return 0
    } else if (x < 0) {
        return -1
    } else {
        return 0
    }
}
"#;
    let parser = codegraph_kotlin::KotlinParser::with_config(ParserConfig::default());
    let graph = graph_with_source(&parser, source, "test.kt");

    let simple = find_function(&graph, "simple").expect("simple not found");
    assert_core_function_properties(simple, "simple");
    assert_has_body_prefix(simple, "simple");

    let branching = find_function(&graph, "branching").expect("branching not found");
    assert_has_complexity(branching, "branching");
}

// ── C# ──

#[test]
fn csharp_graph_properties() {
    let source = r#"
public class App {
    public int Simple(int x) {
        return x + 1;
    }

    public int Branching(int x, int[] items) {
        if (x > 0) {
            foreach (var item in items) {
                if (item > 10) return item;
            }
            return 0;
        } else if (x < 0) {
            return -1;
        }
        return 0;
    }
}
"#;
    let parser = codegraph_csharp::CSharpParser::with_config(ParserConfig::default());
    let graph = graph_with_source(&parser, source, "App.cs");

    let simple = find_function(&graph, "Simple").expect("Simple not found");
    assert_core_function_properties(simple, "Simple");
    assert_has_body_prefix(simple, "Simple");

    let branching = find_function(&graph, "Branching").expect("Branching not found");
    assert_has_complexity(branching, "Branching");
}

// ── Swift ──

#[test]
fn swift_graph_properties() {
    let source = r#"
func simple(x: Int) -> Int {
    return x + 1
}

func branching(x: Int, items: [Int]) -> Int {
    if x > 0 {
        for item in items {
            if item > 10 {
                return item
            }
        }
        return 0
    } else if x < 0 {
        return -1
    } else {
        return 0
    }
}
"#;
    let parser = codegraph_swift::SwiftParser::with_config(ParserConfig::default());
    let graph = graph_with_source(&parser, source, "test.swift");

    let simple = find_function(&graph, "simple").expect("simple not found");
    assert_core_function_properties(simple, "simple");
    assert_has_body_prefix(simple, "simple");
}
