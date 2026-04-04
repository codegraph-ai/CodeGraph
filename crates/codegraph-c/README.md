# codegraph-c

C language parser for the CodeGraph framework.

## Overview

`codegraph-c` provides robust C source code parsing with specialized support for Linux kernel and system-level code. It uses tree-sitter for fault-tolerant AST generation and includes a sophisticated preprocessing pipeline that handles:

- GCC extensions (`__attribute__`, `__asm__`, `typeof`, etc.)
- Linux kernel macros (`__init`, `__exit`, `container_of`, etc.)
- Preprocessor conditionals (`#if 0`, `#ifdef`, etc.)
- Platform-specific code (Linux, FreeBSD, Darwin)

## Features

- **Fault-Tolerant Parsing**: Extracts entities even from code with syntax errors
- **Layered Pipeline**: Multi-stage preprocessing for kernel code
- **Platform Detection**: Automatic detection of target platform with confidence scoring
- **Entity Extraction**: Functions, structs, unions, enums, typedefs
- **Relationship Tracking**: Include directives, function calls
- **Complexity Metrics**: Cyclomatic complexity calculation
- **Graph Integration**: Full integration with `codegraph` for code analysis

## Usage

### Basic Parsing

```rust
use codegraph_c::CParser;
use codegraph_parser_api::CodeParser;
use codegraph::CodeGraph;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut graph = CodeGraph::in_memory()?;
    let parser = CParser::new();

    let file_info = parser.parse_file(Path::new("main.c"), &mut graph)?;
    println!("Parsed {} functions", file_info.functions.len());
    Ok(())
}
```

### Kernel Code Parsing

For code with kernel-specific constructs:

```rust
use codegraph_c::{CParser, ExtractionOptions};
use codegraph_c::extractor::extract_with_options;
use codegraph_parser_api::ParserConfig;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = r#"
        #include <linux/module.h>

        static __init int my_init(void) {
            return 0;
        }
    "#;

    let options = ExtractionOptions::for_kernel_code();
    let result = extract_with_options(
        source,
        Path::new("driver.c"),
        &ParserConfig::default(),
        &options
    )?;

    println!("Extracted {} functions", result.ir.functions.len());
    Ok(())
}
```

### Pipeline Processing

For maximum control over preprocessing:

```rust
use codegraph_c::pipeline::{Pipeline, PipelineConfig};

fn main() {
    let pipeline = Pipeline::new();
    let config = PipelineConfig::for_kernel_code();

    let source = r#"
        #include <linux/module.h>
        MODULE_LICENSE("GPL");
        static __init int my_init(void) { return 0; }
    "#;

    let result = pipeline.process(source, &config);
    println!("Platform: {} (confidence: {:.0}%)",
        result.platform.platform_id,
        result.platform.confidence * 100.0);
    println!("Processed source ready for parsing");
}
```

## Architecture

The parser uses a layered processing pipeline:

```
Source Code
    │
    ▼
┌─────────────────────────────────────┐
│  1. Platform Detection              │
│     Detect Linux/FreeBSD/Darwin     │
└─────────────────────────────────────┘
    │
    ▼
┌─────────────────────────────────────┐
│  2. Conditional Evaluation          │
│     Strip #if 0 blocks              │
└─────────────────────────────────────┘
    │
    ▼
┌─────────────────────────────────────┐
│  3. GCC Neutralization              │
│     Handle __attribute__, etc.      │
└─────────────────────────────────────┘
    │
    ▼
┌─────────────────────────────────────┐
│  4. Macro Expansion                 │
│     Expand kernel macros            │
└─────────────────────────────────────┘
    │
    ▼
┌─────────────────────────────────────┐
│  5. tree-sitter Parsing             │
│     Fault-tolerant AST              │
└─────────────────────────────────────┘
    │
    ▼
┌─────────────────────────────────────┐
│  6. Entity Extraction               │
│     Functions, structs, calls       │
└─────────────────────────────────────┘
```

## Supported Constructs

### Type Definitions
- `u8`, `u16`, `u32`, `u64`, `s8`, `s16`, `s32`, `s64`
- `size_t`, `ssize_t`, `uintptr_t`, `intptr_t`
- `__le16`, `__le32`, `__be16`, `__be32` (kernel types)
- Boolean types: `bool`, `_Bool`

### Attributes (stripped)
- `__init`, `__exit`, `__user`, `__kernel`
- `__iomem`, `__force`, `__percpu`, `__rcu`
- `__must_check`, `__always_inline`, `__noinline`
- `__section(...)`, `__aligned(...)`

### GCC Extensions (neutralized)
- `__attribute__((...))`
- `__extension__`
- `__asm__`, `__asm volatile`
- `typeof()`, `__typeof__()`
- Statement expressions `({ ... })`

### Macros (expanded or neutralized)
- `container_of()`, `offsetof()`
- `likely()`, `unlikely()`
- `BUILD_BUG_ON()`, `WARN_ON()`
- `list_for_each()` and iterator macros

## Testing

```bash
# Run all tests
cargo test -p codegraph-c

# Run integration tests
cargo test -p codegraph-c --test integration_tests
```

## Performance

The parser achieves good parsing rates on real-world kernel code:

| Codebase | Files | Clean Parse Rate |
|----------|-------|------------------|
| ICE Driver | 84 | 75%+ |
| i915 Graphics | 526 | 70%+ |
| NVMe CLI | 156 | 85%+ |
| netperf | 47 | 94%+ |

## License

MIT OR Apache-2.0
