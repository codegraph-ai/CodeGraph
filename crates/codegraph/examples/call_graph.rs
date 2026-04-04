// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Call graph example demonstrating function call tracking
//!
//! This example shows how to build a call graph to track which functions
//! call which other functions, useful for understanding control flow.

use codegraph::{helpers, CodeGraph};
use std::path::Path;

fn main() -> codegraph::Result<()> {
    let mut graph = CodeGraph::open(Path::new("./call_graph.graph"))?;

    println!("Building a call graph...\n");

    // Add the main file
    let main_file = helpers::add_file(&mut graph, "src/main.rs", "rust")?;
    println!("✓ Added file: src/main.rs");

    // Add functions
    let main_fn = helpers::add_function(&mut graph, main_file, "main", 1, 20)?;
    let process_data = helpers::add_function(&mut graph, main_file, "process_data", 22, 35)?;
    let validate_input = helpers::add_function(&mut graph, main_file, "validate_input", 37, 45)?;
    let save_result = helpers::add_function(&mut graph, main_file, "save_result", 47, 60)?;

    println!("✓ Added 4 functions");

    // Build the call graph:
    // main -> process_data (line 5)
    // main -> save_result (line 15)
    // process_data -> validate_input (line 25)
    // process_data -> save_result (line 32)

    helpers::add_call(&mut graph, main_fn, process_data, 5)?;
    helpers::add_call(&mut graph, main_fn, save_result, 15)?;
    helpers::add_call(&mut graph, process_data, validate_input, 25)?;
    helpers::add_call(&mut graph, process_data, save_result, 32)?;

    println!("✓ Added 4 call relationships\n");

    // Analyze the call graph
    println!("--- Call Graph Analysis ---\n");

    // Find what main() calls
    let main_callees = helpers::get_callees(&graph, main_fn)?;
    println!("main() calls {} functions:", main_callees.len());
    for callee_id in &main_callees {
        let node = graph.get_node(*callee_id)?;
        if let Some(name) = node.properties.get_string("name") {
            println!("  - {name}");
        }
    }

    // Find what process_data() calls
    let process_callees = helpers::get_callees(&graph, process_data)?;
    println!(
        "\nprocess_data() calls {} functions:",
        process_callees.len()
    );
    for callee_id in &process_callees {
        let node = graph.get_node(*callee_id)?;
        if let Some(name) = node.properties.get_string("name") {
            println!("  - {name}");
        }
    }

    // Find who calls save_result()
    let save_callers = helpers::get_callers(&graph, save_result)?;
    println!(
        "\nsave_result() is called by {} functions:",
        save_callers.len()
    );
    for caller_id in &save_callers {
        let node = graph.get_node(*caller_id)?;
        if let Some(name) = node.properties.get_string("name") {
            println!("  - {name}");
        }
    }

    // Find who calls validate_input()
    let validate_callers = helpers::get_callers(&graph, validate_input)?;
    println!(
        "\nvalidate_input() is called by {} functions:",
        validate_callers.len()
    );
    for caller_id in &validate_callers {
        let node = graph.get_node(*caller_id)?;
        if let Some(name) = node.properties.get_string("name") {
            println!("  - {name}");
        }
    }

    // Statistics
    println!("\n--- Statistics ---");
    println!(
        "Total functions: {}",
        helpers::get_functions_in_file(&graph, main_file)?.len()
    );
    println!("Total calls: {}", graph.edge_count());

    // Persist
    graph.flush()?;
    println!("\n✓ Call graph saved to disk");

    Ok(())
}
