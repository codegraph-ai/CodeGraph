// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use codegraph::CodeGraph;
use codegraph_python::Parser;
use std::path::Path;

fn main() {
    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = Parser::new();

    let test_file = Path::new("tests/fixtures/calls.py");
    let result = parser.parse_file(test_file, &mut graph);

    match result {
        Ok(file_info) => {
            println!("✓ Successfully parsed!");
            println!("\nEntities:");
            println!("  Functions: {}", file_info.functions.len());
            println!("  Classes:   {}", file_info.classes.len());
            println!("\nFunctions found:");
            for func in &file_info.functions {
                println!("  - {func}");
            }

            // To see calls, we'd need to access the IR directly
            // For now, we know from unit tests that calls are being extracted
        }
        Err(e) => {
            println!("✗ Parse error: {e}");
        }
    }
}
