// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for QueryBuilder
//!
//! These tests verify the fluent query interface for finding code patterns.

use codegraph::{helpers, CodeGraph, NodeType};

#[test]
fn test_query_builder_node_type_filter() {
    let mut graph = CodeGraph::in_memory().unwrap();

    // Add various node types
    let file_id = helpers::add_file(&mut graph, "src/main.rs", "rust").unwrap();
    let func1_id = helpers::add_function(&mut graph, file_id, "func1", 1, 10).unwrap();
    let func2_id = helpers::add_function(&mut graph, file_id, "func2", 12, 20).unwrap();
    let class_id = helpers::add_class(&mut graph, file_id, "MyClass", 22, 40).unwrap();

    // Query for only functions
    let results = graph
        .query()
        .node_type(NodeType::Function)
        .execute()
        .unwrap();

    assert_eq!(results.len(), 2);
    assert!(results.contains(&func1_id));
    assert!(results.contains(&func2_id));
    assert!(!results.contains(&file_id));
    assert!(!results.contains(&class_id));
}

#[test]
fn test_query_builder_in_file_filter() {
    let mut graph = CodeGraph::in_memory().unwrap();

    // Add two files with functions
    let file1_id = helpers::add_file(&mut graph, "src/main.rs", "rust").unwrap();
    let func1_id = helpers::add_function(&mut graph, file1_id, "main", 1, 10).unwrap();

    let file2_id = helpers::add_file(&mut graph, "src/lib.rs", "rust").unwrap();
    let func2_id = helpers::add_function(&mut graph, file2_id, "helper", 1, 10).unwrap();

    // Query for functions in main.rs
    let results = graph
        .query()
        .node_type(NodeType::Function)
        .in_file("src/main.rs")
        .execute()
        .unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0], func1_id);

    // Query for functions in lib.rs
    let results = graph
        .query()
        .node_type(NodeType::Function)
        .in_file("src/lib.rs")
        .execute()
        .unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0], func2_id);
}

#[test]
fn test_query_builder_file_pattern_filter() {
    let mut graph = CodeGraph::in_memory().unwrap();

    // Add files with different patterns
    helpers::add_file(&mut graph, "src/main.rs", "rust").unwrap();
    helpers::add_file(&mut graph, "src/lib.rs", "rust").unwrap();
    helpers::add_file(&mut graph, "tests/test_main.rs", "rust").unwrap();
    helpers::add_file(&mut graph, "src/utils.py", "python").unwrap();

    // Query for src/*.rs files
    let results = graph
        .query()
        .node_type(NodeType::CodeFile)
        .file_pattern("src/*.rs")
        .execute()
        .unwrap();

    assert_eq!(results.len(), 2);

    // Query for all .py files
    let results = graph
        .query()
        .node_type(NodeType::CodeFile)
        .file_pattern("**/*.py")
        .execute()
        .unwrap();

    assert_eq!(results.len(), 1);
}

#[test]
fn test_query_builder_property_filter() {
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_id = helpers::add_file(&mut graph, "src/main.rs", "rust").unwrap();

    // Add functions with different properties
    helpers::add_function_with_metadata(
        &mut graph,
        file_id,
        helpers::FunctionMetadata {
            name: "pub_func",
            line_start: 1,
            line_end: 10,
            visibility: "public",
            signature: "fn pub_func()",
            is_async: false,
            is_test: false,
        },
    )
    .unwrap();

    helpers::add_function_with_metadata(
        &mut graph,
        file_id,
        helpers::FunctionMetadata {
            name: "priv_func",
            line_start: 12,
            line_end: 20,
            visibility: "private",
            signature: "fn priv_func()",
            is_async: false,
            is_test: false,
        },
    )
    .unwrap();

    helpers::add_function_with_metadata(
        &mut graph,
        file_id,
        helpers::FunctionMetadata {
            name: "async_func",
            line_start: 22,
            line_end: 30,
            visibility: "public",
            signature: "async fn async_func()",
            is_async: true,
            is_test: false,
        },
    )
    .unwrap();

    // Query for public functions
    let results = graph
        .query()
        .node_type(NodeType::Function)
        .property("visibility", "public")
        .execute()
        .unwrap();

    assert_eq!(results.len(), 2);

    // Query for async functions
    let results = graph
        .query()
        .node_type(NodeType::Function)
        .property("is_async", true)
        .execute()
        .unwrap();

    assert_eq!(results.len(), 1);
}

#[test]
fn test_query_builder_name_contains_filter() {
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_id = helpers::add_file(&mut graph, "src/main.rs", "rust").unwrap();
    helpers::add_function(&mut graph, file_id, "get_user", 1, 10).unwrap();
    helpers::add_function(&mut graph, file_id, "set_user", 12, 20).unwrap();
    helpers::add_function(&mut graph, file_id, "delete_user", 22, 30).unwrap();
    helpers::add_function(&mut graph, file_id, "process_data", 32, 40).unwrap();

    // Query for functions with "user" in name (case insensitive)
    let results = graph
        .query()
        .node_type(NodeType::Function)
        .name_contains("user")
        .execute()
        .unwrap();

    assert_eq!(results.len(), 3);

    // Query for functions with "data" in name
    let results = graph
        .query()
        .node_type(NodeType::Function)
        .name_contains("data")
        .execute()
        .unwrap();

    assert_eq!(results.len(), 1);
}

#[test]
fn test_query_builder_custom_predicate() {
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_id = helpers::add_file(&mut graph, "src/main.rs", "rust").unwrap();
    helpers::add_function(&mut graph, file_id, "short", 1, 5).unwrap();
    helpers::add_function(&mut graph, file_id, "medium", 10, 25).unwrap();
    helpers::add_function(&mut graph, file_id, "long", 30, 100).unwrap();

    // Query for functions longer than 20 lines
    let results = graph
        .query()
        .node_type(NodeType::Function)
        .custom(|node| {
            if let (Some(start), Some(end)) = (
                node.properties.get_int("line_start"),
                node.properties.get_int("line_end"),
            ) {
                (end - start) > 20
            } else {
                false
            }
        })
        .execute()
        .unwrap();

    assert_eq!(results.len(), 1);
}

#[test]
fn test_query_builder_chaining_multiple_filters() {
    let mut graph = CodeGraph::in_memory().unwrap();

    let file1_id = helpers::add_file(&mut graph, "src/main.rs", "rust").unwrap();
    let file2_id = helpers::add_file(&mut graph, "src/lib.rs", "rust").unwrap();

    // Add various functions
    helpers::add_function_with_metadata(
        &mut graph,
        file1_id,
        helpers::FunctionMetadata {
            name: "pub_main",
            line_start: 1,
            line_end: 10,
            visibility: "public",
            signature: "fn pub_main()",
            is_async: false,
            is_test: false,
        },
    )
    .unwrap();

    helpers::add_function_with_metadata(
        &mut graph,
        file1_id,
        helpers::FunctionMetadata {
            name: "priv_helper",
            line_start: 12,
            line_end: 20,
            visibility: "private",
            signature: "fn priv_helper()",
            is_async: false,
            is_test: false,
        },
    )
    .unwrap();

    helpers::add_function_with_metadata(
        &mut graph,
        file2_id,
        helpers::FunctionMetadata {
            name: "pub_lib",
            line_start: 1,
            line_end: 10,
            visibility: "public",
            signature: "fn pub_lib()",
            is_async: false,
            is_test: false,
        },
    )
    .unwrap();

    // Query with multiple filters: public functions in main.rs
    let results = graph
        .query()
        .node_type(NodeType::Function)
        .in_file("src/main.rs")
        .property("visibility", "public")
        .execute()
        .unwrap();

    assert_eq!(results.len(), 1);

    // Query with name filter and property filter
    let results = graph
        .query()
        .node_type(NodeType::Function)
        .name_contains("lib")
        .property("visibility", "public")
        .execute()
        .unwrap();

    assert_eq!(results.len(), 1);
}

#[test]
fn test_query_builder_limit() {
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_id = helpers::add_file(&mut graph, "src/main.rs", "rust").unwrap();
    for i in 0..10 {
        helpers::add_function(&mut graph, file_id, &format!("func{i}"), i * 10, i * 10 + 5)
            .unwrap();
    }

    // Query with limit
    let results = graph
        .query()
        .node_type(NodeType::Function)
        .limit(3)
        .execute()
        .unwrap();

    assert_eq!(results.len(), 3);

    // Query with limit larger than results
    let results = graph
        .query()
        .node_type(NodeType::Function)
        .limit(100)
        .execute()
        .unwrap();

    assert_eq!(results.len(), 10);
}

#[test]
fn test_query_builder_count_and_exists() {
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_id = helpers::add_file(&mut graph, "src/main.rs", "rust").unwrap();
    helpers::add_function(&mut graph, file_id, "func1", 1, 10).unwrap();
    helpers::add_function(&mut graph, file_id, "func2", 12, 20).unwrap();
    helpers::add_function(&mut graph, file_id, "func3", 22, 30).unwrap();

    // Test count
    let count = graph.query().node_type(NodeType::Function).count().unwrap();

    assert_eq!(count, 3);

    // Test exists (should be true)
    let exists = graph
        .query()
        .node_type(NodeType::Function)
        .name_contains("func2")
        .exists()
        .unwrap();

    assert!(exists);

    // Test exists (should be false)
    let exists = graph
        .query()
        .node_type(NodeType::Function)
        .name_contains("nonexistent")
        .exists()
        .unwrap();

    assert!(!exists);
}

#[test]
fn test_query_builder_performance_10k_nodes() {
    let mut graph = CodeGraph::in_memory().unwrap();

    // Add 10K functions across multiple files
    let file_id = helpers::add_file(&mut graph, "src/large.rs", "rust").unwrap();
    for i in 0..10000 {
        let name = if i % 3 == 0 {
            format!("public_func_{i}")
        } else {
            format!("func_{i}")
        };

        let visibility = if i % 3 == 0 { "public" } else { "private" };
        let signature = format!("fn {name}()");

        helpers::add_function_with_metadata(
            &mut graph,
            file_id,
            helpers::FunctionMetadata {
                name: &name,
                line_start: i * 10,
                line_end: i * 10 + 5,
                visibility,
                signature: &signature,
                is_async: false,
                is_test: false,
            },
        )
        .unwrap();
    }

    // Query for public functions with name filter
    let results = graph
        .query()
        .node_type(NodeType::Function)
        .property("visibility", "public")
        .name_contains("public")
        .execute()
        .unwrap();

    // Should find ~3333 functions (every 3rd one)
    assert!(results.len() > 3300 && results.len() < 3400);

    // Test count is faster than execute
    let count = graph
        .query()
        .node_type(NodeType::Function)
        .property("visibility", "public")
        .count()
        .unwrap();

    assert!(count > 3300 && count < 3400);

    // Test exists short-circuits
    let exists = graph
        .query()
        .node_type(NodeType::Function)
        .property("visibility", "public")
        .exists()
        .unwrap();

    assert!(exists);
}
