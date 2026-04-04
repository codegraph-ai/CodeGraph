# codegraph-java

Java parser for CodeGraph - extracts code entities and relationships from Java source files.

## Features

- Parse Java source files (.java)
- Extract classes, interfaces, enums, and records
- Extract methods and constructors
- Track relationships (calls, imports, inheritance, implementations)
- Support for Java packages and imports
- Full integration with codegraph-parser-api

## Usage

```rust
use codegraph_java::JavaParser;
use codegraph_parser_api::CodeParser;
use codegraph::CodeGraph;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut graph = CodeGraph::in_memory()?;
    let parser = JavaParser::new();

    let file_info = parser.parse_file(Path::new("Main.java"), &mut graph)?;
    println!("Parsed {} functions", file_info.functions.len());
    println!("Parsed {} classes", file_info.classes.len());
    Ok(())
}
```

## Entity Mapping

| Java Construct | Maps To |
|----------------|---------|
| `class` | `ClassEntity` |
| `interface` | `TraitEntity` |
| `enum` | `ClassEntity` (with "enum" attribute) |
| `record` | `ClassEntity` (with "record" attribute) |
| `method` | `FunctionEntity` |
| `constructor` | `FunctionEntity` |
| `package` | Used for qualified names |
| `import` | `ImportRelation` |
| `extends` (class) | `InheritanceRelation` |
| `implements` | `ImplementationRelation` |

## License

Apache-2.0
