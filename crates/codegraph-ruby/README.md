# codegraph-ruby

Ruby parser for CodeGraph - extracts code entities and relationships from Ruby source files.

## Features

- Parse Ruby source files (.rb, .rake, .gemspec)
- Extract classes, modules, methods (instance and singleton)
- Track relationships (calls, require/require_relative, inheritance, include/extend/prepend)
- Support for Ruby's mixins and module system
- Full integration with codegraph-parser-api

## Usage

```rust
use codegraph_ruby::RubyParser;
use codegraph_parser_api::CodeParser;
use codegraph::CodeGraph;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut graph = CodeGraph::in_memory()?;
    let parser = RubyParser::new();

    let file_info = parser.parse_file(Path::new("app.rb"), &mut graph)?;
    println!("Parsed {} methods", file_info.functions.len());
    println!("Parsed {} classes", file_info.classes.len());
    Ok(())
}
```

## Entity Mapping

| Ruby Construct | Maps To |
|----------------|---------|
| `class` | `ClassEntity` |
| `module` | `TraitEntity` |
| `def` | `FunctionEntity` |
| `def self.` | `FunctionEntity` (with is_static=true) |
| `require` | `ImportRelation` |
| `require_relative` | `ImportRelation` |
| `<` (inheritance) | `InheritanceRelation` |
| `include` | `ImplementationRelation` |
| `extend` | `ImplementationRelation` |
| `prepend` | `ImplementationRelation` |

## License

Apache-2.0
