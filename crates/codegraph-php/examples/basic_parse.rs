// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Basic example of using the PHP parser

use codegraph::CodeGraph;
use codegraph_php::{CodeParser, PhpParser};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create an in-memory graph
    let mut graph = CodeGraph::in_memory()?;

    // Create the PHP parser
    let parser = PhpParser::new();

    // Parse PHP source code directly
    let source = r#"<?php
namespace App\Controllers;

use App\Models\User;
use App\Services\AuthService;

interface Authenticatable {
    public function authenticate(string $email, string $password): ?User;
}

class AuthController implements Authenticatable {
    private AuthService $authService;

    public function __construct(AuthService $authService) {
        $this->authService = $authService;
    }

    public function authenticate(string $email, string $password): ?User {
        return $this->authService->login($email, $password);
    }

    public function logout(): void {
        $this->authService->logout();
    }
}
"#;

    let file_info = parser.parse_source(source, Path::new("AuthController.php"), &mut graph)?;

    println!("Parsed PHP file:");
    println!("  Functions: {}", file_info.functions.len());
    println!("  Classes: {}", file_info.classes.len());
    println!("  Interfaces/Traits: {}", file_info.traits.len());
    println!("  Imports: {}", file_info.imports.len());
    println!("  Lines: {}", file_info.line_count);
    println!("  Parse time: {:?}", file_info.parse_time);

    // Print function details
    println!("\nFunctions:");
    for func_id in &file_info.functions {
        let node = graph.get_node(*func_id)?;
        let name = node.properties.get_string("name").unwrap_or("unknown");
        let visibility = node.properties.get_string("visibility").unwrap_or("public");
        println!("  - {} ({})", name, visibility);
    }

    // Print class details
    println!("\nClasses:");
    for class_id in &file_info.classes {
        let node = graph.get_node(*class_id)?;
        let name = node.properties.get_string("name").unwrap_or("unknown");
        println!("  - {}", name);
    }

    // Print interface details
    println!("\nInterfaces:");
    for trait_id in &file_info.traits {
        let node = graph.get_node(*trait_id)?;
        let name = node.properties.get_string("name").unwrap_or("unknown");
        println!("  - {}", name);
    }

    // Print import details
    println!("\nImports:");
    for import_id in &file_info.imports {
        let node = graph.get_node(*import_id)?;
        let name = node.properties.get_string("name").unwrap_or("unknown");
        println!("  - {}", name);
    }

    Ok(())
}
