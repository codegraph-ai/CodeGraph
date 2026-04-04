// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for codegraph-typescript parser

use codegraph::{CodeGraph, NodeType};
use codegraph_parser_api::CodeParser;
use codegraph_typescript::TypeScriptParser;
use std::path::Path;

#[test]
fn test_parse_simple_function() {
    let source = r#"
function hello() {
    console.log("Hello, world!");
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = TypeScriptParser::new();

    let result = parser.parse_source(source, Path::new("test.ts"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.functions.len(), 1);
}

#[test]
fn test_parse_function_with_parameters() {
    let source = r#"
function add(a: number, b: number): number {
    return a + b;
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = TypeScriptParser::new();

    let result = parser.parse_source(source, Path::new("test.ts"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.functions.len(), 1);
}

#[test]
fn test_parse_async_function() {
    let source = r#"
async function fetchData(): Promise<string> {
    const response = await fetch('api/data');
    return response.json();
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = TypeScriptParser::new();

    let result = parser.parse_source(source, Path::new("test.ts"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.functions.len(), 1);
}

#[test]
fn test_parse_arrow_function() {
    let source = r#"
const multiply = (a: number, b: number): number => a * b;
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = TypeScriptParser::new();

    let result = parser.parse_source(source, Path::new("test.ts"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.functions.len(), 1);
}

#[test]
fn test_parse_class() {
    let source = r#"
class Person {
    name: string;
    age: number;

    constructor(name: string, age: number) {
        this.name = name;
        this.age = age;
    }
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = TypeScriptParser::new();

    let result = parser.parse_source(source, Path::new("test.ts"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.classes.len(), 1);
}

#[test]
fn test_parse_class_with_methods() {
    let source = r#"
class Calculator {
    add(a: number, b: number): number {
        return a + b;
    }

    subtract(a: number, b: number): number {
        return a - b;
    }

    multiply(a: number, b: number): number {
        return a * b;
    }
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = TypeScriptParser::new();

    let result = parser.parse_source(source, Path::new("test.ts"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.classes.len(), 1);
    // Note: Method extraction not yet implemented in TypeScript visitor
}

#[test]
fn test_parse_interface() {
    let source = r#"
interface User {
    name: string;
    age: number;
    email?: string;
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = TypeScriptParser::new();

    let result = parser.parse_source(source, Path::new("test.ts"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.traits.len(), 1);
}

#[test]
fn test_parse_multiple_interfaces() {
    let source = r#"
interface Readable {
    read(): string;
}

interface Writable {
    write(data: string): void;
}

interface ReadWrite extends Readable, Writable {
    reset(): void;
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = TypeScriptParser::new();

    let result = parser.parse_source(source, Path::new("test.ts"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.traits.len(), 3);
}

#[test]
fn test_parse_imports() {
    let source = r#"
import { useState } from 'react';
import type { User } from './types';
import * as Utils from './utils';
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = TypeScriptParser::new();

    let result = parser.parse_source(source, Path::new("test.ts"), &mut graph);
    assert!(result.is_ok());

    // Note: Import extraction not yet fully implemented in TypeScript visitor
    let _info = result.unwrap();
}

#[test]
fn test_parse_default_import() {
    let source = r#"
import React from 'react';
import ReactDOM from 'react-dom';
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = TypeScriptParser::new();

    let result = parser.parse_source(source, Path::new("test.ts"), &mut graph);
    assert!(result.is_ok());

    // Note: Import extraction not yet fully implemented in TypeScript visitor
    let _info = result.unwrap();
}

#[test]
fn test_parse_type_alias() {
    let source = r#"
type ID = string | number;
type Point = { x: number; y: number };
type Callback = (data: string) => void;
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = TypeScriptParser::new();

    let result = parser.parse_source(source, Path::new("test.ts"), &mut graph);
    assert!(result.is_ok());
}

#[test]
fn test_parse_enum() {
    let source = r#"
enum Color {
    Red = 'RED',
    Green = 'GREEN',
    Blue = 'BLUE'
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = TypeScriptParser::new();

    let result = parser.parse_source(source, Path::new("test.ts"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.classes.len(), 1, "Enum should be extracted as a class");

    // Verify the enum node has the "enum" attribute
    let class_ids = graph.query().node_type(NodeType::Class).execute().unwrap();
    assert_eq!(class_ids.len(), 1);
    let node = graph.get_node(class_ids[0]).unwrap();
    let name = node.properties.get_string("name").unwrap();
    assert_eq!(name, "Color");
}

#[test]
fn test_parse_const_enum() {
    let source = r#"
const enum Direction {
    Up,
    Down,
    Left,
    Right
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = TypeScriptParser::new();

    let result = parser.parse_source(source, Path::new("test.ts"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(
        info.classes.len(),
        1,
        "Const enum should be extracted as a class"
    );
}

#[test]
fn test_parse_multiple_entities() {
    let source = r#"
import { Component } from 'react';

interface Props {
    name: string;
}

class Greeter extends Component<Props> {
    greet(): string {
        return `Hello, ${this.props.name}!`;
    }
}

function createGreeter(name: string): Greeter {
    return new Greeter({ name });
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = TypeScriptParser::new();

    let result = parser.parse_source(source, Path::new("test.ts"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    // Note: Import and method extraction not yet fully implemented
    assert_eq!(info.traits.len(), 1);
    assert_eq!(info.classes.len(), 1);
    assert!(!info.functions.is_empty()); // createGreeter function
}

#[test]
fn test_parse_empty_file() {
    let source = "";

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = TypeScriptParser::new();

    let result = parser.parse_source(source, Path::new("test.ts"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.functions.len(), 0);
    assert_eq!(info.classes.len(), 0);
}

#[test]
fn test_parse_comments_only() {
    let source = r#"
// This is a single line comment

/*
 * This is a
 * multi-line comment
 */

/**
 * This is a JSDoc comment
 */
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = TypeScriptParser::new();

    let result = parser.parse_source(source, Path::new("test.ts"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.functions.len(), 0);
    assert_eq!(info.classes.len(), 0);
}

#[test]
fn test_syntax_error() {
    let source = r#"
function broken( {
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = TypeScriptParser::new();

    let result = parser.parse_source(source, Path::new("test.ts"), &mut graph);
    assert!(result.is_err());
}

#[test]
fn test_parser_info() {
    let parser = TypeScriptParser::new();
    assert_eq!(parser.language(), "typescript");
    assert!(parser.can_parse(Path::new("test.ts")));
    assert!(parser.can_parse(Path::new("test.js")));
    assert!(parser.can_parse(Path::new("test.tsx")));
    assert!(parser.can_parse(Path::new("test.jsx")));
    assert!(!parser.can_parse(Path::new("test.rs")));
}

#[test]
fn test_parse_generic_class() {
    let source = r#"
class Container<T> {
    private value: T;

    constructor(value: T) {
        this.value = value;
    }

    getValue(): T {
        return this.value;
    }
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = TypeScriptParser::new();

    let result = parser.parse_source(source, Path::new("test.ts"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.classes.len(), 1);
    // Note: Method extraction not yet implemented in TypeScript visitor
}

#[test]
fn test_parse_abstract_class() {
    let source = r#"
abstract class Animal {
    abstract makeSound(): string;

    move(): void {
        console.log("Moving...");
    }
}

class Dog extends Animal {
    makeSound(): string {
        return "Woof!";
    }
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = TypeScriptParser::new();

    let result = parser.parse_source(source, Path::new("test.ts"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    // Note: Both abstract and concrete classes should be extracted
    assert!(!info.classes.is_empty());
}

#[test]
fn test_parse_complex_file() {
    let source = r#"
import { EventEmitter } from 'events';

/**
 * Represents a user in the system
 */
interface User {
    id: number;
    name: string;
    email: string;
}

/**
 * User service for managing users
 */
class UserService extends EventEmitter {
    private users: Map<number, User> = new Map();

    /**
     * Add a new user
     */
    addUser(user: User): void {
        this.users.set(user.id, user);
        this.emit('userAdded', user);
    }

    /**
     * Get user by ID
     */
    getUser(id: number): User | undefined {
        return this.users.get(id);
    }

    /**
     * Delete user by ID
     */
    deleteUser(id: number): boolean {
        const result = this.users.delete(id);
        if (result) {
            this.emit('userDeleted', id);
        }
        return result;
    }
}

/**
 * Create a new user service instance
 */
function createUserService(): UserService {
    return new UserService();
}

export { UserService, createUserService };
export type { User };
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = TypeScriptParser::new();

    let result = parser.parse_source(source, Path::new("user-service.ts"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    // Note: Import and method extraction not yet fully implemented
    assert_eq!(info.traits.len(), 1); // User interface
    assert_eq!(info.classes.len(), 1); // UserService class
    assert!(!info.functions.is_empty()); // createUserService function
}

#[test]
fn test_parser_metrics() {
    use std::io::Write;
    use tempfile::NamedTempFile;

    let source = r#"
function func1() {}
function func2() {}
"#;

    // Create a temporary file for testing
    let mut temp_file = NamedTempFile::new().unwrap();
    write!(temp_file, "{source}").unwrap();
    temp_file.flush().unwrap();

    let mut graph = CodeGraph::in_memory().unwrap();
    let mut parser = TypeScriptParser::new();

    // parse_file (not parse_source) updates metrics
    let _ = parser.parse_file(temp_file.path(), &mut graph);

    let metrics = parser.metrics();
    assert_eq!(metrics.files_attempted, 1);
    assert_eq!(metrics.files_succeeded, 1);

    parser.reset_metrics();
    let metrics = parser.metrics();
    assert_eq!(metrics.files_attempted, 0);
}

#[test]
fn test_parse_jsx_syntax() {
    let source = r#"
import React from 'react';

function Welcome(props: { name: string }) {
    return <h1>Hello, {props.name}</h1>;
}

export default Welcome;
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = TypeScriptParser::new();

    // Note: JSX/TSX syntax requires special handling in tree-sitter-typescript
    // Currently this may fail to parse - this is a known limitation
    let result = parser.parse_source(source, Path::new("welcome.tsx"), &mut graph);

    // For now, just check that parsing doesn't crash
    // JSX support may need additional configuration
    let _ = result;
}

#[test]
fn test_parse_decorator() {
    let source = r#"
function log(target: any, key: string) {
    console.log(`${key} was called`);
}

class Service {
    @log
    execute() {
        console.log("Executing...");
    }
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = TypeScriptParser::new();

    let result = parser.parse_source(source, Path::new("test.ts"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.classes.len(), 1);
    assert!(!info.functions.is_empty()); // log function
                                         // Note: Method extraction not yet implemented in TypeScript visitor
}

#[test]
fn test_triple_slash_reference_path() {
    let source = r#"/// <reference path="./types.d.ts" />
/// <reference path="../globals.d.ts" />

import { Something } from './something';

function hello(): void {
    console.log("Hello");
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = TypeScriptParser::new();
    let result = parser.parse_source(source, Path::new("test.ts"), &mut graph);
    assert!(result.is_ok());

    let _info = result.unwrap();
    // 2 reference directives + 1 import statement = 3 Module nodes
    let module_ids = graph.query().node_type(NodeType::Module).execute().unwrap();
    assert_eq!(
        module_ids.len(),
        3,
        "Expected 3 modules, found {}",
        module_ids.len()
    );

    // Check that the reference paths are present
    let mut ref_paths: Vec<String> = Vec::new();
    for id in &module_ids {
        let node = graph.get_node(*id).unwrap();
        let name = node.properties.get_string("name").unwrap().to_string();
        if name == "./types.d.ts" || name == "../globals.d.ts" {
            ref_paths.push(name);
        }
    }
    ref_paths.sort();
    assert_eq!(ref_paths, vec!["../globals.d.ts", "./types.d.ts"]);
}

#[test]
fn test_method_visibility() {
    let source = r#"
class MyClass {
    private secret(): void {}
    protected internal(): string { return ""; }
    public visible(): number { return 1; }
    implicit(): boolean { return true; }
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = TypeScriptParser::new();

    let result = parser.parse_source(source, Path::new("test.ts"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.classes.len(), 1);

    // Get all function nodes and check their visibility property
    let func_ids = graph
        .query()
        .node_type(NodeType::Function)
        .execute()
        .unwrap();

    let mut visibilities: Vec<(String, String)> = func_ids
        .iter()
        .filter_map(|&id| {
            let node = graph.get_node(id).ok()?;
            let name = node.properties.get_string("name")?.to_string();
            let vis = node.properties.get_string("visibility")?.to_string();
            Some((name, vis))
        })
        .collect();
    visibilities.sort_by(|a, b| a.0.cmp(&b.0));

    // Verify each method has the correct visibility
    assert!(
        visibilities
            .iter()
            .any(|(n, v)| n == "secret" && v == "private"),
        "secret should be private, got: {:?}",
        visibilities
    );
    assert!(
        visibilities
            .iter()
            .any(|(n, v)| n == "internal" && v == "protected"),
        "internal should be protected, got: {:?}",
        visibilities
    );
    assert!(
        visibilities
            .iter()
            .any(|(n, v)| n == "visible" && v == "public"),
        "visible should be public, got: {:?}",
        visibilities
    );
}

#[test]
fn test_triple_slash_types_reference_not_captured() {
    let source = r#"/// <reference types="node" />
/// This is a regular comment
// Another comment

function hello(): void {}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = TypeScriptParser::new();
    let result = parser.parse_source(source, Path::new("test.ts"), &mut graph);
    assert!(result.is_ok());

    // types= references should NOT be captured, and regular comments should be ignored
    let module_ids = graph.query().node_type(NodeType::Module).execute().unwrap();
    assert_eq!(
        module_ids.len(),
        0,
        "Expected 0 modules from types refs and comments"
    );
}
