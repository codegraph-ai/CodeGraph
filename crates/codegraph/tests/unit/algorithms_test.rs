// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for graph algorithms (TDD - written FIRST)
//!
//! Tests cover:
//! - T126: BFS traversal with max depth
//! - T127: DFS traversal (iterative)
//! - T128: Tarjan's SCC (cycle detection)
//! - T129: Path finding between nodes
//! - T130: transitive_dependencies()
//! - T131: transitive_dependents()
//! - T132: call_chain()
//! - T133: circular_deps()
//! - T134: Cycle detection with actual circular imports

use codegraph::{helpers, CodeGraph};

// Helper to create a linear dependency chain: A -> B -> C -> D
fn create_linear_chain() -> codegraph::Result<(CodeGraph, Vec<u64>)> {
    let mut graph = CodeGraph::in_memory()?;

    let a = helpers::add_file(&mut graph, "a.py", "python")?;
    let b = helpers::add_file(&mut graph, "b.py", "python")?;
    let c = helpers::add_file(&mut graph, "c.py", "python")?;
    let d = helpers::add_file(&mut graph, "d.py", "python")?;

    helpers::add_import(&mut graph, a, b, vec![])?;
    helpers::add_import(&mut graph, b, c, vec![])?;
    helpers::add_import(&mut graph, c, d, vec![])?;

    Ok((graph, vec![a, b, c, d]))
}

// Helper to create a circular dependency: A -> B -> C -> A
fn create_circular_chain() -> codegraph::Result<(CodeGraph, Vec<u64>)> {
    let mut graph = CodeGraph::in_memory()?;

    let a = helpers::add_file(&mut graph, "a.py", "python")?;
    let b = helpers::add_file(&mut graph, "b.py", "python")?;
    let c = helpers::add_file(&mut graph, "c.py", "python")?;

    helpers::add_import(&mut graph, a, b, vec![])?;
    helpers::add_import(&mut graph, b, c, vec![])?;
    helpers::add_import(&mut graph, c, a, vec![])?; // Creates cycle

    Ok((graph, vec![a, b, c]))
}

// T126: Test BFS traversal with max depth
#[test]
fn test_bfs_traversal_with_max_depth() {
    let (graph, nodes) = create_linear_chain().unwrap();
    let [a, b, c, d] = [nodes[0], nodes[1], nodes[2], nodes[3]];

    // BFS from A with depth 1 should find only B
    let result = graph
        .bfs(a, codegraph::Direction::Outgoing, Some(1))
        .unwrap();
    assert_eq!(result.len(), 1);
    assert!(result.contains(&b));

    // BFS from A with depth 2 should find B and C
    let result = graph
        .bfs(a, codegraph::Direction::Outgoing, Some(2))
        .unwrap();
    assert_eq!(result.len(), 2);
    assert!(result.contains(&b));
    assert!(result.contains(&c));

    // BFS from A with depth 3 should find B, C, and D
    let result = graph
        .bfs(a, codegraph::Direction::Outgoing, Some(3))
        .unwrap();
    assert_eq!(result.len(), 3);
    assert!(result.contains(&b));
    assert!(result.contains(&c));
    assert!(result.contains(&d));

    // BFS with unlimited depth
    let result = graph.bfs(a, codegraph::Direction::Outgoing, None).unwrap();
    assert_eq!(result.len(), 3);
}

// T127: Test DFS traversal
#[test]
fn test_dfs_traversal() {
    let (graph, nodes) = create_linear_chain().unwrap();
    let [a, _, _, _] = [nodes[0], nodes[1], nodes[2], nodes[3]];

    // DFS from A should visit all reachable nodes
    let result = graph
        .dfs(a, codegraph::Direction::Outgoing, Some(10))
        .unwrap();
    assert_eq!(result.len(), 3); // B, C, D

    // DFS with depth limit
    let result = graph
        .dfs(a, codegraph::Direction::Outgoing, Some(1))
        .unwrap();
    assert_eq!(result.len(), 1); // Only B
}

// T128: Test Tarjan's SCC (cycle detection)
#[test]
fn test_tarjans_scc_cycle_detection() {
    let (graph, nodes) = create_circular_chain().unwrap();
    let [a, b, c] = [nodes[0], nodes[1], nodes[2]];

    // Should detect the strongly connected component
    let sccs = graph.find_strongly_connected_components().unwrap();

    // Should find at least one SCC with all three nodes
    let found_cycle = sccs
        .iter()
        .any(|scc| scc.len() == 3 && scc.contains(&a) && scc.contains(&b) && scc.contains(&c));

    assert!(
        found_cycle,
        "Should detect the circular dependency as an SCC"
    );
}

// T129: Test path finding between nodes
#[test]
fn test_find_all_paths() {
    let (graph, nodes) = create_linear_chain().unwrap();
    let [a, _, _, d] = [nodes[0], nodes[1], nodes[2], nodes[3]];

    // Find all paths from A to D
    let paths = graph.find_all_paths(a, d, Some(5)).unwrap();

    // Should find exactly one path: A -> B -> C -> D
    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0].len(), 4); // A, B, C, D
    assert_eq!(paths[0][0], a);
    assert_eq!(paths[0][3], d);
}

// T130: Test transitive_dependencies()
#[test]
fn test_transitive_dependencies() {
    let (graph, nodes) = create_linear_chain().unwrap();
    let [a, b, c, d] = [nodes[0], nodes[1], nodes[2], nodes[3]];

    // Transitive dependencies of A should be [B, C, D]
    let deps = helpers::transitive_dependencies(&graph, a, Some(10)).unwrap();
    assert_eq!(deps.len(), 3);
    assert!(deps.contains(&b));
    assert!(deps.contains(&c));
    assert!(deps.contains(&d));

    // With depth limit 1, should only get B
    let deps = helpers::transitive_dependencies(&graph, a, Some(1)).unwrap();
    assert_eq!(deps.len(), 1);
    assert!(deps.contains(&b));

    // Transitive dependencies of D should be empty
    let deps = helpers::transitive_dependencies(&graph, d, Some(10)).unwrap();
    assert_eq!(deps.len(), 0);
}

// T131: Test transitive_dependents()
#[test]
fn test_transitive_dependents() {
    let (graph, nodes) = create_linear_chain().unwrap();
    let [a, b, c, d] = [nodes[0], nodes[1], nodes[2], nodes[3]];

    // Transitive dependents of D should be [C, B, A]
    let dependents = helpers::transitive_dependents(&graph, d, Some(10)).unwrap();
    assert_eq!(dependents.len(), 3);
    assert!(dependents.contains(&a));
    assert!(dependents.contains(&b));
    assert!(dependents.contains(&c));

    // With depth limit 1, should only get C
    let dependents = helpers::transitive_dependents(&graph, d, Some(1)).unwrap();
    assert_eq!(dependents.len(), 1);
    assert!(dependents.contains(&c));

    // Transitive dependents of A should be empty
    let dependents = helpers::transitive_dependents(&graph, a, Some(10)).unwrap();
    assert_eq!(dependents.len(), 0);
}

// T132: Test call_chain() - paths between functions
#[test]
fn test_call_chain() {
    let mut graph = CodeGraph::in_memory().unwrap();

    let file = helpers::add_file(&mut graph, "main.py", "python").unwrap();
    let func_a = helpers::add_function_with_metadata(
        &mut graph,
        file,
        helpers::FunctionMetadata {
            name: "func_a",
            line_start: 1,
            line_end: 5,
            visibility: "public",
            signature: "def func_a():",
            is_async: false,
            is_test: false,
        },
    )
    .unwrap();
    let func_b = helpers::add_function_with_metadata(
        &mut graph,
        file,
        helpers::FunctionMetadata {
            name: "func_b",
            line_start: 7,
            line_end: 11,
            visibility: "public",
            signature: "def func_b():",
            is_async: false,
            is_test: false,
        },
    )
    .unwrap();
    let func_c = helpers::add_function_with_metadata(
        &mut graph,
        file,
        helpers::FunctionMetadata {
            name: "func_c",
            line_start: 13,
            line_end: 17,
            visibility: "public",
            signature: "def func_c():",
            is_async: false,
            is_test: false,
        },
    )
    .unwrap();

    // Create call chain: func_a -> func_b -> func_c
    helpers::add_call(&mut graph, func_a, func_b, 3).unwrap();
    helpers::add_call(&mut graph, func_b, func_c, 9).unwrap();

    // Find call chain from func_a to func_c
    let chains = helpers::call_chain(&graph, func_a, func_c, Some(5)).unwrap();

    assert_eq!(chains.len(), 1);
    assert_eq!(chains[0].len(), 3); // func_a, func_b, func_c
    assert_eq!(chains[0][0], func_a);
    assert_eq!(chains[0][1], func_b);
    assert_eq!(chains[0][2], func_c);
}

// T133: Test circular_deps() - detect file import cycles
#[test]
fn test_circular_deps() {
    let (graph, nodes) = create_circular_chain().unwrap();
    let [a, b, c] = [nodes[0], nodes[1], nodes[2]];

    // Should detect circular dependencies
    let cycles = helpers::circular_deps(&graph).unwrap();

    // Should find at least one cycle containing A, B, and C
    let found_cycle = cycles.iter().any(|cycle| {
        cycle.len() >= 3 && cycle.contains(&a) && cycle.contains(&b) && cycle.contains(&c)
    });

    assert!(found_cycle, "Should detect the circular import");
}

// T134: Test cycle detection with actual circular imports
#[test]
fn test_actual_circular_imports() {
    let mut graph = CodeGraph::in_memory().unwrap();

    // Create a more complex circular dependency scenario
    let utils = helpers::add_file(&mut graph, "utils.py", "python").unwrap();
    let models = helpers::add_file(&mut graph, "models.py", "python").unwrap();
    let views = helpers::add_file(&mut graph, "views.py", "python").unwrap();

    // utils -> models -> views -> utils (circular)
    helpers::add_import(&mut graph, utils, models, vec!["Model"]).unwrap();
    helpers::add_import(&mut graph, models, views, vec!["render"]).unwrap();
    helpers::add_import(&mut graph, views, utils, vec!["helper"]).unwrap();

    // Detect cycles
    let cycles = helpers::circular_deps(&graph).unwrap();

    // Should detect the cycle
    assert!(!cycles.is_empty(), "Should detect circular imports");

    // Verify the cycle contains all three files
    let found = cycles
        .iter()
        .any(|cycle| cycle.contains(&utils) && cycle.contains(&models) && cycle.contains(&views));
    assert!(found, "Cycle should include all three files");

    // Test that transitive_dependencies handles cycles gracefully
    let deps = helpers::transitive_dependencies(&graph, utils, None).unwrap();
    // Should find models and views but not loop infinitely
    assert!(deps.contains(&models));
    assert!(deps.contains(&views));
    assert_eq!(deps.len(), 2); // Should not include utils itself or duplicates
}

// Additional test: BFS should handle cycles without infinite loop
#[test]
fn test_bfs_handles_cycles() {
    let (graph, nodes) = create_circular_chain().unwrap();
    let a = nodes[0];

    // BFS should complete without infinite loop
    let result = graph.bfs(a, codegraph::Direction::Outgoing, None).unwrap();

    // Should visit all nodes exactly once
    assert_eq!(result.len(), 2); // B and C (not A itself)
}
