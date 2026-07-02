// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST extraction for Tcl source code

use codegraph_parser_api::{CodeIR, ModuleEntity, ParserConfig, ParserError};
use std::path::Path;
use tree_sitter::Parser;

use crate::eda::EdaData;
use crate::sdc::SdcData;
use crate::visitor::TclVisitor;

/// Extra Tcl-specific data beyond standard CodeIR
pub struct TclExtraData {
    pub sdc: SdcData,
    pub eda: EdaData,
}

/// Extract code entities and relationships from Tcl source code
pub fn extract(
    source: &str,
    file_path: &Path,
    _config: &ParserConfig,
) -> Result<(CodeIR, TclExtraData), ParserError> {
    let mut parser = Parser::new();
    let language = crate::ts_tcl::language();
    parser
        .set_language(&language)
        .map_err(|e| ParserError::ParseError(file_path.to_path_buf(), e.to_string()))?;

    let tree = parser.parse(source, None).ok_or_else(|| {
        ParserError::ParseError(file_path.to_path_buf(), "Failed to parse".to_string())
    })?;

    let root_node = tree.root_node();
    // Note: NOT checking root_node.has_error() here — tree-sitter-tcl's vendored grammar
    // intentionally produces ERROR nodes for keywords (proc, namespace, if, etc.) which
    // are resolved by sibling-stitching in the visitor. has_error() would reject valid Tcl.

    let mut ir = CodeIR::new(file_path.to_path_buf());

    let module_name = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    ir.module = Some(ModuleEntity {
        name: module_name,
        path: file_path.display().to_string(),
        language: "tcl".to_string(),
        line_count: source.lines().count(),
        doc_comment: None,
        attributes: Vec::new(),
    });

    let mut visitor = TclVisitor::new(source.as_bytes());
    visitor.visit_node(root_node);

    ir.functions = visitor.functions;
    ir.classes = visitor.classes;
    ir.imports = visitor.imports;
    ir.calls = visitor.calls;

    let extra = TclExtraData {
        sdc: visitor.sdc_data,
        eda: visitor.eda_data,
    };

    Ok((ir, extra))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_simple_proc() {
        let source = r#"
proc greet {name} {
    puts "Hello $name"
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("greet.tcl"), &config);

        assert!(result.is_ok());
        let (ir, _extra) = result.unwrap();
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].name, "greet");
    }

    #[test]
    fn test_extract_sdc_file() {
        let source = r#"
create_clock -name clk -period 10 [get_ports clk_in]
set_input_delay -clock clk 0.5 [all_inputs]
set_false_path -from [get_clocks clk1] -to [get_clocks clk2]
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("constraints.sdc"), &config);

        assert!(result.is_ok());
        let (_ir, extra) = result.unwrap();
        assert!(!extra.sdc.clocks.is_empty());
    }

    #[test]
    fn test_extract_eda_flow() {
        let source = r#"
read_verilog design.v
read_liberty lib.db
link_design
compile
report_timing
write_verilog synth_design.v
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("synth.tcl"), &config);

        assert!(result.is_ok());
        let (ir, extra) = result.unwrap();
        assert!(!extra.eda.design_reads.is_empty());
        assert!(!extra.eda.design_writes.is_empty());
        assert!(!ir.imports.is_empty());
    }

    #[test]
    fn test_extract_module_info() {
        let source = "proc foo {} {}";
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.tcl"), &config);

        assert!(result.is_ok());
        let (ir, _) = result.unwrap();
        assert!(ir.module.is_some());
        let module = ir.module.unwrap();
        assert_eq!(module.name, "test");
        assert_eq!(module.language, "tcl");
    }

    #[test]
    fn test_extract_calls() {
        // A single proc body — visitor correctly sets current_procedure here
        let source = r#"
proc caller {} {
    set x 1
    set y 2
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.tcl"), &config);
        assert!(result.is_ok());
        let (ir, _extra) = result.unwrap();
        assert!(!ir.calls.is_empty(), "Expected calls to be extracted");

        // Tcl records keyword commands (set, global, expr, etc.) as call relationships
        assert!(
            ir.calls.iter().any(|c| c.callee == "set"),
            "Expected 'set' to appear as a call, got: {:?}",
            ir.calls
                .iter()
                .map(|c| format!("{}->{}", c.caller, c.callee))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_extract_module_metadata_fields() {
        // The name/language pair is pinned by test_extract_module_info; this pins the
        // remaining ModuleEntity fields extract() assembles directly: path, line_count,
        // doc_comment and attributes.
        let source = "proc foo {} {}\nproc bar {} {}\n";
        let config = ParserConfig::default();
        let (ir, _extra) = extract(source, Path::new("dir/mod.tcl"), &config).unwrap();

        let module = ir.module.expect("module should be present");
        assert_eq!(module.path, Path::new("dir/mod.tcl").display().to_string());
        assert_eq!(module.line_count, source.lines().count());
        assert_eq!(module.line_count, 2);
        assert!(module.doc_comment.is_none());
        assert!(module.attributes.is_empty());
    }

    #[test]
    fn test_extract_unknown_module_fallback() {
        // An empty path has no file_stem, so the module name resolves to the "unknown"
        // literal fallback — a branch every other test skips by passing a real filename.
        let config = ParserConfig::default();
        let (ir, _extra) = extract("proc foo {} {}", Path::new(""), &config).unwrap();

        let module = ir.module.expect("module should be present");
        assert_eq!(module.name, "unknown");
        assert_eq!(module.language, "tcl");
    }

    #[test]
    fn test_extract_empty_source() {
        // Empty source parses to a valid empty tree (no ParseError); line_count is 0 and
        // no entities of any kind are produced.
        let config = ParserConfig::default();
        let (ir, extra) = extract("", Path::new("empty.tcl"), &config).unwrap();

        let module = ir.module.expect("module should be present");
        assert_eq!(module.name, "empty");
        assert_eq!(module.line_count, 0);
        assert!(ir.functions.is_empty());
        assert!(ir.classes.is_empty());
        assert!(ir.imports.is_empty());
        assert!(ir.calls.is_empty());
        assert!(extra.sdc.clocks.is_empty());
        assert!(extra.eda.design_reads.is_empty());
    }

    #[test]
    fn test_extract_line_count_tracks_source_lines() {
        // line_count is derived from source.lines().count(), independent of how many
        // entities are extracted — a blank-line-heavy source with a single proc.
        let source = "\n\n\nproc solo {} {}\n\n";
        let config = ParserConfig::default();
        let (ir, _extra) = extract(source, Path::new("blanks.tcl"), &config).unwrap();

        let module = ir.module.expect("module should be present");
        assert_eq!(module.line_count, source.lines().count());
        assert_eq!(module.line_count, 5);
        assert_eq!(ir.functions.len(), 1);
    }

    #[test]
    fn test_extract_no_imports_for_plain_proc() {
        // ir.imports is populated from visitor.imports; a plain proc with no package
        // require or EDA design-read command leaves it empty — the mirror of the
        // non-empty import assertion in test_extract_eda_flow.
        let source = "proc foo {} {\n    set x 1\n}";
        let config = ParserConfig::default();
        let (ir, _extra) = extract(source, Path::new("plain.tcl"), &config).unwrap();

        assert!(ir.imports.is_empty());
        assert_eq!(ir.functions.len(), 1);
    }
}
