// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Integration test: parse real source → build indexes → query.
//! Catches bugs where hand-built test graphs don't match parser output.

use codegraph::{CodeGraph, MemoryBackend};
use codegraph_parser_api::{CodeParser, ParserConfig};
use std::path::Path;

fn parse_rust_source(source: &str) -> CodeGraph {
    let mut graph = CodeGraph::in_memory().expect("Failed to create graph");
    let parser = codegraph_rust::RustParser::with_config(ParserConfig::default());
    parser
        .parse_source(source, Path::new("test.rs"), &mut graph)
        .expect("Parse failed");
    graph
}

fn parse_python_source(source: &str) -> CodeGraph {
    let mut graph = CodeGraph::in_memory().expect("Failed to create graph");
    let parser = codegraph_python::PythonParser::new();
    parser
        .parse_source(source, Path::new("test.py"), &mut graph)
        .expect("Parse failed");
    graph
}

/// Verify that find_by_imports with fuzzy matching finds Rust `use` statements.
/// This catches the bug where the LSP defaulted to exact matching:
/// searching "serde" wouldn't match "serde::{Serialize, Deserialize}".
#[tokio::test]
async fn test_rust_find_by_imports_fuzzy() {
    let source = r#"
use serde::{Serialize, Deserialize};
use anyhow::Result;
use std::collections::HashMap;

fn main() {
    let x: HashMap<String, String> = HashMap::new();
}
"#;

    let graph = parse_rust_source(source);
    let graph = std::sync::Arc::new(tokio::sync::RwLock::new(graph));
    let engine = codegraph_server::ai_query::QueryEngine::new(graph.clone());
    engine.build_indexes().await;

    // Fuzzy search for "serde" should match "serde::{Serialize, Deserialize}"
    let options = codegraph_server::ai_query::ImportSearchOptions::new()
        .with_match_mode(codegraph_server::ai_query::ImportMatchMode::Fuzzy);
    let results = engine.find_by_imports("serde", &options).await;
    assert!(
        !results.is_empty(),
        "Fuzzy search for 'serde' should match 'serde::{{Serialize, Deserialize}}'"
    );

    // Fuzzy search for "anyhow" should match "anyhow::Result"
    let results = engine.find_by_imports("anyhow", &options).await;
    assert!(
        !results.is_empty(),
        "Fuzzy search for 'anyhow' should match 'anyhow::Result'"
    );

    // Exact search for "serde" should NOT match (full path is different)
    let exact_options = codegraph_server::ai_query::ImportSearchOptions::new()
        .with_match_mode(codegraph_server::ai_query::ImportMatchMode::Exact);
    let results = engine.find_by_imports("serde", &exact_options).await;
    assert!(
        results.is_empty(),
        "Exact search for 'serde' should not match 'serde::{{Serialize, Deserialize}}'"
    );

    // Prefix search for "std" should match "std::collections::HashMap"
    let prefix_options = codegraph_server::ai_query::ImportSearchOptions::new()
        .with_match_mode(codegraph_server::ai_query::ImportMatchMode::Prefix);
    let results = engine.find_by_imports("std", &prefix_options).await;
    assert!(
        !results.is_empty(),
        "Prefix search for 'std' should match 'std::collections::HashMap'"
    );
}

/// Verify Python import detection works end-to-end.
#[tokio::test]
async fn test_python_find_by_imports_fuzzy() {
    let source = r#"
from fastapi import FastAPI, APIRouter
import os
from typing import Optional

app = FastAPI()
"#;

    let graph = parse_python_source(source);
    let graph = std::sync::Arc::new(tokio::sync::RwLock::new(graph));
    let engine = codegraph_server::ai_query::QueryEngine::new(graph.clone());
    engine.build_indexes().await;

    let options = codegraph_server::ai_query::ImportSearchOptions::new()
        .with_match_mode(codegraph_server::ai_query::ImportMatchMode::Fuzzy);

    let results = engine.find_by_imports("fastapi", &options).await;
    assert!(!results.is_empty(), "Should find files importing 'fastapi'");

    let results = engine.find_by_imports("os", &options).await;
    assert!(!results.is_empty(), "Should find files importing 'os'");
}

/// Verify Python complexity is actually written to graph nodes.
/// Catches the bug where parser_impl.rs didn't write complexity properties.
#[tokio::test]
async fn test_python_complexity_in_graph() {
    let source = r#"
def complex_function(x, data):
    if x > 0:
        for item in data:
            if item.is_valid():
                process(item)
            elif item.is_optional():
                skip(item)
    else:
        while x < 10:
            x += 1
    return x
"#;

    let graph = parse_python_source(source);

    // Find the function node and check complexity
    let mut found_complexity = false;
    for (_node_id, node) in graph.iter_nodes() {
        if node.node_type == codegraph::NodeType::Function {
            if let Some(name) = node.properties.get_string("name") {
                if name == "complex_function" {
                    let complexity = node.properties.get_int("complexity").unwrap_or(0);
                    assert!(
                        complexity > 1,
                        "complex_function should have complexity > 1, got {}",
                        complexity
                    );
                    found_complexity = true;

                    // Also verify breakdown
                    let branches = node.properties.get_int("complexity_branches").unwrap_or(0);
                    assert!(branches > 0, "Should have branches > 0, got {}", branches);

                    let loops = node.properties.get_int("complexity_loops").unwrap_or(0);
                    assert!(loops > 0, "Should have loops > 0, got {}", loops);
                }
            }
        }
    }
    assert!(
        found_complexity,
        "Should find complex_function node with complexity metrics"
    );
}

/// Verify Python HTTP decorator detection writes route properties to graph.
#[tokio::test]
async fn test_python_http_decorator_in_graph() {
    let source = r#"
from fastapi import APIRouter

router = APIRouter()

@router.get("/users/{user_id}")
def get_user(user_id: int):
    return {"user_id": user_id}

@router.post("/users/")
def create_user(name: str):
    return {"name": name}

@router.delete("/users/{user_id}")
def delete_user(user_id: int):
    return {"deleted": True}
"#;

    let graph = parse_python_source(source);

    let mut handlers_found = 0;
    for (_node_id, node) in graph.iter_nodes() {
        if node.node_type == codegraph::NodeType::Function {
            if let Some(method) = node.properties.get_string("http_method") {
                let route = node.properties.get_string("route").unwrap_or("");
                let name = node.properties.get_string("name").unwrap_or("");
                match name {
                    "get_user" => {
                        assert_eq!(method, "GET");
                        assert_eq!(route, "/users/{user_id}");
                    }
                    "create_user" => {
                        assert_eq!(method, "POST");
                        assert_eq!(route, "/users/");
                    }
                    "delete_user" => {
                        assert_eq!(method, "DELETE");
                        assert_eq!(route, "/users/{user_id}");
                    }
                    _ => {}
                }
                handlers_found += 1;
            }
        }
    }
    assert_eq!(handlers_found, 3, "Should find 3 HTTP handlers");
}
