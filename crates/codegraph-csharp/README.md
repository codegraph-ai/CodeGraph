# codegraph-csharp

C# parser for CodeGraph - extracts code entities and relationships from C# source files.

## Features

- Parse C# source files (.cs)
- Extract classes, interfaces, structs, enums, records, and methods
- Track relationships (calls, using directives, inheritance, implementations)
- Support for namespaces, async methods, and generics
- Full integration with codegraph-parser-api

## Usage

```rust
use codegraph_csharp::CSharpParser;
use codegraph_parser_api::CodeParser;
use codegraph::CodeGraph;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut graph = CodeGraph::in_memory()?;
    let parser = CSharpParser::new();

    let file_info = parser.parse_file(Path::new("Program.cs"), &mut graph)?;
    println!("Parsed {} methods", file_info.functions.len());
    println!("Parsed {} classes", file_info.classes.len());
    Ok(())
}
```

## Entity Mapping

| C# Construct | Maps To |
|--------------|---------|
| `class` | `ClassEntity` |
| `interface` | `TraitEntity` |
| `struct` | `ClassEntity` (with attribute) |
| `enum` | `ClassEntity` (with attribute) |
| `record` | `ClassEntity` (with attribute) |
| `method` | `FunctionEntity` |
| `constructor` | `FunctionEntity` |
| `using` | `ImportRelation` |
| `:` (inheritance) | `InheritanceRelation` |
| `:` (implementation) | `ImplementationRelation` |

## License

Apache-2.0
