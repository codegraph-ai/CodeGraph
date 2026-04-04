// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for codegraph-verilog parser

use codegraph::CodeGraph;
use codegraph_parser_api::CodeParser;
use codegraph_verilog::VerilogParser;
use std::path::Path;

#[test]
fn test_parse_simple_module() {
    let source = "module counter (input clk, input reset); endmodule\n";

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = VerilogParser::new();

    let result = parser.parse_source(source, Path::new("counter.v"), &mut graph);
    assert!(result.is_ok(), "Failed: {:?}", result.err());

    let info = result.unwrap();
    assert_eq!(info.classes.len(), 1);
}

#[test]
fn test_parse_module_with_ports() {
    let source = r#"
module adder (
    input  [7:0] a,
    input  [7:0] b,
    output [8:0] sum
);
    assign sum = a + b;
endmodule
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = VerilogParser::new();

    let result = parser.parse_source(source, Path::new("adder.v"), &mut graph);
    assert!(result.is_ok(), "Failed: {:?}", result.err());

    let info = result.unwrap();
    assert_eq!(info.classes.len(), 1);

    // Verify the module name was extracted
    let module_node = graph.get_node(info.classes[0]).unwrap();
    assert_eq!(
        module_node.properties.get_string("name"),
        Some("adder"),
        "Module name should be 'adder'"
    );
}

#[test]
fn test_parse_multiple_modules() {
    let source = r#"
module adder (input a, b, output sum);
    assign sum = a + b;
endmodule

module subtractor (input a, b, output diff);
    assign diff = a - b;
endmodule
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = VerilogParser::new();

    let result = parser.parse_source(source, Path::new("alu.v"), &mut graph);
    assert!(result.is_ok(), "Failed: {:?}", result.err());

    let info = result.unwrap();
    assert_eq!(info.classes.len(), 2, "Expected 2 modules");
}

#[test]
fn test_parse_module_instantiation() {
    let source = r#"
module top ();
    wire clk, reset;
    wire [7:0] count;
    counter u1 (.clk(clk), .reset(reset), .count(count));
endmodule
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = VerilogParser::new();

    let result = parser.parse_source(source, Path::new("top.v"), &mut graph);
    assert!(result.is_ok(), "Failed: {:?}", result.err());

    let info = result.unwrap();
    assert_eq!(info.classes.len(), 1);

    // Module instantiation 'counter' should be recorded as a call
    let top_module_id = info.classes[0];
    let top_node = graph.get_node(top_module_id).unwrap();
    let unresolved = top_node
        .properties
        .get_string_list_compat("unresolved_calls")
        .unwrap_or_default();
    assert!(
        unresolved.contains(&"counter".to_string()),
        "Expected 'counter' in unresolved calls, got: {:?}",
        unresolved
    );
}

#[test]
fn test_parse_always_block() {
    let source = r#"
module counter (input clk, input reset, output reg [7:0] count);
    always @(posedge clk) begin
        if (reset)
            count <= 8'b0;
        else
            count <= count + 1;
    end
endmodule
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = VerilogParser::new();

    let result = parser.parse_source(source, Path::new("counter.v"), &mut graph);
    assert!(result.is_ok(), "Failed: {:?}", result.err());

    let info = result.unwrap();
    assert_eq!(info.classes.len(), 1);
}

#[test]
fn test_parse_function() {
    let source = r#"
module math_unit ();
    function integer add_values;
        input integer a, b;
        add_values = a + b;
    endfunction
endmodule
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = VerilogParser::new();

    let result = parser.parse_source(source, Path::new("math.v"), &mut graph);
    assert!(result.is_ok(), "Failed: {:?}", result.err());

    let info = result.unwrap();
    assert_eq!(info.classes.len(), 1);
    assert!(!info.functions.is_empty(), "Expected at least one function");
}

#[test]
fn test_parse_task() {
    let source = r#"
module testbench ();
    task display_value;
        input integer val;
        $display("Value = %d", val);
    endtask
endmodule
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = VerilogParser::new();

    let result = parser.parse_source(source, Path::new("tb.v"), &mut graph);
    assert!(result.is_ok(), "Failed: {:?}", result.err());

    let info = result.unwrap();
    assert_eq!(info.classes.len(), 1);
    assert!(!info.functions.is_empty(), "Expected task as function");
}

#[test]
fn test_parse_empty_module() {
    let source = "module empty (); endmodule\n";

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = VerilogParser::new();

    let result = parser.parse_source(source, Path::new("empty.v"), &mut graph);
    assert!(result.is_ok(), "Failed: {:?}", result.err());

    let info = result.unwrap();
    assert_eq!(info.classes.len(), 1);
    assert_eq!(info.functions.len(), 0);
}

#[test]
fn test_parse_syntax_error() {
    let source = "module broken (  {\n";

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = VerilogParser::new();

    let result = parser.parse_source(source, Path::new("broken.v"), &mut graph);
    assert!(result.is_err(), "Expected parse error for broken syntax");
}

#[test]
fn test_parser_info() {
    let parser = VerilogParser::new();
    assert_eq!(parser.language(), "systemverilog");
    assert!(parser.can_parse(Path::new("counter.v")));
    assert!(parser.can_parse(Path::new("defs.vh")));
    assert!(parser.can_parse(Path::new("design.sv")));
    assert!(parser.can_parse(Path::new("defs.svh")));
    assert!(!parser.can_parse(Path::new("main.go")));
}

#[test]
fn test_parse_sv_interface() {
    let source = "interface my_bus; logic clk; endinterface\n";

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = VerilogParser::new();

    let result = parser.parse_source(source, Path::new("bus.sv"), &mut graph);
    assert!(result.is_ok(), "Failed: {:?}", result.err());

    let info = result.unwrap();
    assert_eq!(info.classes.len(), 1, "Expected 1 interface");
    let node = graph.get_node(info.classes[0]).unwrap();
    assert_eq!(node.properties.get_string("name"), Some("my_bus"));
}

#[test]
fn test_parse_sv_class() {
    let source = "class Packet; int data; endclass\n";

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = VerilogParser::new();

    let result = parser.parse_source(source, Path::new("packet.sv"), &mut graph);
    assert!(result.is_ok(), "Failed: {:?}", result.err());

    let info = result.unwrap();
    assert_eq!(info.classes.len(), 1);
    let node = graph.get_node(info.classes[0]).unwrap();
    assert_eq!(node.properties.get_string("name"), Some("Packet"));
}

#[test]
fn test_parse_sv_package() {
    let source = "package my_pkg; endpackage\n";

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = VerilogParser::new();

    let result = parser.parse_source(source, Path::new("pkg.sv"), &mut graph);
    assert!(result.is_ok(), "Failed: {:?}", result.err());

    let info = result.unwrap();
    assert_eq!(info.classes.len(), 1);
    let node = graph.get_node(info.classes[0]).unwrap();
    assert_eq!(node.properties.get_string("name"), Some("my_pkg"));
}

#[test]
fn test_parse_sv_language_tag() {
    let parser = VerilogParser::new();
    let mut graph = CodeGraph::in_memory().unwrap();
    let source = "module top(); endmodule\n";

    let sv_result = parser.parse_source(source, Path::new("top.sv"), &mut graph);
    assert!(sv_result.is_ok());

    let v_result = parser.parse_source(source, Path::new("top.v"), &mut graph);
    assert!(v_result.is_ok());
}

#[test]
fn test_parser_metrics() {
    use std::io::Write;
    use tempfile::NamedTempFile;

    let source = "module mod1 (); endmodule\n";

    let mut temp_file = NamedTempFile::with_suffix(".v").unwrap();
    write!(temp_file, "{source}").unwrap();
    temp_file.flush().unwrap();

    let mut graph = CodeGraph::in_memory().unwrap();
    let mut parser = VerilogParser::new();

    let _ = parser.parse_file(temp_file.path(), &mut graph);

    let metrics = parser.metrics();
    assert_eq!(metrics.files_attempted, 1);
    assert_eq!(metrics.files_succeeded, 1);

    parser.reset_metrics();
    let metrics = parser.metrics();
    assert_eq!(metrics.files_attempted, 0);
}

#[test]
fn test_calls_edges() {
    // Module instantiation creates a call relationship
    let source = r#"
module parent ();
    child u1 ();
    child u2 ();
endmodule
"#;

    let mut graph = CodeGraph::in_memory().unwrap();
    let parser = VerilogParser::new();

    let result = parser.parse_source(source, Path::new("test.v"), &mut graph);
    assert!(result.is_ok(), "Failed: {:?}", result.err());

    let info = result.unwrap();

    // 'parent' module should be present
    let parent_id = graph
        .query()
        .node_type(codegraph::NodeType::Class)
        .execute()
        .unwrap()
        .into_iter()
        .find(|&id| {
            graph
                .get_node(id)
                .map(|n| n.properties.get_string("name") == Some("parent"))
                .unwrap_or(false)
        })
        .expect("Should find 'parent' module node");

    // parent should have a Contains edge from the file
    use codegraph::Direction;
    let parents = graph
        .get_neighbors(parent_id, Direction::Incoming)
        .unwrap_or_default();
    assert!(
        !parents.is_empty(),
        "Expected 'parent' to have a parent (Contains edge from file)"
    );

    // Unresolved calls should include 'child'
    let parent_node = graph.get_node(parent_id).unwrap();
    let unresolved = parent_node
        .properties
        .get_string_list_compat("unresolved_calls")
        .unwrap_or_default();
    assert!(
        unresolved.contains(&"child".to_string()),
        "Expected 'child' in unresolved calls, got: {:?}",
        unresolved
    );

    let _ = info;
}
