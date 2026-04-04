// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

/// Example of parsing an entire Python project
///
/// This example demonstrates:
/// - Parsing all Python files in a directory recursively
/// - Excluding directories like __pycache__
/// - Collecting statistics across multiple files
/// - Handling errors gracefully
use codegraph::CodeGraph;
use codegraph_python::{Parser, ParserConfig};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create an in-memory graph
    let mut graph = CodeGraph::in_memory()?;

    // Create a parser with configuration
    let config = ParserConfig {
        include_private: true,
        include_tests: true,
        parse_docs: true,
        max_file_size: 10 * 1024 * 1024, // 10MB
        file_extensions: vec!["py".to_string()],
        exclude_dirs: vec![
            "__pycache__".to_string(),
            ".venv".to_string(),
            "venv".to_string(),
        ],
        parallel: false, // Use parallel: true for faster parsing of large projects
        num_threads: None,
    };

    let parser = Parser::with_config(config);

    // Parse a project directory
    let project_path = Path::new("tests/fixtures/test_project");

    println!("Parsing project: {}", project_path.display());
    println!();

    match parser.parse_directory(project_path, &mut graph) {
        Ok(project_info) => {
            println!("✓ Parsing complete!");
            println!();
            println!("Project Statistics:");
            println!("==================");
            println!("  Files parsed:    {}", project_info.files.len());
            println!("  Files failed:    {}", project_info.failed_files.len());
            println!("  Success rate:    {:.1}%", project_info.success_rate());
            println!();
            println!("  Total functions: {}", project_info.total_functions);
            println!("  Total classes:   {}", project_info.total_classes);
            println!("  Total traits:    {}", project_info.total_traits);
            println!("  Total lines:     {}", project_info.total_lines);
            println!();
            println!("  Parse time:      {:?}", project_info.total_time);
            println!("  Avg per file:    {:?}", project_info.avg_parse_time());
            println!();

            if !project_info.files.is_empty() {
                println!("Files parsed:");
                for file_info in &project_info.files {
                    println!("  📄 {}", file_info.file_path.display());
                    println!(
                        "     Functions: {}, Classes: {}, Lines: {}",
                        file_info.functions.len(),
                        file_info.classes.len(),
                        file_info.lines
                    );
                }
                println!();
            }

            if !project_info.failed_files.is_empty() {
                println!("Failed files:");
                for (path, error) in &project_info.failed_files {
                    println!("  ✗ {}", path.display());
                    println!("     Error: {error}");
                }
                println!();
            }

            println!("Graph Statistics:");
            println!("=================");
            println!("  Total nodes: {}", graph.node_count());
            println!("  Total edges: {}", graph.edge_count());
        }
        Err(e) => {
            println!("✗ Failed to parse project!");
            println!();
            println!("Error: {e}");
            return Err(Box::new(e));
        }
    }

    Ok(())
}
