// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for CSS parser

use codegraph::CodeGraph;
use codegraph_css::CssParser;
use codegraph_parser_api::CodeParser;
use std::path::Path;

const SAMPLE_APP: &str = include_str!("fixtures/sample_app.css");

#[test]
fn test_parse_sample_app_selectors() {
    let parser = CssParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.css"), &mut graph)
        .unwrap();

    // Should find many rule_sets (selectors mapped as "functions")
    assert!(
        file_info.functions.len() >= 15,
        "Expected at least 15 selectors, found {}",
        file_info.functions.len()
    );

    let mut selector_names = Vec::new();
    for func_id in &file_info.functions {
        let node = graph.get_node(*func_id).unwrap();
        if let Some(codegraph::PropertyValue::String(name)) = node.properties.get("name") {
            selector_names.push(name.clone());
        }
    }

    println!("Selectors found: {} total", selector_names.len());
    println!(
        "Sample selectors: {:?}",
        &selector_names[..selector_names.len().min(10)]
    );

    // Check for known selectors
    assert!(
        selector_names.iter().any(|n| n.contains("container")),
        "Should contain .container, found: {:?}",
        selector_names
    );
    assert!(
        selector_names.iter().any(|n| n == "body"),
        "Should contain body, found: {:?}",
        selector_names
    );
    assert!(
        selector_names.iter().any(|n| n.contains("btn")),
        "Should contain .btn, found: {:?}",
        selector_names
    );
}

#[test]
fn test_parse_sample_app_imports() {
    let parser = CssParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.css"), &mut graph)
        .unwrap();

    // Should find @import "reset.css", @import "variables.css", @import url("fonts.css")
    assert!(
        file_info.imports.len() >= 3,
        "Expected at least 3 imports, found {}",
        file_info.imports.len()
    );

    let mut import_names = Vec::new();
    for import_id in &file_info.imports {
        let node = graph.get_node(*import_id).unwrap();
        if let Some(codegraph::PropertyValue::String(name)) = node.properties.get("name") {
            import_names.push(name.clone());
        }
    }

    println!("Imports found: {:?}", import_names);

    assert!(
        import_names.iter().any(|n| n == "reset.css"),
        "Should import reset.css, found: {:?}",
        import_names
    );
    assert!(
        import_names.iter().any(|n| n == "variables.css"),
        "Should import variables.css, found: {:?}",
        import_names
    );
    assert!(
        import_names.iter().any(|n| n == "fonts.css"),
        "Should import fonts.css via url(), found: {:?}",
        import_names
    );
}

#[test]
fn test_parse_sample_app_media_nested_selectors() {
    let parser = CssParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.css"), &mut graph)
        .unwrap();

    let mut selector_names = Vec::new();
    for func_id in &file_info.functions {
        let node = graph.get_node(*func_id).unwrap();
        if let Some(codegraph::PropertyValue::String(name)) = node.properties.get("name") {
            selector_names.push(name.clone());
        }
    }

    // @media blocks contain nested rule_sets that should be extracted
    // .container, .navbar, h1 appear inside @media (max-width: 768px)
    // .col, .btn appear inside @media (max-width: 576px)
    // The selectors may appear multiple times (once top-level, once nested)
    let container_count = selector_names.iter().filter(|n| n.contains("container")).count();
    assert!(
        container_count >= 2,
        "Expected .container at least twice (top-level + @media), found {} times",
        container_count
    );
}

#[test]
fn test_parse_sample_app_graph_edges() {
    let parser = CssParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let _file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.css"), &mut graph)
        .unwrap();

    let edge_count = graph.edge_count();
    assert!(
        edge_count >= 10,
        "Expected at least 10 edges (file→selectors, file→imports), found {}",
        edge_count
    );

    println!("Total graph edges: {}", edge_count);
}

#[test]
fn test_parse_sample_app_summary() {
    let parser = CssParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.css"), &mut graph)
        .unwrap();

    println!("\n=== CSS Parser Sample App Summary ===");
    println!("File: sample_app.css");
    println!("Lines: {}", file_info.line_count);
    println!("Selectors (functions): {}", file_info.functions.len());
    println!("Imports: {}", file_info.imports.len());
    println!("Parse time: {:?}", file_info.parse_time);
    println!("=====================================\n");

    assert!(file_info.line_count > 50);
    assert!(!file_info.functions.is_empty());
    assert!(!file_info.imports.is_empty());
}

#[test]
fn test_parse_pseudo_selectors() {
    let parser = CssParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"
a:hover {
    color: blue;
}

input:focus {
    outline: none;
}

.btn:disabled {
    opacity: 0.5;
}
"#;

    let file_info = parser
        .parse_source(source, Path::new("pseudo.css"), &mut graph)
        .unwrap();

    assert_eq!(
        file_info.functions.len(),
        3,
        "Expected 3 pseudo-class selectors"
    );

    let mut names = Vec::new();
    for func_id in &file_info.functions {
        let node = graph.get_node(*func_id).unwrap();
        if let Some(codegraph::PropertyValue::String(name)) = node.properties.get("name") {
            names.push(name.clone());
        }
    }

    println!("Pseudo selectors: {:?}", names);
    assert!(names.iter().any(|n| n.contains("hover")));
    assert!(names.iter().any(|n| n.contains("focus")));
}

#[test]
fn test_parse_comma_selectors() {
    let parser = CssParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"
h1, h2, h3 {
    font-weight: bold;
}
"#;

    let file_info = parser
        .parse_source(source, Path::new("multi.css"), &mut graph)
        .unwrap();

    assert_eq!(file_info.functions.len(), 1, "h1, h2, h3 is one rule_set");

    let node = graph.get_node(file_info.functions[0]).unwrap();
    if let Some(codegraph::PropertyValue::String(name)) = node.properties.get("name") {
        assert!(
            name.contains("h1") && name.contains("h2") && name.contains("h3"),
            "Selector should include all three tags, got: {}",
            name
        );
    }
}

#[test]
fn test_import_url_syntax() {
    let parser = CssParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"@import url("https://fonts.googleapis.com/css2?family=Inter");"#;

    let file_info = parser
        .parse_source(source, Path::new("fonts.css"), &mut graph)
        .unwrap();

    assert_eq!(file_info.imports.len(), 1);

    let node = graph.get_node(file_info.imports[0]).unwrap();
    if let Some(codegraph::PropertyValue::String(name)) = node.properties.get("name") {
        assert!(
            name.contains("fonts.googleapis.com"),
            "Expected Google Fonts URL, got: {}",
            name
        );
    }
}
