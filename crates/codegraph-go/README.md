# codegraph-go

Go parser for CodeGraph - extracts code entities and relationships from Go source files.

## Features

- ✅ Parse Go source files (.go)
- ✅ Extract functions and methods
- ✅ Extract structs and interfaces
- ✅ Track imports
- ✅ Full integration with `codegraph-parser-api`

## Quick Start

```rust
use codegraph::CodeGraph;
use codegraph_parser_api::CodeParser;
use codegraph_go::GoParser;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut graph = CodeGraph::in_memory()?;
    let parser = GoParser::new();

    let file_info = parser.parse_file(
        Path::new("main.go"),
        &mut graph
    )?;

    println!("Parsed {} functions", file_info.functions.len());
    println!("Parsed {} structs", file_info.classes.len());

    Ok(())
}
```

## License

Apache-2.0
