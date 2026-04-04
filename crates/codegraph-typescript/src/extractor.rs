// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST extraction for TypeScript/JavaScript source code

use codegraph_parser_api::{CodeIR, ModuleEntity, ParserConfig, ParserError};
use std::path::Path;
use tree_sitter::Parser;

use crate::visitor::TypeScriptVisitor;

/// Extract code entities and relationships from TypeScript/JavaScript source code
pub fn extract(
    source: &str,
    file_path: &Path,
    _config: &ParserConfig,
) -> Result<CodeIR, ParserError> {
    // Create tree-sitter parser with appropriate language variant
    let mut parser = Parser::new();

    // Detect if file is JSX/TSX based on extension
    let is_jsx = file_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e == "tsx" || e == "jsx")
        .unwrap_or(false);

    let language = if is_jsx {
        tree_sitter_typescript::language_tsx()
    } else {
        tree_sitter_typescript::language_typescript()
    };

    parser
        .set_language(&language)
        .map_err(|e| ParserError::ParseError(file_path.to_path_buf(), e.to_string()))?;

    // Parse the source code
    let tree = parser.parse(source, None).ok_or_else(|| {
        ParserError::ParseError(file_path.to_path_buf(), "Failed to parse".to_string())
    })?;

    // Check for syntax errors
    let root_node = tree.root_node();
    if root_node.has_error() {
        return Err(ParserError::SyntaxError(
            file_path.to_path_buf(),
            0,
            0,
            "Syntax error in source code".to_string(),
        ));
    }

    // Create IR for this file
    let mut ir = CodeIR::new(file_path.to_path_buf());

    // Create module entity for the file
    let module_name = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    let module = ModuleEntity {
        name: module_name.clone(),
        path: file_path.display().to_string(),
        language: "typescript".to_string(),
        line_count: source.lines().count(),
        doc_comment: None,
        attributes: Vec::new(),
    };

    ir.module = Some(module);

    // Create visitor and walk the AST
    let mut visitor = TypeScriptVisitor::new(source.as_bytes());
    visitor.visit_node(root_node);

    // Transfer extracted entities to IR
    ir.functions = visitor.functions;
    ir.classes = visitor.classes;
    ir.traits = visitor.interfaces;
    ir.imports = visitor.imports;
    ir.calls = visitor.calls;
    ir.implementations = visitor.implementations;
    ir.inheritance = visitor.inheritance;
    ir.type_references = visitor.type_references;

    Ok(ir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_simple_function() {
        let source = r#"
function hello() {
    console.log("Hello, world!");
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.ts"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].name, "hello");
    }

    #[test]
    fn test_extract_class() {
        let source = r#"
class Person {
    name: string;
    age: number;
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.ts"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "Person");
    }

    #[test]
    fn test_extract_interface() {
        let source = r#"
interface User {
    id: number;
    username: string;
    email: string;
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.ts"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.traits.len(), 1);
        assert_eq!(ir.traits[0].name, "User");
    }

    #[test]
    fn test_extract_async_function() {
        let source = r#"
async function fetchData() {
    const response = await fetch('api/data');
    return response.json();
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.ts"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].name, "fetchData");
        assert!(ir.functions[0].is_async);
    }

    #[test]
    fn test_extract_arrow_function() {
        let source = r#"
const add = (a: number, b: number) => {
    return a + b;
};
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.ts"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.functions.len(), 1);
    }

    #[test]
    fn test_extract_class_with_methods() {
        let source = r#"
class Calculator {
    add(a: number, b: number): number {
        return a + b;
    }

    subtract(a: number, b: number): number {
        return a - b;
    }
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.ts"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "Calculator");
        // Note: Method extraction not yet implemented in visitor
        // Methods would need "method_definition" node type support
    }

    #[test]
    fn test_extract_import_statement() {
        let source = r#"
import { Component } from 'react';
import fs from 'fs';
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.ts"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.imports.len(), 2);
    }

    #[test]
    fn test_extract_multiple_entities() {
        let source = r#"
interface Shape {
    area(): number;
}

class Circle implements Shape {
    radius: number;

    constructor(radius: number) {
        this.radius = radius;
    }

    area(): number {
        return Math.PI * this.radius * this.radius;
    }
}

function calculateTotal(shapes: Shape[]): number {
    return shapes.reduce((sum, shape) => sum + shape.area(), 0);
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.ts"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.traits.len(), 1);
        assert_eq!(ir.traits[0].name, "Shape");
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "Circle");
        assert!(ir.functions.len() >= 2); // constructor, area, calculateTotal, arrow function
    }

    #[test]
    fn test_extract_with_syntax_error() {
        let source = r#"
function broken( {
    // Missing closing brace
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.ts"), &config);

        assert!(result.is_err());
        match result {
            Err(ParserError::SyntaxError(..)) => (),
            _ => panic!("Expected SyntaxError"),
        }
    }

    #[test]
    fn test_extract_module_info() {
        let source = r#"
function test() {
    console.log("test");
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("module.ts"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert!(ir.module.is_some());
        let module = ir.module.unwrap();
        assert_eq!(module.name, "module");
        assert_eq!(module.language, "typescript");
        assert!(module.line_count > 0);
    }

    #[test]
    fn test_extract_jsx_component() {
        let source = r#"
import React from 'react';

function Welcome(props) {
    return <h1>Hello, {props.name}</h1>;
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("Welcome.jsx"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].name, "Welcome");
        assert_eq!(ir.imports.len(), 1);
    }

    #[test]
    fn test_extract_tsx_component() {
        let source = r#"
import React from 'react';

interface Props {
    name: string;
}

function Greeting(props: Props) {
    return <h1>Hello, {props.name}!</h1>;
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("Greeting.tsx"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].name, "Greeting");
        assert_eq!(ir.traits.len(), 1);
        assert_eq!(ir.traits[0].name, "Props");
        assert_eq!(ir.imports.len(), 1);
    }

    #[test]
    fn test_extract_tsx_class_component() {
        let source = r#"
import React, { Component } from 'react';

class Counter extends Component {
    render() {
        return <button onClick={this.handleClick}>Count</button>;
    }
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("Counter.tsx"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "Counter");
        assert!(!ir.functions.is_empty()); // render method
        assert_eq!(ir.imports.len(), 1);
    }

    #[test]
    fn test_extract_jsx_with_fragments() {
        let source = r#"
function List() {
    return (
        <>
            <li>Item 1</li>
            <li>Item 2</li>
        </>
    );
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("List.jsx"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].name, "List");
    }

    #[test]
    fn test_extract_tsx_with_hooks() {
        let source = r#"
import { useState, useEffect } from 'react';

function Counter() {
    const [count, setCount] = useState(0);

    useEffect(() => {
        document.title = `Count: ${count}`;
    }, [count]);

    return <button onClick={() => setCount(count + 1)}>{count}</button>;
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("Counter.tsx"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        // Now extracts Counter + 2 arrow functions in hooks
        assert!(!ir.functions.is_empty());
        // Counter should be the first named function
        let counter_fn = ir.functions.iter().find(|f| f.name == "Counter");
        assert!(counter_fn.is_some(), "Counter function should be extracted");
        assert_eq!(ir.imports.len(), 1);
    }

    #[test]
    fn test_extract_type_references() {
        let source = r#"
interface MyParams {
    uri: string;
}

interface MyResponse {
    data: MyParams;
}

function process(params: MyParams): MyResponse {
    return { data: params };
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.ts"), &config);
        assert!(result.is_ok());
        let ir = result.unwrap();

        eprintln!("type_references: {:?}", ir.type_references);

        // process should reference MyParams (param) and MyResponse (return)
        let process_refs: Vec<_> = ir
            .type_references
            .iter()
            .filter(|r| r.referrer == "process")
            .map(|r| r.type_name.as_str())
            .collect();
        assert!(
            process_refs.contains(&"MyParams"),
            "process should reference MyParams. Got: {:?}",
            process_refs
        );
        assert!(
            process_refs.contains(&"MyResponse"),
            "process should reference MyResponse. Got: {:?}",
            process_refs
        );

        // MyResponse has field type MyParams
        let response_refs: Vec<_> = ir
            .type_references
            .iter()
            .filter(|r| r.referrer == "MyResponse")
            .map(|r| r.type_name.as_str())
            .collect();
        assert!(
            response_refs.contains(&"MyParams"),
            "MyResponse should reference MyParams via field type. Got: {:?}",
            response_refs
        );
    }

    #[test]
    fn test_extract_type_references_in_expressions() {
        let source = r#"
interface Config {
    timeout: number;
}

interface Result {
    data: string;
}

function process() {
    const cfg: Config = { timeout: 100 };
    const result = {} as Result;
    const items = new Map<string, Config>();
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.ts"), &config);
        assert!(result.is_ok());
        let ir = result.unwrap();

        let process_refs: Vec<_> = ir
            .type_references
            .iter()
            .filter(|r| r.referrer == "process")
            .map(|r| r.type_name.as_str())
            .collect();

        eprintln!("process type refs: {:?}", process_refs);

        // Variable type annotation: const cfg: Config
        assert!(
            process_refs.contains(&"Config"),
            "Should reference Config from variable annotation. Got: {:?}",
            process_refs
        );

        // as cast: {} as Result
        assert!(
            process_refs.contains(&"Result"),
            "Should reference Result from as cast. Got: {:?}",
            process_refs
        );
    }
}
