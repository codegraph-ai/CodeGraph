# codegraph-rust

Rust parser for CodeGraph - extracts code entities and relationships from Rust source files.

## Features

- ✅ Parse Rust source files and extract:
  - Functions (including async functions)
  - Structs and enums
  - Traits and trait implementations
  - Modules
  - Generic types
  - Use statements (imports)
- ✅ Track relationships:
  - Function calls
  - Trait implementations
  - Trait inheritance
  - Module imports
- ✅ Full integration with `codegraph-parser-api`
- ✅ Support for Rust-specific constructs (impl blocks, associated functions, generics)
- ✅ Configurable behavior via `ParserConfig`

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
codegraph = "0.1"
codegraph-rust = "0.1"
codegraph-parser-api = "0.1"
```

### Basic Usage

```rust
use codegraph::CodeGraph;
use codegraph_parser_api::CodeParser;
use codegraph_rust::RustParser;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create an in-memory graph
    let mut graph = CodeGraph::in_memory()?;

    // Create the parser
    let parser = RustParser::new();

    // Parse a Rust file
    let file_info = parser.parse_file(
        Path::new("src/main.rs"),
        &mut graph
    )?;

    println!("Parsed {} functions", file_info.functions.len());
    println!("Parsed {} structs/enums", file_info.classes.len());
    println!("Parsed {} traits", file_info.traits.len());

    Ok(())
}
```

### Parse from String

```rust
use codegraph::CodeGraph;
use codegraph_parser_api::CodeParser;
use codegraph_rust::RustParser;
use std::path::Path;

let source = r#"
pub struct Person {
    pub name: String,
    age: u32,
}

impl Person {
    pub fn new(name: String, age: u32) -> Self {
        Self { name, age }
    }
}
"#;

let mut graph = CodeGraph::in_memory()?;
let parser = RustParser::new();

let info = parser.parse_source(source, Path::new("person.rs"), &mut graph)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Custom Configuration

```rust
use codegraph_parser_api::ParserConfig;
use codegraph_rust::RustParser;

let config = ParserConfig::default()
    .with_max_file_size(5 * 1024 * 1024) // 5 MB
    .with_parallel(true);

let parser = RustParser::with_config(config);
```

### Parse Multiple Files

```rust
use codegraph::CodeGraph;
use codegraph_parser_api::CodeParser;
use codegraph_rust::RustParser;
use std::path::PathBuf;

let mut graph = CodeGraph::in_memory()?;
let parser = RustParser::new();

let files = vec![
    PathBuf::from("src/main.rs"),
    PathBuf::from("src/lib.rs"),
    PathBuf::from("src/utils.rs"),
];

let project_info = parser.parse_files(&files, &mut graph)?;

println!("Total functions: {}", project_info.total_functions);
println!("Total classes: {}", project_info.total_classes);
println!("Success rate: {:.1}%", project_info.success_rate() * 100.0);
# Ok::<(), Box<dyn std::error::Error>>(())
```

### Parse Directory

```rust
use codegraph::CodeGraph;
use codegraph_parser_api::CodeParser;
use codegraph_rust::RustParser;
use std::path::Path;

let mut graph = CodeGraph::in_memory()?;
let parser = RustParser::new();

// Parse all .rs files in src/ directory recursively
let project_info = parser.parse_directory(
    Path::new("src"),
    &mut graph
)?;

println!("Parsed {} files", project_info.files.len());
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Supported Rust Constructs

### Functions
- ✅ Free functions
- ✅ Associated functions (static methods)
- ✅ Methods (with self parameter)
- ✅ Async functions
- ✅ Generic functions
- ✅ Function parameters with types
- ✅ Return types

### Structs and Enums
- ✅ Structs with named fields
- ✅ Tuple structs
- ✅ Unit structs
- ✅ Enums with variants
- ✅ Generic structs and enums
- ✅ Field visibility

### Traits
- ✅ Trait definitions
- ✅ Required methods
- ✅ Trait inheritance
- ✅ Generic traits

### Implementations
- ✅ Inherent impl blocks
- ✅ Trait impl blocks
- ✅ Generic implementations

### Imports
- ✅ Use statements
- ✅ Nested imports
- ✅ Wildcard imports
- ✅ Import aliases

### Documentation
- ✅ Doc comments (`///` and `//!`)
- ✅ Module-level documentation

## Configuration Options

The parser respects all `ParserConfig` options:

```rust
pub struct ParserConfig {
    pub skip_private: bool,        // Skip private items
    pub skip_tests: bool,          // Skip #[test] functions
    pub max_file_size: usize,      // Maximum file size to parse
    pub timeout_per_file: Option<Duration>, // Timeout per file
    pub parallel: bool,            // Enable parallel parsing
    pub include_docs: bool,        // Extract documentation
    pub extract_types: bool,       // Extract type information
}
```

## Graph Structure

The parser creates the following node types in the graph:

- **File**: Represents the Rust source file/module
- **Function**: Represents functions and methods
- **Class**: Represents structs and enums
- **Trait**: Represents trait definitions
- **Import**: Represents use statements

And the following edge types:

- **Contains**: File/Class → Function/Field
- **Calls**: Function → Function
- **Implements**: Struct/Enum → Trait
- **Inherits**: Trait → Trait (trait inheritance)
- **Imports**: File → Import

## Performance

The parser uses the `syn` crate for Rust parsing, which is fast and battle-tested:

- **Single file** (100 LOC): ~5-10ms
- **1K files**: ~5-10 seconds
- **10K files**: ~50-100 seconds

Enable parallel parsing for large projects:

```rust
let config = ParserConfig::default().with_parallel(true);
let parser = RustParser::with_config(config);
```

## Examples

See the `tests/` directory for comprehensive examples.

## License

Apache-2.0

## Contributing

Contributions are welcome! Please see the main CodeGraph repository for guidelines.
