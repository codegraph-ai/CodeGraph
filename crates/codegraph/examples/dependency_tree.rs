// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Dependency tree example demonstrating file import tracking
//!
//! This example shows how to track file dependencies (imports) to understand
//! which files depend on which other files.

use codegraph::{helpers, CodeGraph};
use std::path::Path;

fn main() -> codegraph::Result<()> {
    let mut graph = CodeGraph::open(Path::new("./dependency_tree.graph"))?;

    println!("Building a file dependency tree...\n");

    // Create a project structure:
    // main.rs imports utils.rs and model.rs
    // utils.rs imports config.rs
    // model.rs imports utils.rs

    let main_file = helpers::add_file(&mut graph, "src/main.rs", "rust")?;
    let utils_file = helpers::add_file(&mut graph, "src/utils.rs", "rust")?;
    let model_file = helpers::add_file(&mut graph, "src/model.rs", "rust")?;
    let config_file = helpers::add_file(&mut graph, "src/config.rs", "rust")?;

    println!("✓ Added 4 source files");

    // Build the dependency graph with imported symbols
    helpers::add_import(&mut graph, main_file, utils_file, vec!["helper", "process"])?;
    helpers::add_import(
        &mut graph,
        main_file,
        model_file,
        vec!["Person", "Database"],
    )?;
    helpers::add_import(
        &mut graph,
        utils_file,
        config_file,
        vec!["Config", "load_config"],
    )?;
    helpers::add_import(&mut graph, model_file, utils_file, vec!["validate"])?;

    println!("✓ Added 4 import relationships\n");

    // Analyze dependencies
    println!("--- Dependency Analysis ---\n");

    // What does main.rs depend on?
    let main_deps = helpers::get_file_dependencies(&graph, main_file)?;
    println!("main.rs depends on {} files:", main_deps.len());
    for dep_id in &main_deps {
        let node = graph.get_node(*dep_id)?;
        if let Some(path) = node.properties.get_string("path") {
            let edges = graph.get_edges_between(main_file, *dep_id)?;
            if let Ok(edge) = graph.get_edge(edges[0]) {
                if let Some(symbols) = edge.properties.get_string_list("symbols") {
                    println!("  - {} (imports: {})", path, symbols.join(", "));
                }
            }
        }
    }

    // Who depends on utils.rs?
    let utils_dependents = helpers::get_file_dependents(&graph, utils_file)?;
    println!("\nutils.rs is used by {} files:", utils_dependents.len());
    for dep_id in &utils_dependents {
        let node = graph.get_node(*dep_id)?;
        if let Some(path) = node.properties.get_string("path") {
            println!("  - {path}");
        }
    }

    // What does model.rs depend on?
    let model_deps = helpers::get_file_dependencies(&graph, model_file)?;
    println!("\nmodel.rs depends on {} files:", model_deps.len());
    for dep_id in &model_deps {
        let node = graph.get_node(*dep_id)?;
        if let Some(path) = node.properties.get_string("path") {
            println!("  - {path}");
        }
    }

    // Who depends on config.rs?
    let config_dependents = helpers::get_file_dependents(&graph, config_file)?;
    println!("\nconfig.rs is used by {} files:", config_dependents.len());
    for dep_id in &config_dependents {
        let node = graph.get_node(*dep_id)?;
        if let Some(path) = node.properties.get_string("path") {
            println!("  - {path}");
        }
    }

    // Find circular dependencies (model.rs and utils.rs)
    println!("\n--- Circular Dependencies ---");
    let mut circular = Vec::new();
    for file_id in [main_file, utils_file, model_file, config_file] {
        let deps = helpers::get_file_dependencies(&graph, file_id)?;
        for dep_id in deps {
            let reverse_deps = helpers::get_file_dependencies(&graph, dep_id)?;
            if reverse_deps.contains(&file_id) {
                let file = graph.get_node(file_id)?;
                let dep = graph.get_node(dep_id)?;
                if let (Some(file_path), Some(dep_path)) = (
                    file.properties.get_string("path"),
                    dep.properties.get_string("path"),
                ) {
                    circular.push(format!("{file_path} <-> {dep_path}"));
                }
            }
        }
    }

    if circular.is_empty() {
        println!("No circular dependencies found.");
    } else {
        println!("Found {} circular dependencies:", circular.len());
        for circ in circular {
            println!("  - {circ}");
        }
    }

    // Statistics
    println!("\n--- Statistics ---");
    println!("Total files: {}", graph.node_count());
    println!("Total imports: {}", graph.edge_count());

    // Calculate files with no dependencies (leaf nodes)
    let mut leaves = Vec::new();
    for file_id in [main_file, utils_file, model_file, config_file] {
        let deps = helpers::get_file_dependencies(&graph, file_id)?;
        if deps.is_empty() {
            let node = graph.get_node(file_id)?;
            if let Some(path) = node.properties.get_string("path") {
                leaves.push(path.to_string());
            }
        }
    }
    println!("Files with no dependencies: {}", leaves.len());
    for leaf in leaves {
        println!("  - {leaf}");
    }

    // Persist
    graph.flush()?;
    println!("\n✓ Dependency tree saved to disk");

    Ok(())
}
