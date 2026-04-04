# codegraph-typescript

TypeScript/JavaScript parser for CodeGraph - extracts code entities and relationships from TS/JS source files.

## Version 0.3.0 - Cyclomatic Complexity Analysis

**codegraph-typescript v0.3.0** adds AST-based cyclomatic complexity calculation for all functions using tree-sitter.

### Complexity Metrics

```rust
use codegraph_parser_api::CodeParser;
use codegraph_typescript::TypeScriptParser;

let parser = TypeScriptParser::new();
let ir = parser.parse_source(source, Path::new("example.ts"))?;

for func in &ir.functions {
    if let Some(complexity) = &func.complexity {
        println!("{}: CC={} Grade={}",
            func.name,
            complexity.cyclomatic_complexity,
            complexity.grade()
        );
    }
}
```

### What's Analyzed
- if/else statements and ternary operators
- switch/case statements
- for, while, do-while loops
- try/catch exception handling
- Logical operators (&&, ||)
- Nesting depth tracking

## Features

- ✅ Parse TypeScript and JavaScript files (.ts, .tsx, .js, .jsx)
- ✅ Extract functions (including arrow functions, async functions)
- ✅ Extract classes and interfaces
- ✅ Track imports and exports
- ✅ **Cyclomatic complexity analysis** (v0.3.0)
- ✅ Full integration with `codegraph-parser-api`

## Quick Start

```rust
use codegraph::CodeGraph;
use codegraph_parser_api::CodeParser;
use codegraph_typescript::TypeScriptParser;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut graph = CodeGraph::in_memory()?;
    let parser = TypeScriptParser::new();

    let file_info = parser.parse_file(
        Path::new("src/index.ts"),
        &mut graph
    )?;

    println!("Parsed {} functions", file_info.functions.len());
    println!("Parsed {} classes", file_info.classes.len());

    Ok(())
}
```

## Supported Features

- Functions (regular, arrow, async, generator)
- Classes (including methods, properties, constructors)
- Interfaces
- Import/export statements
- TypeScript type annotations
- JSX/TSX syntax

## License

Apache-2.0
