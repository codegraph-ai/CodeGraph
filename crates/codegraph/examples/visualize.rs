// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Visualization and export example demonstrating all export formats.
//!
//! This example shows how to export a code graph to various formats:
//! - DOT (Graphviz) for visual diagrams
//! - JSON (D3.js) for web visualization
//! - CSV for data analysis
//! - RDF Triples for semantic web queries

use codegraph::{export::DotOptions, helpers, CodeGraph, EdgeType, NodeType};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

fn main() -> codegraph::Result<()> {
    let mut graph = CodeGraph::open(Path::new("./visualize_demo.graph"))?;

    println!("=== Building Code Graph ===\n");

    // Create a Python project structure
    let main_py = helpers::add_file(&mut graph, "src/main.py", "python")?;
    let utils_py = helpers::add_file(&mut graph, "src/utils.py", "python")?;
    let models_py = helpers::add_file(&mut graph, "src/models.py", "python")?;
    let tests_py = helpers::add_file(&mut graph, "tests/test_main.py", "python")?;

    println!("✓ Added 4 Python files");

    // Add classes
    let _user_class = helpers::add_class(&mut graph, models_py, "User", 1, 20)?;
    let _product_class = helpers::add_class(&mut graph, models_py, "Product", 22, 40)?;

    println!("✓ Added 2 classes");

    // Add functions
    let main_fn = helpers::add_function_with_metadata(
        &mut graph,
        main_py,
        helpers::FunctionMetadata {
            name: "main",
            line_start: 1,
            line_end: 20,
            visibility: "public",
            signature: "def main():",
            is_async: false,
            is_test: false,
        },
    )?;

    let process_fn = helpers::add_function_with_metadata(
        &mut graph,
        main_py,
        helpers::FunctionMetadata {
            name: "process_user",
            line_start: 22,
            line_end: 35,
            visibility: "public",
            signature: "def process_user(user):",
            is_async: false,
            is_test: false,
        },
    )?;

    let validate_fn = helpers::add_function_with_metadata(
        &mut graph,
        utils_py,
        helpers::FunctionMetadata {
            name: "validate_email",
            line_start: 1,
            line_end: 10,
            visibility: "public",
            signature: "def validate_email(email):",
            is_async: false,
            is_test: false,
        },
    )?;

    let format_fn = helpers::add_function_with_metadata(
        &mut graph,
        utils_py,
        helpers::FunctionMetadata {
            name: "format_output",
            line_start: 12,
            line_end: 25,
            visibility: "private",
            signature: "def format_output(data):",
            is_async: false,
            is_test: false,
        },
    )?;

    let test_fn = helpers::add_function_with_metadata(
        &mut graph,
        tests_py,
        helpers::FunctionMetadata {
            name: "test_main",
            line_start: 5,
            line_end: 20,
            visibility: "public",
            signature: "def test_main():",
            is_async: false,
            is_test: true,
        },
    )?;

    println!("✓ Added 5 functions");

    // Build relationships
    helpers::add_call(&mut graph, main_fn, process_fn, 10)?;
    helpers::add_call(&mut graph, process_fn, validate_fn, 25)?;
    helpers::add_call(&mut graph, process_fn, format_fn, 30)?;
    helpers::add_call(&mut graph, test_fn, main_fn, 8)?;

    helpers::add_import(
        &mut graph,
        main_py,
        utils_py,
        vec!["validate_email", "format_output"],
    )?;
    helpers::add_import(&mut graph, main_py, models_py, vec!["User", "Product"])?;
    helpers::add_import(&mut graph, tests_py, main_py, vec!["main"])?;

    println!("✓ Added 7 relationships\n");

    // Export to different formats
    println!("=== Exporting to Multiple Formats ===\n");

    // 1. DOT (Graphviz) - Simple export
    println!("1. DOT (Graphviz) Format:");
    let dot = graph.export_dot()?;
    fs::create_dir_all("output").expect("Failed to create output directory");
    fs::write("output/graph.dot", &dot).expect("Failed to write DOT file");
    println!("   ✓ Saved to output/graph.dot");
    println!("   → Render with: dot -Tpng output/graph.dot -o output/graph.png");
    println!("   Lines: {}\n", dot.lines().count());

    // 2. DOT with custom styling
    println!("2. DOT (Styled) Format:");
    let mut node_colors = HashMap::new();
    node_colors.insert(NodeType::CodeFile, "#FFF3E0".to_string());
    node_colors.insert(NodeType::Function, "#BBDEFB".to_string());
    node_colors.insert(NodeType::Class, "#FFE0B2".to_string());

    let mut edge_colors = HashMap::new();
    edge_colors.insert(EdgeType::Calls, "#F44336".to_string());
    edge_colors.insert(EdgeType::Imports, "#4CAF50".to_string());
    edge_colors.insert(EdgeType::Contains, "#9E9E9E".to_string());

    let mut node_shapes = HashMap::new();
    node_shapes.insert(NodeType::CodeFile, "note".to_string());
    node_shapes.insert(NodeType::Function, "ellipse".to_string());
    node_shapes.insert(NodeType::Class, "component".to_string());

    let options = DotOptions {
        node_colors,
        edge_colors,
        node_shapes,
        rankdir: "TB".to_string(),
        show_properties: vec!["visibility".to_string(), "is_test".to_string()],
    };

    let styled_dot = graph.export_dot_styled(options)?;
    fs::write("output/graph_styled.dot", &styled_dot).expect("Failed to write styled DOT file");
    println!("   ✓ Saved to output/graph_styled.dot");
    println!("   → Includes colors, shapes, and properties");
    println!("   Lines: {}\n", styled_dot.lines().count());

    // 3. JSON (D3.js compatible)
    println!("3. JSON (D3.js) Format:");
    let json = graph.export_json()?;
    fs::write("output/graph.json", &json).expect("Failed to write JSON file");
    println!("   ✓ Saved to output/graph.json");
    println!("   → Use with D3.js force-directed layout");
    println!("   Size: {} bytes\n", json.len());

    // 4. JSON filtered (functions only)
    println!("4. JSON (Filtered - Functions Only):");
    let filtered_json =
        graph.export_json_filtered(|node| node.node_type == NodeType::Function, true)?;
    fs::write("output/functions.json", &filtered_json).expect("Failed to write filtered JSON file");
    println!("   ✓ Saved to output/functions.json");
    println!("   → Only function nodes and their call relationships");
    println!("   Size: {} bytes\n", filtered_json.len());

    // 5. CSV (for data analysis)
    println!("5. CSV Format:");
    graph.export_csv(Path::new("output/nodes.csv"), Path::new("output/edges.csv"))?;
    println!("   ✓ Saved to output/nodes.csv and output/edges.csv");
    println!("   → Import into Excel, pandas, or R for analysis");

    let nodes_csv = fs::read_to_string("output/nodes.csv").expect("Failed to read nodes CSV");
    let edges_csv = fs::read_to_string("output/edges.csv").expect("Failed to read edges CSV");
    println!("   Nodes: {} rows", nodes_csv.lines().count() - 1);
    println!("   Edges: {} rows\n", edges_csv.lines().count() - 1);

    // 6. RDF Triples (for semantic queries)
    println!("6. RDF Triples (N-Triples) Format:");
    let triples = graph.export_triples()?;
    fs::write("output/graph.nt", &triples).expect("Failed to write triples file");
    println!("   ✓ Saved to output/graph.nt");
    println!("   → Query with SPARQL or import into triple store");
    println!("   Triples: {}\n", triples.lines().count());

    // Demonstrate filtered export for large graphs
    println!("=== Filtered Export Example ===\n");
    println!("For large graphs, export subsets:");

    // Export only test files
    let test_files_json = graph.export_json_filtered(
        |node| {
            if let Some(path) = node.properties.get_string("path") {
                path.starts_with("tests/")
            } else {
                false
            }
        },
        true,
    )?;
    fs::write("output/test_files.json", &test_files_json).expect("Failed to write test files JSON");
    println!("   ✓ Saved test files only to output/test_files.json");

    // Export only public functions
    let public_fns = graph
        .query()
        .node_type(NodeType::Function)
        .property("visibility", "public")
        .count()?;
    println!("   → Public functions: {public_fns}");

    // Show statistics
    println!("\n=== Graph Statistics ===\n");
    println!("Total nodes: {}", graph.node_count());
    println!("Total edges: {}", graph.edge_count());

    let files = graph.query().node_type(NodeType::CodeFile).count()?;
    let classes = graph.query().node_type(NodeType::Class).count()?;
    let functions = graph.query().node_type(NodeType::Function).count()?;

    println!("Files: {files}");
    println!("Classes: {classes}");
    println!("Functions: {functions}");

    println!("\n✓ All exports complete! Check the output/ directory.");
    println!("\nVisualization tips:");
    println!("- Use Graphviz: dot -Tsvg output/graph.dot -o output/graph.svg");
    println!("- Use D3.js: Load output/graph.json in force-directed layout");
    println!("- Use Python: pandas.read_csv('output/nodes.csv')");
    println!("- Use SPARQL: Load output/graph.nt into Apache Jena or similar");

    graph.flush()?;
    Ok(())
}
