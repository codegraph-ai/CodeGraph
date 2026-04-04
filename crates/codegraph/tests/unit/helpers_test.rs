// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for helper functions
//!
//! These tests verify the convenience functions for common code operations.

use codegraph::{CodeGraph, EdgeType, NodeType};

#[test]
fn test_add_file() {
    let mut graph = CodeGraph::in_memory().unwrap();

    // Add a file using helper function
    let file_id = codegraph::helpers::add_file(&mut graph, "src/main.rs", "rust").unwrap();

    // Verify node was created with correct properties
    let node = graph.get_node(file_id).unwrap();
    assert_eq!(node.node_type, NodeType::CodeFile);
    assert_eq!(node.properties.get_string("path"), Some("src/main.rs"));
    assert_eq!(node.properties.get_string("language"), Some("rust"));
}

#[test]
fn test_add_function() {
    let mut graph = CodeGraph::in_memory().unwrap();

    // First add a file
    let file_id = codegraph::helpers::add_file(&mut graph, "src/lib.rs", "rust").unwrap();

    // Add a function that should auto-link to the file
    let func_id =
        codegraph::helpers::add_function(&mut graph, file_id, "my_function", 10, 20).unwrap();

    // Verify function node was created
    let node = graph.get_node(func_id).unwrap();
    assert_eq!(node.node_type, NodeType::Function);
    assert_eq!(node.properties.get_string("name"), Some("my_function"));
    assert_eq!(node.properties.get_int("line_start"), Some(10));
    assert_eq!(node.properties.get_int("line_end"), Some(20));

    // Verify Contains edge was automatically created
    let edges = graph.get_edges_between(file_id, func_id).unwrap();
    assert_eq!(edges.len(), 1);
    let edge = graph.get_edge(edges[0]).unwrap();
    assert_eq!(edge.edge_type, EdgeType::Contains);
}

#[test]
fn test_add_class() {
    let mut graph = CodeGraph::in_memory().unwrap();

    // Add a file
    let file_id = codegraph::helpers::add_file(&mut graph, "src/model.rs", "rust").unwrap();

    // Add a class that should auto-link to the file
    let class_id = codegraph::helpers::add_class(&mut graph, file_id, "Person", 50, 100).unwrap();

    // Verify class node
    let node = graph.get_node(class_id).unwrap();
    assert_eq!(node.node_type, NodeType::Class);
    assert_eq!(node.properties.get_string("name"), Some("Person"));
    assert_eq!(node.properties.get_int("line_start"), Some(50));
    assert_eq!(node.properties.get_int("line_end"), Some(100));

    // Verify Contains edge
    let edges = graph.get_edges_between(file_id, class_id).unwrap();
    assert_eq!(edges.len(), 1);
    let edge = graph.get_edge(edges[0]).unwrap();
    assert_eq!(edge.edge_type, EdgeType::Contains);
}

#[test]
fn test_add_method() {
    let mut graph = CodeGraph::in_memory().unwrap();

    // Add file and class
    let file_id = codegraph::helpers::add_file(&mut graph, "src/model.rs", "rust").unwrap();
    let class_id = codegraph::helpers::add_class(&mut graph, file_id, "Person", 50, 100).unwrap();

    // Add a method that should link to the class
    let method_id =
        codegraph::helpers::add_method(&mut graph, class_id, "get_name", 55, 60).unwrap();

    // Verify method node
    let node = graph.get_node(method_id).unwrap();
    assert_eq!(node.node_type, NodeType::Function);
    assert_eq!(node.properties.get_string("name"), Some("get_name"));
    assert_eq!(node.properties.get_int("line_start"), Some(55));
    assert_eq!(node.properties.get_int("line_end"), Some(60));

    // Verify Contains edge from class to method
    let edges = graph.get_edges_between(class_id, method_id).unwrap();
    assert_eq!(edges.len(), 1);
    let edge = graph.get_edge(edges[0]).unwrap();
    assert_eq!(edge.edge_type, EdgeType::Contains);
}

#[test]
fn test_add_module() {
    let mut graph = CodeGraph::in_memory().unwrap();

    // Add a module
    let module_id = codegraph::helpers::add_module(&mut graph, "utils", "src/utils.rs").unwrap();

    // Verify module node
    let node = graph.get_node(module_id).unwrap();
    assert_eq!(node.node_type, NodeType::Module);
    assert_eq!(node.properties.get_string("name"), Some("utils"));
    assert_eq!(node.properties.get_string("path"), Some("src/utils.rs"));
}

#[test]
fn test_add_call() {
    let mut graph = CodeGraph::in_memory().unwrap();

    // Add file and two functions
    let file_id = codegraph::helpers::add_file(&mut graph, "src/main.rs", "rust").unwrap();
    let caller_id = codegraph::helpers::add_function(&mut graph, file_id, "main", 1, 10).unwrap();
    let callee_id =
        codegraph::helpers::add_function(&mut graph, file_id, "helper", 12, 20).unwrap();

    // Add a call relationship with line metadata
    let edge_id = codegraph::helpers::add_call(&mut graph, caller_id, callee_id, 5).unwrap();

    // Verify Calls edge with line metadata
    let edge = graph.get_edge(edge_id).unwrap();
    assert_eq!(edge.edge_type, EdgeType::Calls);
    assert_eq!(edge.source_id, caller_id);
    assert_eq!(edge.target_id, callee_id);
    assert_eq!(edge.properties.get_int("line"), Some(5));
}

#[test]
fn test_add_import() {
    let mut graph = CodeGraph::in_memory().unwrap();

    // Add two files
    let file1_id = codegraph::helpers::add_file(&mut graph, "src/main.rs", "rust").unwrap();
    let file2_id = codegraph::helpers::add_file(&mut graph, "src/utils.rs", "rust").unwrap();

    // Add import with symbols
    let edge_id =
        codegraph::helpers::add_import(&mut graph, file1_id, file2_id, vec!["helper", "process"])
            .unwrap();

    // Verify Imports edge with symbols
    let edge = graph.get_edge(edge_id).unwrap();
    assert_eq!(edge.edge_type, EdgeType::Imports);
    assert_eq!(edge.source_id, file1_id);
    assert_eq!(edge.target_id, file2_id);

    let expected = ["helper".to_string(), "process".to_string()];
    assert_eq!(
        edge.properties.get_string_list("symbols"),
        Some(&expected[..])
    );
}

#[test]
fn test_get_callers() {
    let mut graph = CodeGraph::in_memory().unwrap();

    // Create function network
    let file_id = codegraph::helpers::add_file(&mut graph, "src/main.rs", "rust").unwrap();
    let main_id = codegraph::helpers::add_function(&mut graph, file_id, "main", 1, 5).unwrap();
    let helper_id = codegraph::helpers::add_function(&mut graph, file_id, "helper", 7, 12).unwrap();
    let util_id = codegraph::helpers::add_function(&mut graph, file_id, "util", 14, 20).unwrap();

    // main calls helper, util calls helper
    codegraph::helpers::add_call(&mut graph, main_id, helper_id, 3).unwrap();
    codegraph::helpers::add_call(&mut graph, util_id, helper_id, 15).unwrap();

    // Get all callers of helper
    let callers = codegraph::helpers::get_callers(&graph, helper_id).unwrap();
    assert_eq!(callers.len(), 2);
    assert!(callers.contains(&main_id));
    assert!(callers.contains(&util_id));
}

#[test]
fn test_get_callees() {
    let mut graph = CodeGraph::in_memory().unwrap();

    // Create function network
    let file_id = codegraph::helpers::add_file(&mut graph, "src/main.rs", "rust").unwrap();
    let main_id = codegraph::helpers::add_function(&mut graph, file_id, "main", 1, 10).unwrap();
    let helper1_id =
        codegraph::helpers::add_function(&mut graph, file_id, "helper1", 12, 20).unwrap();
    let helper2_id =
        codegraph::helpers::add_function(&mut graph, file_id, "helper2", 22, 30).unwrap();

    // main calls both helpers
    codegraph::helpers::add_call(&mut graph, main_id, helper1_id, 5).unwrap();
    codegraph::helpers::add_call(&mut graph, main_id, helper2_id, 7).unwrap();

    // Get all callees of main
    let callees = codegraph::helpers::get_callees(&graph, main_id).unwrap();
    assert_eq!(callees.len(), 2);
    assert!(callees.contains(&helper1_id));
    assert!(callees.contains(&helper2_id));
}

#[test]
fn test_get_functions_in_file() {
    let mut graph = CodeGraph::in_memory().unwrap();

    // Add file and multiple functions
    let file_id = codegraph::helpers::add_file(&mut graph, "src/lib.rs", "rust").unwrap();
    let func1_id = codegraph::helpers::add_function(&mut graph, file_id, "func1", 1, 5).unwrap();
    let func2_id = codegraph::helpers::add_function(&mut graph, file_id, "func2", 7, 12).unwrap();
    let func3_id = codegraph::helpers::add_function(&mut graph, file_id, "func3", 14, 20).unwrap();

    // Add a class too (should not be included)
    let _class_id = codegraph::helpers::add_class(&mut graph, file_id, "MyClass", 22, 30).unwrap();

    // Get only functions in file
    let functions = codegraph::helpers::get_functions_in_file(&graph, file_id).unwrap();
    assert_eq!(functions.len(), 3);
    assert!(functions.contains(&func1_id));
    assert!(functions.contains(&func2_id));
    assert!(functions.contains(&func3_id));
}

#[test]
fn test_get_file_dependencies() {
    let mut graph = CodeGraph::in_memory().unwrap();

    // Create multiple files
    let main_id = codegraph::helpers::add_file(&mut graph, "src/main.rs", "rust").unwrap();
    let utils_id = codegraph::helpers::add_file(&mut graph, "src/utils.rs", "rust").unwrap();
    let model_id = codegraph::helpers::add_file(&mut graph, "src/model.rs", "rust").unwrap();

    // main imports utils and model
    codegraph::helpers::add_import(&mut graph, main_id, utils_id, vec!["helper"]).unwrap();
    codegraph::helpers::add_import(&mut graph, main_id, model_id, vec!["Person"]).unwrap();

    // Get dependencies of main
    let deps = codegraph::helpers::get_file_dependencies(&graph, main_id).unwrap();
    assert_eq!(deps.len(), 2);
    assert!(deps.contains(&utils_id));
    assert!(deps.contains(&model_id));
}

#[test]
fn test_get_file_dependents() {
    let mut graph = CodeGraph::in_memory().unwrap();

    // Create multiple files
    let utils_id = codegraph::helpers::add_file(&mut graph, "src/utils.rs", "rust").unwrap();
    let main_id = codegraph::helpers::add_file(&mut graph, "src/main.rs", "rust").unwrap();
    let lib_id = codegraph::helpers::add_file(&mut graph, "src/lib.rs", "rust").unwrap();

    // main and lib both import utils
    codegraph::helpers::add_import(&mut graph, main_id, utils_id, vec!["helper"]).unwrap();
    codegraph::helpers::add_import(&mut graph, lib_id, utils_id, vec!["process"]).unwrap();

    // Get dependents of utils (who imports utils)
    let dependents = codegraph::helpers::get_file_dependents(&graph, utils_id).unwrap();
    assert_eq!(dependents.len(), 2);
    assert!(dependents.contains(&main_id));
    assert!(dependents.contains(&lib_id));
}
