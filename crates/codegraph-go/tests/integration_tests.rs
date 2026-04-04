// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for codegraph-go parser

use codegraph::CodeGraph;
use codegraph_go::GoParser;
use codegraph_parser_api::CodeParser;
use std::path::Path;

#[test]
fn test_parse_simple_function() {
    let source = r#"
package main

func hello() {
    println("Hello, world!")
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = GoParser::new();

    let result = parser.parse_source(source, Path::new("main.go"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.functions.len(), 1);
}

#[test]
fn test_parse_function_with_parameters() {
    let source = r#"
package main

func add(a int, b int) int {
    return a + b
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = GoParser::new();

    let result = parser.parse_source(source, Path::new("main.go"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.functions.len(), 1);
}

#[test]
fn test_parse_multiple_return_values() {
    let source = r#"
package main

func swap(a, b int) (int, int) {
    return b, a
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = GoParser::new();

    let result = parser.parse_source(source, Path::new("main.go"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.functions.len(), 1);
}

#[test]
fn test_parse_variadic_function() {
    let source = r#"
package main

func sum(numbers ...int) int {
    total := 0
    for _, n := range numbers {
        total += n
    }
    return total
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = GoParser::new();

    let result = parser.parse_source(source, Path::new("main.go"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.functions.len(), 1);
}

#[test]
fn test_parse_struct() {
    let source = r#"
package main

type Person struct {
    Name string
    Age  int
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = GoParser::new();

    let result = parser.parse_source(source, Path::new("main.go"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.classes.len(), 1);
}

#[test]
fn test_parse_struct_with_methods() {
    let source = r#"
package main

type Calculator struct {
    result int
}

func (c *Calculator) Add(a, b int) int {
    return a + b
}

func (c *Calculator) Multiply(a, b int) int {
    return a * b
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = GoParser::new();

    let result = parser.parse_source(source, Path::new("main.go"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.classes.len(), 1);
    assert_eq!(info.functions.len(), 2); // Two methods
}

#[test]
fn test_parse_interface() {
    let source = r#"
package main

type Reader interface {
    Read(p []byte) (n int, err error)
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = GoParser::new();

    let result = parser.parse_source(source, Path::new("main.go"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.traits.len(), 1);
}

#[test]
fn test_parse_multiple_interfaces() {
    let source = r#"
package main

type Reader interface {
    Read(p []byte) (n int, err error)
}

type Writer interface {
    Write(p []byte) (n int, err error)
}

type ReadWriter interface {
    Reader
    Writer
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = GoParser::new();

    let result = parser.parse_source(source, Path::new("main.go"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.traits.len(), 3);
}

#[test]
fn test_parse_imports() {
    let source = r#"
package main

import (
    "fmt"
    "os"
    "encoding/json"
)

func main() {
    fmt.Println("Hello")
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = GoParser::new();

    let result = parser.parse_source(source, Path::new("main.go"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    // Should extract 3 individual imports: fmt, os, encoding/json
    assert_eq!(info.imports.len(), 3);
}

#[test]
fn test_parse_import_aliases() {
    let source = r#"
package main

import (
    f "fmt"
    . "os"
    _ "encoding/json"
)
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = GoParser::new();

    let result = parser.parse_source(source, Path::new("main.go"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    // Should extract 3 individual imports with proper alias handling
    // (Detailed alias/wildcard testing is done in unit tests)
    assert_eq!(info.imports.len(), 3);
}

#[test]
fn test_parse_multiple_entities() {
    let source = r#"
package main

import "fmt"

type Animal interface {
    Speak() string
}

type Dog struct {
    Name string
}

func (d *Dog) Speak() string {
    return "Woof!"
}

func main() {
    dog := &Dog{Name: "Rex"}
    fmt.Println(dog.Speak())
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = GoParser::new();

    let result = parser.parse_source(source, Path::new("main.go"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.traits.len(), 1);
    assert_eq!(info.classes.len(), 1);
    assert_eq!(info.functions.len(), 2); // Speak method + main function
    assert_eq!(info.imports.len(), 1);
}

#[test]
fn test_parse_empty_file() {
    let source = "package main\n";

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = GoParser::new();

    let result = parser.parse_source(source, Path::new("main.go"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.functions.len(), 0);
    assert_eq!(info.classes.len(), 0);
}

#[test]
fn test_parse_comments_only() {
    let source = r#"
package main

// This is a comment
/* This is a
   multi-line comment */
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = GoParser::new();

    let result = parser.parse_source(source, Path::new("main.go"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.functions.len(), 0);
    assert_eq!(info.classes.len(), 0);
}

#[test]
fn test_syntax_error() {
    let source = r#"
package main

func broken( {
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = GoParser::new();

    let result = parser.parse_source(source, Path::new("main.go"), &mut graph);
    assert!(result.is_err());
}

#[test]
fn test_parser_info() {
    let parser = GoParser::new();
    assert_eq!(parser.language(), "go");
    assert!(parser.can_parse(Path::new("main.go")));
    assert!(parser.can_parse(Path::new("file.go")));
    assert!(!parser.can_parse(Path::new("file.rs")));
}

#[test]
fn test_parse_generic_types() {
    let source = r#"
package main

type Stack[T any] struct {
    items []T
}

func (s *Stack[T]) Push(item T) {
    s.items = append(s.items, item)
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = GoParser::new();

    let result = parser.parse_source(source, Path::new("main.go"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.classes.len(), 1);
    assert_eq!(info.functions.len(), 1);
}

#[test]
fn test_parse_complex_file() {
    let source = r#"
package main

import (
    "fmt"
    "errors"
)

// Person represents a person
type Person struct {
    Name string
    Age  int
}

// NewPerson creates a new person
func NewPerson(name string, age int) (*Person, error) {
    if age < 0 {
        return nil, errors.New("age cannot be negative")
    }
    return &Person{Name: name, Age: age}, nil
}

// Greet returns a greeting message
func (p *Person) Greet() string {
    return fmt.Sprintf("Hello, I'm %s", p.Name)
}

func main() {
    person, err := NewPerson("Alice", 30)
    if err != nil {
        panic(err)
    }
    fmt.Println(person.Greet())
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = GoParser::new();

    let result = parser.parse_source(source, Path::new("main.go"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.classes.len(), 1); // Person struct
    assert_eq!(info.functions.len(), 3); // NewPerson, Greet, main
                                         // Should extract 2 individual imports: fmt, errors
    assert_eq!(info.imports.len(), 2);
}

#[test]
fn test_parser_metrics() {
    use std::io::Write;
    use tempfile::NamedTempFile;

    let source = r#"
package main

func func1() {}
func func2() {}
"#;

    // Create a temporary file for testing
    let mut temp_file = NamedTempFile::new().unwrap();
    write!(temp_file, "{source}").unwrap();
    temp_file.flush().unwrap();

    let mut graph = CodeGraph::in_memory().unwrap();
    let mut parser = GoParser::new();

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
fn test_parse_constants_and_variables() {
    let source = r#"
package main

const (
    MaxRetries = 3
    Timeout = 30
)

var (
    GlobalCounter int
    Logger *log.Logger
)
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = GoParser::new();

    let result = parser.parse_source(source, Path::new("main.go"), &mut graph);
    assert!(result.is_ok());
}

#[test]
fn test_parse_embedded_structs() {
    let source = r#"
package main

type Base struct {
    ID int
}

type Derived struct {
    Base
    Name string
}
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = GoParser::new();

    let result = parser.parse_source(source, Path::new("main.go"), &mut graph);
    assert!(result.is_ok());

    let info = result.unwrap();
    assert_eq!(info.classes.len(), 2);
}
