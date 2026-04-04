# codegraph-tcl

Tcl/SDC/UPF parser for CodeGraph — extracts code entities, EDA commands, and SDC constraints from Tcl source files into a queryable graph database.

## Features

- **Multi-format support**: Parses `.tcl`, `.sdc`, and `.upf` files
- **EDA/VLSI domain awareness**: Recognizes Synopsys, Cadence, and OpenROAD command patterns
- **SDC constraint extraction**: Parses clocks, delays, timing exceptions, and false paths
- **Full CodeParser trait**: Integrates with the codegraph parser ecosystem
- **Complexity analysis**: Cyclomatic complexity for all procedures
- **55 tests** across visitor, SDC, EDA, extractor, and parser modules
- **Criterion benchmarks** included

## What it Extracts

### Tcl Entities
- **Procedures**: Names, parameters (with defaults), variadic args, signatures
- **Namespaces**: `namespace eval` blocks mapped as classes with qualified proc names
- **Imports**: `source` commands and `package require` statements
- **Calls**: Function/command call relationships (who calls whom)
- **Doc comments**: `#` comments preceding procedures

### SDC Constraints
- **Clocks**: `create_clock`, `create_generated_clock` with name, period, waveform
- **IO Delays**: `set_input_delay`, `set_output_delay` with clock reference
- **Timing Exceptions**: `set_false_path`, `set_multicycle_path`, `set_max_delay`, `set_min_delay`

### EDA Commands
- **Design reads**: `read_verilog`, `read_vhdl`, `read_def`, `read_lef`, `read_sdc`, etc.
- **Design writes**: `write_verilog`, `write_def`, `write_sdf`, etc.
- **Tool flow**: `compile`, `compile_ultra`, `report_timing`, `report_area`, `place_opt`, etc.
- **Object queries**: `get_ports`, `get_pins`, `get_cells`, `get_nets`, `get_clocks`
- **Command registration**: `define_proc_attributes`
- **Collection iteration**: `foreach_in_collection`

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
codegraph = "0.2"
codegraph-tcl = "0.1"
codegraph-parser-api = "0.2"
```

### Basic Usage

```rust
use codegraph::CodeGraph;
use codegraph_parser_api::CodeParser;
use codegraph_tcl::TclParser;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut graph = CodeGraph::in_memory()?;
    let parser = TclParser::new();

    let file_info = parser.parse_file(
        Path::new("scripts/flow.tcl"),
        &mut graph,
    )?;

    println!("Parsed {} procedures", file_info.functions.len());
    println!("Parsed {} namespaces", file_info.classes.len());
    println!("Found {} imports", file_info.imports.len());

    Ok(())
}
```

### Parse from String

```rust
use codegraph::CodeGraph;
use codegraph_parser_api::CodeParser;
use codegraph_tcl::TclParser;
use std::path::Path;

let source = r#"
package require Tcl 8.6
source helpers.tcl

namespace eval utils {
    proc add {a b} {
        expr {$a + $b}
    }
}

proc greet {name {greeting "Hello"}} {
    puts "$greeting, $name!"
}
"#;

let mut graph = CodeGraph::in_memory()?;
let parser = TclParser::new();
let info = parser.parse_source(source, Path::new("example.tcl"), &mut graph)?;
```

### Parse SDC Constraints

```rust
use codegraph::CodeGraph;
use codegraph_parser_api::CodeParser;
use codegraph_tcl::TclParser;
use std::path::Path;

let sdc_source = r#"
create_clock -name sys_clk -period 10.0 [get_ports clk]
set_input_delay -clock sys_clk -max 2.0 [all_inputs]
set_false_path -from [get_clocks pll_clk] -to [get_clocks sys_clk]
"#;

let mut graph = CodeGraph::in_memory()?;
let parser = TclParser::new();
let info = parser.parse_source(sdc_source, Path::new("timing.sdc"), &mut graph)?;
```

SDC data is stored as JSON-serialized properties on the file node:
- `sdc_clocks`: Array of clock definitions
- `sdc_io_delays`: Array of input/output delay constraints
- `sdc_timing_exceptions`: Array of false paths, multicycle paths, etc.

### Parse EDA Flow Scripts

```rust
let eda_source = r#"
read_verilog top.v
read_verilog -sv pkg.sv
read_sdc timing.sdc
compile_ultra
report_timing -delay_type max
write_verilog synthesized.v
"#;
```

EDA data is stored as properties on the file node:
- `eda_design_reads`: Array of `[format, path]` pairs
- `eda_design_writes`: Array of `[format, path]` pairs
- `eda_registered_commands`: Array of `[name, usage]` pairs

Design file reads are also recorded as import relationships for dependency tracking.

## Architecture

Follows the standard codegraph parser pipeline:

```
Source code → tree-sitter AST → Visitor → CodeIR → Mapper → CodeGraph
```

| Module | Purpose |
|--------|---------|
| `parser_impl.rs` | `CodeParser` trait implementation |
| `extractor.rs` | Source → `CodeIR` using tree-sitter |
| `visitor.rs` | AST traversal with ERROR node resolution |
| `mapper.rs` | `CodeIR` → graph nodes and edges |
| `sdc.rs` | SDC constraint extraction |
| `eda.rs` | EDA command classification |

### ERROR Node Resolution

The vendored tree-sitter-tcl grammar uses ABI v15, but the workspace requires ABI v14. The version downgrade causes 14 Tcl keywords (`proc`, `namespace`, `if`, `while`, `foreach`, `set`, `global`, `expr`, `try`, `catch`, `finally`, `elseif`, `else`, `regexp`) to parse as ERROR nodes instead of proper named AST nodes.

The visitor handles this transparently:

- `resolve_error_keyword()` scans ERROR node children for recognizable keywords
- `resolve_kind()` normalizes ERROR → keyword and `procedure` → `proc`
- All dispatch code sees resolved kinds — never raw "ERROR"

## Graph Structure

The parser creates the following node types:

| Node Type | Represents | Properties |
|-----------|-----------|------------|
| `File` | Source file | `path`, `language`, SDC/EDA data |
| `Function` | `proc` definition | `signature`, `visibility`, `complexity` |
| `Class` | `namespace eval` block | `attributes: ["namespace"]` |
| `Import` | `source` / `package require` | `imported`, `is_wildcard` |

Edge types:

| Edge Type | Relationship |
|-----------|-------------|
| `Contains` | File/Namespace → Function |
| `Calls` | Function → Function/Command |
| `Imports` | File → Imported file/package |

## Supported File Extensions

| Extension | Description |
|-----------|-------------|
| `.tcl` | Tcl scripts |
| `.sdc` | Synopsys Design Constraints |
| `.upf` | Unified Power Format |

## Performance

Benchmarks use Criterion (run with `cargo bench -p codegraph-tcl`):

- **Single file** (<500 LOC): ~1-5ms
- **SDC file** (constraints only): ~1ms

## Known Limitations

The ABI v15 → v14 downgrade causes tree-sitter-tcl to produce ERROR nodes for grammar keywords. While the visitor resolves these transparently, cascading parse errors can occur in deeply nested multi-keyword structures (e.g., a namespace containing multiple procedures each with complex control flow). The parser handles each construct reliably in isolation and in moderate combinations.

## License

Apache-2.0
