# codegraph-php

PHP parser for CodeGraph - extracts code entities and relationships from PHP source files.

## Features

- Parse PHP 7.x and 8.x source files
- Extract functions, classes, interfaces, traits, and enums
- Track relationships (calls, imports, inheritance, implementations)
- Support for namespaces and use statements
- Full integration with codegraph-parser-api

## Supported PHP Constructs

| PHP Construct | Maps To |
|---------------|---------|
| `function` | `FunctionEntity` |
| `class method` | `FunctionEntity` (with `parent_class`) |
| `class` | `ClassEntity` |
| `interface` | `TraitEntity` |
| `trait` | `TraitEntity` |
| `enum` (PHP 8.1+) | `ClassEntity` (with `enum` attribute) |
| `namespace` | `ModuleEntity` |
| `use` statements | `ImportRelation` |
| `extends` | `InheritanceRelation` |
| `implements` | `ImplementationRelation` |
| Trait `use` | `ImplementationRelation` |

## Quick Start

```rust
use codegraph_php::PhpParser;
use codegraph_parser_api::CodeParser;
use codegraph::CodeGraph;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut graph = CodeGraph::in_memory()?;
    let parser = PhpParser::new();

    // Parse a file
    let file_info = parser.parse_file(Path::new("src/Controller.php"), &mut graph)?;
    println!("Parsed {} functions", file_info.functions.len());
    println!("Parsed {} classes", file_info.classes.len());

    // Or parse source directly
    let source = r#"<?php
class Example {
    public function test(): void {
        echo "Hello";
    }
}
"#;
    let file_info = parser.parse_source(source, Path::new("example.php"), &mut graph)?;
    println!("Parsed {} entities", file_info.entity_count());

    Ok(())
}
```

## Examples

Run the basic example:

```bash
cargo run --example basic_parse
```

## Testing

```bash
# Run all tests
cargo test -p codegraph-php

# Run specific test
cargo test -p codegraph-php test_parse_classes

# Run with verbose output
cargo test -p codegraph-php -- --nocapture
```

## Benchmarks

```bash
cargo bench -p codegraph-php
```

## License

Apache-2.0
