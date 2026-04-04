// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST extraction for SystemVerilog/Verilog source code

use codegraph_parser_api::{CodeIR, ModuleEntity, ParserConfig, ParserError};
use std::path::Path;
use tree_sitter::Parser;

use crate::visitor::VerilogVisitor;

/// Determine language string from file extension.
/// `.sv`/`.svh` → "systemverilog", `.v`/`.vh` → "verilog".
fn language_for_path(file_path: &Path) -> &'static str {
    match file_path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "sv" | "svh" => "systemverilog",
        _ => "verilog",
    }
}

/// Extract code entities and relationships from SystemVerilog/Verilog source code
pub fn extract(
    source: &str,
    file_path: &Path,
    _config: &ParserConfig,
) -> Result<CodeIR, ParserError> {
    let mut parser = Parser::new();
    let language = crate::ts_verilog::language();
    parser
        .set_language(&language)
        .map_err(|e| ParserError::ParseError(file_path.to_path_buf(), e.to_string()))?;

    let tree = parser.parse(source, None).ok_or_else(|| {
        ParserError::ParseError(file_path.to_path_buf(), "Failed to parse".to_string())
    })?;

    let root_node = tree.root_node();
    if root_node.has_error() {
        return Err(ParserError::SyntaxError(
            file_path.to_path_buf(),
            0,
            0,
            "Syntax error in SystemVerilog/Verilog source".to_string(),
        ));
    }

    let mut ir = CodeIR::new(file_path.to_path_buf());

    let module_name = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();
    ir.module = Some(ModuleEntity {
        name: module_name,
        path: file_path.display().to_string(),
        language: language_for_path(file_path).to_string(),
        line_count: source.lines().count(),
        doc_comment: None,
        attributes: Vec::new(),
    });

    let mut visitor = VerilogVisitor::new(source.as_bytes());
    visitor.visit_node(root_node);

    ir.classes = visitor.modules;
    ir.functions = visitor.functions;
    ir.imports = visitor.imports;
    ir.calls = visitor.calls;

    Ok(ir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_for_path() {
        assert_eq!(language_for_path(Path::new("foo.sv")), "systemverilog");
        assert_eq!(language_for_path(Path::new("foo.svh")), "systemverilog");
        assert_eq!(language_for_path(Path::new("foo.v")), "verilog");
        assert_eq!(language_for_path(Path::new("foo.vh")), "verilog");
    }

    #[test]
    fn test_extract_simple_module() {
        let source = "module counter (input clk, input reset); endmodule\n";
        let config = ParserConfig::default();
        let result = extract(source, Path::new("counter.v"), &config);

        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "counter");
    }

    #[test]
    fn test_extract_module_with_function() {
        let source = r#"
module math_unit ();
  function integer add;
    input a, b;
    add = a + b;
  endfunction
endmodule
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("math.v"), &config);

        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert!(!ir.functions.is_empty(), "Expected at least one function");
    }

    #[test]
    fn test_extract_module_instantiation() {
        let source = r#"
module top ();
  counter u1 (.clk(clk));
endmodule
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("top.v"), &config);

        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert!(!ir.calls.is_empty(), "Expected module instantiation call");
        assert_eq!(ir.calls[0].callee, "counter");
    }

    #[test]
    fn test_extract_module_info() {
        let source = "module top (); endmodule\n";
        let config = ParserConfig::default();
        let result = extract(source, Path::new("my_module.v"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert!(ir.module.is_some());
        let module = ir.module.unwrap();
        assert_eq!(module.name, "my_module");
        assert_eq!(module.language, "verilog");
        assert!(module.line_count > 0);
    }

    #[test]
    fn test_extract_sv_language_tag() {
        let source = "module top (); endmodule\n";
        let config = ParserConfig::default();
        let result = extract(source, Path::new("my_module.sv"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.module.unwrap().language, "systemverilog");
    }

    #[test]
    fn test_extract_multiple_modules() {
        let source = r#"
module adder (input a, b, output sum);
  assign sum = a + b;
endmodule

module subtractor (input a, b, output diff);
  assign diff = a - b;
endmodule
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("alu.v"), &config);

        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 2);
        let names: Vec<&str> = ir.classes.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"adder"));
        assert!(names.contains(&"subtractor"));
    }

    #[test]
    fn test_extract_with_syntax_error() {
        let source = "module broken ( {\n";
        let config = ParserConfig::default();
        let result = extract(source, Path::new("broken.v"), &config);

        assert!(result.is_err());
    }

    #[test]
    fn test_extract_sv_interface() {
        let source = "interface my_bus; logic clk; endinterface\n";
        let config = ParserConfig::default();
        let result = extract(source, Path::new("bus.sv"), &config);

        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "my_bus");
        assert!(ir.classes[0].is_interface);
    }

    #[test]
    fn test_extract_sv_class() {
        let source = "class Packet; int data; endclass\n";
        let config = ParserConfig::default();
        let result = extract(source, Path::new("packet.sv"), &config);

        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "Packet");
    }

    #[test]
    fn test_extract_sv_package() {
        let source = "package my_pkg; typedef int my_int; endpackage\n";
        let config = ParserConfig::default();
        let result = extract(source, Path::new("pkg.sv"), &config);

        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "my_pkg");
    }

    #[test]
    fn test_extract_sv_program() {
        let source = "program my_test; initial begin end endprogram\n";
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.sv"), &config);

        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "my_test");
    }
}
