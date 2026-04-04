// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Impact analysis example demonstrating complex queries
//!
//! This example shows how to use the QueryBuilder to perform impact analysis:
//! finding which code might be affected by changes to a specific function or file.

use codegraph::{helpers, CodeGraph, NodeType};
use std::path::Path;

fn main() -> codegraph::Result<()> {
    let mut graph = CodeGraph::open(Path::new("./impact_analysis.graph"))?;

    println!("Building a codebase graph for impact analysis...\n");

    // Create a small codebase structure
    let main_file = helpers::add_file(&mut graph, "src/main.rs", "rust")?;
    let utils_file = helpers::add_file(&mut graph, "src/utils.rs", "rust")?;
    let config_file = helpers::add_file(&mut graph, "src/config.rs", "rust")?;

    println!("✓ Added 3 source files");

    // Add functions to main.rs
    let main_fn = helpers::add_function_with_metadata(
        &mut graph,
        main_file,
        helpers::FunctionMetadata {
            name: "main",
            line_start: 1,
            line_end: 20,
            visibility: "public",
            signature: "fn main()",
            is_async: false,
            is_test: false,
        },
    )?;

    let process_data_fn = helpers::add_function_with_metadata(
        &mut graph,
        main_file,
        helpers::FunctionMetadata {
            name: "process_data",
            line_start: 22,
            line_end: 45,
            visibility: "private",
            signature: "fn process_data(data: &str)",
            is_async: false,
            is_test: false,
        },
    )?;

    // Add functions to utils.rs
    let validate_fn = helpers::add_function_with_metadata(
        &mut graph,
        utils_file,
        helpers::FunctionMetadata {
            name: "validate",
            line_start: 1,
            line_end: 15,
            visibility: "public",
            signature: "pub fn validate(input: &str) -> bool",
            is_async: false,
            is_test: false,
        },
    )?;

    let transform_fn = helpers::add_function_with_metadata(
        &mut graph,
        utils_file,
        helpers::FunctionMetadata {
            name: "transform",
            line_start: 17,
            line_end: 30,
            visibility: "public",
            signature: "pub fn transform(data: String) -> String",
            is_async: false,
            is_test: false,
        },
    )?;

    let helper_fn = helpers::add_function_with_metadata(
        &mut graph,
        utils_file,
        helpers::FunctionMetadata {
            name: "helper",
            line_start: 32,
            line_end: 40,
            visibility: "private",
            signature: "fn helper()",
            is_async: false,
            is_test: false,
        },
    )?;

    // Add functions to config.rs
    let load_config_fn = helpers::add_function_with_metadata(
        &mut graph,
        config_file,
        helpers::FunctionMetadata {
            name: "load_config",
            line_start: 1,
            line_end: 25,
            visibility: "public",
            signature: "pub fn load_config() -> Config",
            is_async: false,
            is_test: false,
        },
    )?;

    println!("✓ Added 6 functions");

    // Build call graph
    helpers::add_call(&mut graph, main_fn, process_data_fn, 5)?;
    helpers::add_call(&mut graph, main_fn, load_config_fn, 3)?;
    helpers::add_call(&mut graph, process_data_fn, validate_fn, 25)?;
    helpers::add_call(&mut graph, process_data_fn, transform_fn, 30)?;
    helpers::add_call(&mut graph, transform_fn, helper_fn, 20)?;

    println!("✓ Added 5 call relationships\n");

    // Add file dependencies
    helpers::add_import(
        &mut graph,
        main_file,
        utils_file,
        vec!["validate", "transform"],
    )?;
    helpers::add_import(&mut graph, main_file, config_file, vec!["load_config"])?;

    println!("✓ Added 2 import relationships\n");

    // --- Impact Analysis Queries ---

    println!("=== Impact Analysis ===\n");

    // 1. Find all public functions (API surface)
    println!("1. PUBLIC API SURFACE:");
    let public_fns = graph
        .query()
        .node_type(NodeType::Function)
        .property("visibility", "public")
        .execute()?;

    println!("   Found {} public functions:", public_fns.len());
    for func_id in &public_fns {
        let node = graph.get_node(*func_id)?;
        if let Some(name) = node.properties.get_string("name") {
            if let Some(sig) = node.properties.get_string("signature") {
                println!("   - {name} [{sig}]");
            }
        }
    }

    // 2. Find large functions (potential refactoring candidates)
    println!("\n2. LARGE FUNCTIONS (>20 lines):");
    let large_fns = graph
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
        .execute()?;

    println!("   Found {} large functions:", large_fns.len());
    for func_id in &large_fns {
        let node = graph.get_node(*func_id)?;
        if let (Some(name), Some(start), Some(end)) = (
            node.properties.get_string("name"),
            node.properties.get_int("line_start"),
            node.properties.get_int("line_end"),
        ) {
            println!("   - {} ({} lines)", name, end - start + 1);
        }
    }

    // 3. Find all functions in utils.rs that are called
    println!("\n3. FUNCTIONS IN utils.rs:");
    let utils_fns = graph
        .query()
        .node_type(NodeType::Function)
        .in_file("src/utils.rs")
        .execute()?;

    println!("   Found {} functions in utils.rs:", utils_fns.len());
    for func_id in &utils_fns {
        let node = graph.get_node(*func_id)?;
        if let Some(name) = node.properties.get_string("name") {
            let callers = helpers::get_callers(&graph, *func_id)?;
            println!("   - {} (called by {} functions)", name, callers.len());
        }
    }

    // 4. Impact analysis: What would be affected if we change 'validate'?
    println!("\n4. IMPACT ANALYSIS: Changing 'validate' function:");
    let validate_callers = helpers::get_callers(&graph, validate_fn)?;
    println!("   Direct callers: {}", validate_callers.len());

    for caller_id in &validate_callers {
        let caller = graph.get_node(*caller_id)?;
        if let Some(name) = caller.properties.get_string("name") {
            println!("   - {name}");

            // Find transitive callers (who calls the caller?)
            let transitive = helpers::get_callers(&graph, *caller_id)?;
            for trans_id in &transitive {
                let trans = graph.get_node(*trans_id)?;
                if let Some(trans_name) = trans.properties.get_string("name") {
                    println!("     └─> {trans_name}");
                }
            }
        }
    }

    // 5. Find all Rust files
    println!("\n5. SOURCE FILES:");
    let rust_files = graph
        .query()
        .node_type(NodeType::CodeFile)
        .file_pattern("src/*.rs")
        .execute()?;

    println!("   Found {} Rust files in src/:", rust_files.len());
    for file_id in &rust_files {
        let node = graph.get_node(*file_id)?;
        if let Some(path) = node.properties.get_string("path") {
            let funcs = helpers::get_functions_in_file(&graph, *file_id)?;
            let deps = helpers::get_file_dependencies(&graph, *file_id)?;
            println!(
                "   - {} ({} functions, {} dependencies)",
                path,
                funcs.len(),
                deps.len()
            );
        }
    }

    // 6. Check if specific patterns exist
    println!("\n6. PATTERN CHECKS:");

    let has_async = graph
        .query()
        .node_type(NodeType::Function)
        .property("is_async", true)
        .exists()?;
    println!("   Has async functions: {has_async}");

    let has_tests = graph
        .query()
        .node_type(NodeType::Function)
        .property("is_test", true)
        .exists()?;
    println!("   Has test functions: {has_tests}");

    let has_config = graph
        .query()
        .node_type(NodeType::Function)
        .name_contains("config")
        .exists()?;
    println!("   Has config-related functions: {has_config}");

    // 7. Count queries (optimized - no allocation)
    println!("\n7. STATISTICS:");
    let total_fns = graph.query().node_type(NodeType::Function).count()?;

    let public_count = graph
        .query()
        .node_type(NodeType::Function)
        .property("visibility", "public")
        .count()?;

    let private_count = graph
        .query()
        .node_type(NodeType::Function)
        .property("visibility", "private")
        .count()?;

    println!("   Total functions: {total_fns}");
    println!(
        "   Public: {} ({:.1}%)",
        public_count,
        (public_count as f64 / total_fns as f64) * 100.0
    );
    println!(
        "   Private: {} ({:.1}%)",
        private_count,
        (private_count as f64 / total_fns as f64) * 100.0
    );

    // Persist
    graph.flush()?;
    println!("\n✓ Analysis complete and saved to disk");

    Ok(())
}
