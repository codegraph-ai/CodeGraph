# codegraph-python

Python parser plugin for CodeGraph - extracts code entities and relationships from Python source files into a queryable graph database.

[![Crates.io](https://img.shields.io/crates/v/codegraph-python.svg)](https://crates.io/crates/codegraph-python)
[![Documentation](https://docs.rs/codegraph-python/badge.svg)](https://docs.rs/codegraph-python)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache--2.0-blue.svg)](LICENSE)

## Version 0.3.0 - Cyclomatic Complexity Analysis

**codegraph-python v0.3.0** adds AST-based cyclomatic complexity calculation for all functions.

### Complexity Metrics

Functions now include detailed complexity analysis:

```rust
use codegraph_parser_api::CodeParser;
use codegraph_python::PythonParser;

let parser = PythonParser::new();
let ir = parser.parse_source(source, Path::new("example.py"))?;

for func in &ir.functions {
    if let Some(complexity) = &func.complexity {
        println!("{}: CC={} Grade={}",
            func.name,
            complexity.cyclomatic_complexity,
            complexity.grade()  // A, B, C, D, or F
        );
        println!("  Branches: {}", complexity.branches);
        println!("  Loops: {}", complexity.loops);
        println!("  Logical ops: {}", complexity.logical_operators);
        println!("  Max nesting: {}", complexity.max_nesting_depth);
    }
}
```

### Grading Scale
- **A** (1-5): Simple, low risk
- **B** (6-10): Moderate complexity
- **C** (11-20): Complex, moderate risk
- **D** (21-50): Very complex, high risk
- **F** (51+): Untestable, very high risk

---

## Features

- 🚀 **Fast**: Parse 1000 Python files in under 10 seconds
- 🎯 **Accurate**: Extract functions, classes, methods, decorators, type hints
- 🔗 **Relationships**: Track function calls, imports, inheritance hierarchies
- ⚙️ **Configurable**: Filter by visibility, file size, enable parallel processing
- 🛡️ **Safe**: No panics, graceful error handling, continues on failures
- 📊 **Complete**: 67 tests, 90%+ code coverage
- 🐍 **Python 3.8+**: Full support for async/await, decorators, type hints, match statements

## What it Extracts

### Entities
- **Functions**: Names, signatures, parameters, return types, decorators, async flag
- **Classes**: Names, base classes, methods, fields, decorators, abstract flag
- **Methods**: Instance methods, static methods, class methods, properties
- **Modules**: File-level metadata and documentation

### Relationships
- **Calls**: Function and method call relationships (who calls whom)
- **Imports**: Module dependencies (import statements, from imports, wildcards)
- **Inheritance**: Class hierarchies (single and multiple inheritance)
- **Implementations**: Protocol/ABC implementations

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
codegraph = "0.1.1"
codegraph-python = "0.1.0"
```

## Quick Start

### Parse a Single File

```rust
use codegraph_python::Parser;
use codegraph::CodeGraph;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut graph = CodeGraph::in_memory()?;
    let parser = Parser::new();
    
    let info = parser.parse_file(
        std::path::Path::new("src/main.py"),
        &mut graph
    )?;
    
    println!("Found {} functions", info.functions.len());
    println!("Found {} classes", info.classes.len());
    println!("Parsed in {:?}", info.parse_time);
    
    Ok(())
}
```

### Parse an Entire Project

```rust
use codegraph_python::Parser;
use codegraph::CodeGraph;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut graph = CodeGraph::in_memory()?;
    let parser = Parser::new();
    
    let project_info = parser.parse_directory(
        std::path::Path::new("src/"),
        &mut graph
    )?;
    
    println!("Parsed {} files", project_info.files.len());
    println!("Total functions: {}", project_info.total_functions);
    println!("Total classes: {}", project_info.total_classes);
    println!("Success rate: {:.1}%", project_info.success_rate());
    
    Ok(())
}
```

### Custom Configuration

```rust
use codegraph_python::{Parser, ParserConfig};
use codegraph::CodeGraph;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut graph = CodeGraph::in_memory()?;
    
    let config = ParserConfig {
        include_private: false,        // Skip private entities (_name)
        include_tests: true,            // Include test functions
        parse_docs: true,               // Extract docstrings
        max_file_size: 10 * 1024 * 1024, // 10MB limit
        file_extensions: vec!["py".to_string()],
        exclude_dirs: vec![
            "__pycache__".to_string(),
            ".venv".to_string(),
        ],
        parallel: true,                 // Use parallel processing
        num_threads: Some(4),          // 4 threads
    };
    
    let parser = Parser::with_config(config);
    let project_info = parser.parse_directory(
        std::path::Path::new("src/"),
        &mut graph
    )?;
    
    println!("Parsed {} files in {:?}", 
        project_info.files.len(),
        project_info.total_time
    );
    
    Ok(())
}
```

### Parse from String

```rust
use codegraph_python::Parser;
use codegraph::CodeGraph;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut graph = CodeGraph::in_memory()?;
    let parser = Parser::new();
    
    let source = r#"
def greet(name: str) -> str:
    """Greet someone by name."""
    return f"Hello, {name}!"

class Person:
    def __init__(self, name: str):
        self.name = name
    
    def introduce(self) -> str:
        return greet(self.name)
"#;
    
    let info = parser.parse_source(
        source,
        std::path::Path::new("example.py"),
        &mut graph
    )?;
    
    println!("Found {} entities", info.entity_count());
    
    Ok(())
}
```rust
use codegraph_python::Parser;
use codegraph::CodeGraph;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut graph = CodeGraph::open("./project.graph")?;
    let parser = Parser::new();
    
    let info = parser.parse_project(&mut graph, "./src")?;
    
    println!("Files parsed: {}", info.files.len());
    println!("Total functions: {}", info.total_functions);
    println!("Total classes: {}", info.total_classes);
    println!("Parse time: {:?}", info.total_time);
    
    Ok(())
}
```

### Configure Parser Behavior

```rust
use codegraph_python::{Parser, ParserConfig};

let config = ParserConfig {
    include_private: false,
    include_tests: false,
    parse_docs: true,
    max_file_size: 5 * 1024 * 1024, // 5MB
    parallel: true,
    num_threads: Some(4),
    ..Default::default()
};

let parser = Parser::with_config(config);
```

## Examples

See the [examples/](examples/) directory for more:

- [`basic_parse.rs`](examples/basic_parse.rs) - Parse a single file
- [`project_stats.rs`](examples/project_stats.rs) - Parse a project and show statistics
- [`call_graph.rs`](examples/call_graph.rs) - Extract function call relationships
- [`dependency_analysis.rs`](examples/dependency_analysis.rs) - Analyze import dependencies

Run examples with:

```bash
cargo run --example basic_parse
```

## Performance

Performance targets (measured on modern hardware):

- Single file (<1000 lines): **<10ms**
- Single file (1000-10000 lines): **<100ms**
- Project (100 files): **<1 second**
- Project (1000 files): **<10 seconds**
- Memory usage (1000 files): **<500MB**

## Python Support

Supports Python 3.8+ syntax including:

- ✅ Functions and async functions
- ✅ Classes with inheritance
- ✅ Decorators
- ✅ Type hints
- ✅ Match statements (Python 3.10+)
- ✅ Protocols and Abstract Base Classes
- ✅ Import statements (including wildcards)

## License

Licensed under the Apache License, Version 2.0 ([LICENSE](LICENSE) or http://www.apache.org/licenses/LICENSE-2.0).

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be licensed under the Apache-2.0 license, without any additional terms or conditions.
