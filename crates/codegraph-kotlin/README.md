# codegraph-kotlin

Kotlin parser for CodeGraph - extracts code entities and relationships from Kotlin source files.

## Features

- Parse Kotlin source files (.kt, .kts)
- Extract classes, interfaces, objects, data classes, enum classes
- Extract functions (including suspend functions)
- Track relationships (calls, imports, inheritance, implementations)
- Support for Kotlin packages and imports
- Full integration with codegraph-parser-api

## Usage

```rust
use codegraph_kotlin::KotlinParser;
use codegraph_parser_api::CodeParser;
use codegraph::CodeGraph;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut graph = CodeGraph::in_memory()?;
    let parser = KotlinParser::new();

    let file_info = parser.parse_file(Path::new("Main.kt"), &mut graph)?;
    println!("Parsed {} functions", file_info.functions.len());
    println!("Parsed {} classes", file_info.classes.len());
    Ok(())
}
```

## Entity Mapping

| Kotlin Construct | Maps To |
|------------------|---------|
| `class` | `ClassEntity` |
| `interface` | `TraitEntity` |
| `object` | `ClassEntity` (with "object" attribute) |
| `data class` | `ClassEntity` (with "data" attribute) |
| `enum class` | `ClassEntity` (with "enum" attribute) |
| `sealed class` | `ClassEntity` (with "sealed" attribute) |
| `fun` | `FunctionEntity` |
| `suspend fun` | `FunctionEntity` (with is_async=true) |
| `package` | Used for qualified names |
| `import` | `ImportRelation` |
| `:` (inheritance) | `InheritanceRelation` / `ImplementationRelation` |

## License

Apache-2.0
