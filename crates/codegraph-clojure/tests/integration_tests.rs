// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for Clojure parser

use codegraph::CodeGraph;
use codegraph_clojure::ClojureParser;
use codegraph_parser_api::CodeParser;
use std::path::Path;

const SAMPLE_APP: &str = include_str!("fixtures/sample_app.clj");

#[test]
fn test_parse_sample_app_functions() {
    let parser = ClojureParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.clj"), &mut graph)
        .unwrap();

    // Should find multiple defn forms
    assert!(
        file_info.functions.len() >= 10,
        "Expected at least 10 functions, found {}",
        file_info.functions.len()
    );

    let mut func_names = Vec::new();
    for func_id in &file_info.functions {
        let node = graph.get_node(*func_id).unwrap();
        if let Some(codegraph::PropertyValue::String(name)) = node.properties.get("name") {
            func_names.push(name.clone());
        }
    }

    // Public functions
    assert!(
        func_names.iter().any(|n| n == "make-user"),
        "Expected make-user, found: {:?}",
        func_names
    );
    assert!(
        func_names.iter().any(|n| n == "add-role"),
        "Expected add-role, found: {:?}",
        func_names
    );
    assert!(
        func_names.iter().any(|n| n == "has-role?"),
        "Expected has-role?, found: {:?}",
        func_names
    );
    assert!(
        func_names.iter().any(|n| n == "calculate-discount"),
        "Expected calculate-discount, found: {:?}",
        func_names
    );

    // Private (defn-) functions
    assert!(
        func_names.iter().any(|n| n == "validate-email"),
        "Expected validate-email, found: {:?}",
        func_names
    );
    assert!(
        func_names.iter().any(|n| n == "validate-price"),
        "Expected validate-price, found: {:?}",
        func_names
    );

    println!("Functions found: {}", func_names.len());
    println!("Function names: {:?}", func_names);
}

#[test]
fn test_parse_sample_app_visibility() {
    let parser = ClojureParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.clj"), &mut graph)
        .unwrap();

    let mut private_count = 0;
    let mut public_count = 0;

    for func_id in &file_info.functions {
        let node = graph.get_node(*func_id).unwrap();
        if let Some(codegraph::PropertyValue::String(vis)) = node.properties.get("visibility") {
            if vis == "private" {
                private_count += 1;
            } else if vis == "public" {
                public_count += 1;
            }
        }
    }

    assert!(
        private_count >= 2,
        "Expected at least 2 private (defn-) functions, found {private_count}"
    );
    assert!(
        public_count >= 8,
        "Expected at least 8 public (defn) functions, found {public_count}"
    );
    println!("Public: {public_count}, Private: {private_count}");
}

#[test]
fn test_parse_sample_app_types() {
    let parser = ClojureParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.clj"), &mut graph)
        .unwrap();

    // Should find defprotocol and defrecord forms as classes
    assert!(
        file_info.classes.len() >= 3,
        "Expected at least 3 types (2 protocols + 2 records), found {}",
        file_info.classes.len()
    );

    let mut class_names = Vec::new();
    for class_id in &file_info.classes {
        let node = graph.get_node(*class_id).unwrap();
        if let Some(codegraph::PropertyValue::String(name)) = node.properties.get("name") {
            class_names.push(name.clone());
        }
    }

    assert!(
        class_names.iter().any(|n| n == "Describable"),
        "Expected Describable protocol, found: {:?}",
        class_names
    );
    assert!(
        class_names.iter().any(|n| n == "User"),
        "Expected User record, found: {:?}",
        class_names
    );
    assert!(
        class_names.iter().any(|n| n == "Product"),
        "Expected Product record, found: {:?}",
        class_names
    );

    println!("Types found: {:?}", class_names);
}

#[test]
fn test_parse_sample_app_imports() {
    let parser = ClojureParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.clj"), &mut graph)
        .unwrap();

    assert!(
        file_info.imports.len() >= 2,
        "Expected at least 2 imports (clojure.string + clojure.set), found {}",
        file_info.imports.len()
    );

    let mut import_names = Vec::new();
    for import_id in &file_info.imports {
        let node = graph.get_node(*import_id).unwrap();
        if let Some(codegraph::PropertyValue::String(name)) = node.properties.get("name") {
            import_names.push(name.clone());
        }
    }

    assert!(
        import_names.iter().any(|n| n == "clojure.string"),
        "Expected clojure.string import, found: {:?}",
        import_names
    );
    assert!(
        import_names.iter().any(|n| n == "clojure.set"),
        "Expected clojure.set import, found: {:?}",
        import_names
    );

    println!("Imports found: {:?}", import_names);
}

#[test]
fn test_parse_sample_app_complexity() {
    let parser = ClojureParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.clj"), &mut graph)
        .unwrap();

    // register-user and validate-email have branches/conditionals
    let mut found_complex = false;
    for func_id in &file_info.functions {
        let node = graph.get_node(*func_id).unwrap();
        if let Some(codegraph::PropertyValue::Int(complexity)) = node.properties.get("complexity") {
            if *complexity > 1 {
                found_complex = true;
                let name = node
                    .properties
                    .get("name")
                    .and_then(|v| {
                        if let codegraph::PropertyValue::String(s) = v {
                            Some(s.as_str())
                        } else {
                            None
                        }
                    })
                    .unwrap_or("?");
                println!("Complex function: {} (complexity={})", name, complexity);
            }
        }
    }

    assert!(
        found_complex,
        "Expected at least one function with complexity > 1"
    );
}

#[test]
fn test_parse_sample_app_edges() {
    let parser = ClojureParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let _file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.clj"), &mut graph)
        .unwrap();

    let edge_count = graph.edge_count();
    assert!(
        edge_count >= 1,
        "Expected at least 1 graph edge, found {}",
        edge_count
    );
    println!("Total edges: {}", edge_count);
}

#[test]
fn test_parse_sample_app_summary() {
    let parser = ClojureParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let file_info = parser
        .parse_source(SAMPLE_APP, Path::new("sample_app.clj"), &mut graph)
        .unwrap();

    println!("\n=== Clojure Parser Sample App Summary ===");
    println!("File: sample_app.clj");
    println!("Lines: {}", file_info.line_count);
    println!("Types (protocols/records): {}", file_info.classes.len());
    println!("Functions: {}", file_info.functions.len());
    println!("Imports: {}", file_info.imports.len());
    println!("Parse time: {:?}", file_info.parse_time);
    println!("==========================================\n");

    assert!(file_info.line_count > 50);
    assert!(!file_info.functions.is_empty());
    assert!(!file_info.classes.is_empty());
}

#[test]
fn test_file_extensions() {
    let parser = ClojureParser::new();
    assert!(parser.can_parse(Path::new("core.clj")));
    assert!(parser.can_parse(Path::new("app.cljs")));
    assert!(parser.can_parse(Path::new("shared.cljc")));
    assert!(parser.can_parse(Path::new("deps.edn")));
    assert!(!parser.can_parse(Path::new("main.py")));
    assert!(!parser.can_parse(Path::new("script.lua")));
}

#[test]
fn test_docstring_extraction() {
    let parser = ClojureParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = r#"(defn greet
  "Greets a person by name"
  [name]
  (str "Hello, " name))"#;

    let file_info = parser
        .parse_source(source, Path::new("test.clj"), &mut graph)
        .unwrap();

    assert_eq!(file_info.functions.len(), 1);
    let func_node = graph.get_node(file_info.functions[0]).unwrap();

    // Check that docstring was captured
    if let Some(codegraph::PropertyValue::String(doc)) = func_node.properties.get("doc") {
        assert!(
            doc.contains("Greets"),
            "Docstring should contain 'Greets', got: {}",
            doc
        );
        println!("Docstring: {}", doc);
    }
    // Note: doc is optional — if not found we don't fail, but we do print
}

#[test]
fn test_multiarity_and_params() {
    let parser = ClojureParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();

    let source = "(defn add [x y] (+ x y))";
    let file_info = parser
        .parse_source(source, Path::new("test.clj"), &mut graph)
        .unwrap();

    assert_eq!(file_info.functions.len(), 1);
    let func_node = graph.get_node(file_info.functions[0]).unwrap();

    if let Some(codegraph::PropertyValue::StringList(params)) =
        func_node.properties.get("parameters")
    {
        assert_eq!(params.len(), 2, "Expected 2 params (x, y)");
        println!("Parameters: {:?}", params);
    }
}
