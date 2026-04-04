// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

/// Basic example of parsing a single Python file
///
/// This example demonstrates:
/// - Creating a code graph
/// - Parsing a Python file
/// - Extracting entities and relationships
/// - Reporting parse results
use codegraph::CodeGraph;
use codegraph_python::{ParseError, Parser};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create an in-memory graph
    let mut graph = CodeGraph::in_memory()?;

    // Create a parser with default configuration
    let parser = Parser::new();

    // Parse a Python file
    let file_path = Path::new("tests/fixtures/simple.py");

    println!("Parsing {}...", file_path.display());

    match parser.parse_file(file_path, &mut graph) {
        Ok(file_info) => {
            println!("✓ Successfully parsed!");
            println!();
            println!("Results:");
            println!("  Functions: {}", file_info.functions.len());
            println!("  Classes:   {}", file_info.classes.len());
            println!("  Modules:   {}", file_info.modules.len());
            println!("  Traits:    {}", file_info.traits.len());
            println!("  Lines:     {}", file_info.lines);
            println!("  Parse time: {:?}", file_info.parse_time);
            println!();
            println!("Entities found:");

            if !file_info.functions.is_empty() {
                println!("  Functions:");
                for func in &file_info.functions {
                    println!("    - {func}");
                }
            }

            if !file_info.classes.is_empty() {
                println!("  Classes:");
                for class in &file_info.classes {
                    println!("    - {class}");
                }
            }

            if !file_info.modules.is_empty() {
                println!("  Modules:");
                for module in &file_info.modules {
                    println!("    - {module}");
                }
            }
        }
        Err(e) => {
            println!("✗ Failed to parse!");
            println!();
            let error_msg = match &e {
                ParseError::SyntaxError {
                    file,
                    line,
                    column,
                    message,
                } => {
                    format!("Syntax error in {file} at line {line}, column {column}: {message}")
                }
                ParseError::IoError { path, source } => {
                    format!("I/O error reading {path:?}: {source}")
                }
                ParseError::FileTooLarge {
                    path,
                    max_size,
                    actual_size,
                } => {
                    format!(
                        "File {path:?} is too large: {actual_size} bytes (limit: {max_size} bytes)"
                    )
                }
                other => {
                    format!("Error: {other}")
                }
            };
            println!("{error_msg}");
            return Err(Box::new(e));
        }
    }

    println!();
    println!("Graph now contains {} nodes", graph.node_count());

    Ok(())
}
