// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! AST extraction for C source code
//!
//! This module provides two parsing modes:
//! - Strict mode: Fails on syntax errors (default, for clean code)
//! - Tolerant mode: Extracts what it can even with errors (for real-world code)

use codegraph_parser_api::{CallRelation, CodeIR, ModuleEntity, ParserConfig, ParserError};
use std::path::Path;
use tree_sitter::Parser;

use crate::preprocessor::CPreprocessor;
use crate::visitor::CVisitor;

/// Extraction options for controlling parser behavior
#[derive(Debug, Clone, Default)]
pub struct ExtractionOptions {
    /// If true, extract entities even when the AST has errors
    pub tolerant_mode: bool,
    /// If true, apply preprocessing to help parse kernel/system code
    pub preprocess: bool,
    /// If true, extract function calls for call graph
    pub extract_calls: bool,
    /// Additional type definitions from resolved headers (name, expansion).
    /// These are injected into the preprocessor before parsing.
    pub header_types: Vec<(String, String)>,
}

impl ExtractionOptions {
    /// Create options optimized for kernel/system code
    pub fn for_kernel_code() -> Self {
        Self {
            tolerant_mode: true,
            preprocess: true,
            extract_calls: true,
            header_types: Vec::new(),
        }
    }

    /// Create options for tolerant parsing of any code
    pub fn tolerant() -> Self {
        Self {
            tolerant_mode: true,
            preprocess: false,
            extract_calls: true,
            header_types: Vec::new(),
        }
    }
}

/// Result of extraction with additional metadata
#[derive(Debug)]
pub struct ExtractionResult {
    pub ir: CodeIR,
    /// Functions registered via module_init/module_exit (entry points)
    pub entry_points: Vec<String>,
    /// Functions registered via EXPORT_SYMBOL (public API)
    pub exported_symbols: Vec<String>,
    /// Number of syntax errors encountered (0 = clean parse)
    pub error_count: usize,
    /// Whether the file was fully parsed or partially
    pub is_partial: bool,
    /// Macros detected in the source
    pub detected_macros: Vec<String>,
}

/// Extract code entities and relationships from C source code (strict mode)
pub fn extract(
    source: &str,
    file_path: &Path,
    _config: &ParserConfig,
) -> Result<CodeIR, ParserError> {
    // Detect VMK/kernel code upfront — these files often parse without
    // ERROR nodes but produce wrong IR because tree-sitter doesn't know
    // the custom typedefs. Preprocess them immediately instead of waiting
    // for the tolerant fallback (which only triggers on SyntaxError).
    let needs_preprocess = source_needs_type_preamble(source);

    let options = ExtractionOptions {
        extract_calls: true,
        preprocess: needs_preprocess,
        ..Default::default()
    };

    let result = extract_with_options(source, file_path, &options)?;

    if result.is_partial {
        return Err(ParserError::SyntaxError(
            file_path.to_path_buf(),
            0,
            0,
            "Syntax error".to_string(),
        ));
    }

    Ok(result.ir)
}

/// Quick scan for type names that tree-sitter won't recognise as type
/// specifiers without preprocessing (stdint, kernel, VMware types).
pub fn source_needs_type_preamble(source: &str) -> bool {
    // Check the first ~4KB for common patterns (fast path for normal C files)
    let sample = if source.len() > 4096 {
        &source[..4096]
    } else {
        source
    };

    // C99 stdint.h types — extremely common, tree-sitter doesn't know them
    sample.contains("uint8_t")
        || sample.contains("uint16_t")
        || sample.contains("uint32_t")
        || sample.contains("uint64_t")
        || sample.contains("int8_t")
        || sample.contains("int16_t")
        || sample.contains("int32_t")
        || sample.contains("int64_t")
        // Linux kernel types
        || sample.contains("vmk_")
        || sample.contains("VMK_")
        || sample.contains("vmk_Bool")
        || sample.contains("vmk_uint")
        || sample.contains("vmk_Uplink")
        || sample.contains("vmk_Device")
}

/// Extract with custom options (supports tolerant mode)
pub fn extract_with_options(
    source: &str,
    file_path: &Path,
    options: &ExtractionOptions,
) -> Result<ExtractionResult, ParserError> {
    // Detect macros from original source (before preprocessing)
    let mut preprocessor = CPreprocessor::new();

    // Add header-resolved types before preprocessing
    for (name, expansion) in &options.header_types {
        preprocessor.add_type(name, expansion);
    }

    let detected_macros: Vec<String> = preprocessor
        .analyze_macros(source)
        .iter()
        .map(|m| m.name.clone())
        .collect();

    // Optionally preprocess the source
    let processed_source = if options.preprocess {
        preprocessor.preprocess(source)
    } else {
        source.to_string()
    };

    let mut parser = Parser::new();
    let language = tree_sitter_c::language();
    parser
        .set_language(&language)
        .map_err(|e| ParserError::ParseError(file_path.to_path_buf(), e.to_string()))?;

    let tree = parser.parse(&processed_source, None).ok_or_else(|| {
        ParserError::ParseError(file_path.to_path_buf(), "Failed to parse".to_string())
    })?;

    let root_node = tree.root_node();
    let has_error = root_node.has_error();
    let error_count = if has_error {
        count_errors(root_node)
    } else {
        0
    };

    // In strict mode, fail on errors
    if has_error && !options.tolerant_mode {
        return Err(ParserError::SyntaxError(
            file_path.to_path_buf(),
            0,
            0,
            format!("Syntax error ({error_count} error nodes)"),
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
        language: "c".to_string(),
        line_count: source.lines().count(),
        doc_comment: None,
        attributes: Vec::new(),
    });

    // Visit the AST - the visitor will skip ERROR nodes gracefully
    let mut visitor = CVisitor::new(processed_source.as_bytes());
    visitor.set_extract_calls(options.extract_calls);
    visitor.visit_node(root_node);

    ir.functions = visitor.functions;
    ir.classes = visitor.structs;
    ir.imports = visitor.imports;

    ir.calls = visitor
        .calls
        .into_iter()
        .filter_map(|call| {
            call.caller.map(|caller| {
                let mut rel = CallRelation::new(caller, call.callee, call.line);
                if let (Some(st), Some(fn_name)) = (call.struct_type, call.field_name) {
                    rel = rel.with_vtable(st, fn_name);
                }
                rel
            })
        })
        .collect();

    let entry_points = visitor.entry_points;
    let exported_symbols = visitor.exported_symbols;

    Ok(ExtractionResult {
        ir,
        entry_points,
        exported_symbols,
        error_count,
        is_partial: has_error,
        detected_macros,
    })
}

/// Count ERROR nodes in the syntax tree
fn count_errors(node: tree_sitter::Node) -> usize {
    let mut count = 0;

    if node.is_error() || node.is_missing() {
        count += 1;
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        count += count_errors(child);
    }

    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_simple_function() {
        let source = r#"
int main() {
    return 0;
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.c"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].name, "main");
    }

    #[test]
    fn test_extract_function_with_params() {
        let source = r#"
int add(int a, int b) {
    return a + b;
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.c"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].name, "add");
        assert_eq!(ir.functions[0].parameters.len(), 2);
    }

    #[test]
    fn test_extract_struct() {
        let source = r#"
struct Point {
    int x;
    int y;
};
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.c"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "Point");
    }

    #[test]
    fn test_extract_enum() {
        let source = r#"
enum Color {
    RED,
    GREEN,
    BLUE
};
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.c"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "Color");
    }

    #[test]
    fn test_extract_include() {
        let source = r#"
#include <stdio.h>
#include "myheader.h"
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.c"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.imports.len(), 2);
    }

    #[test]
    fn test_extract_multiple_functions() {
        let source = r#"
int foo() { return 1; }
int bar() { return 2; }
int baz() { return 3; }
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.c"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.functions.len(), 3);
    }

    #[test]
    fn test_extract_static_function() {
        let source = r#"
static void helper() {
    // internal function
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.c"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].visibility, "private");
    }

    #[test]
    fn test_extract_module_info() {
        let source = r#"
int test() {
    return 42;
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("module.c"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert!(ir.module.is_some());
        let module = ir.module.unwrap();
        assert_eq!(module.name, "module");
        assert_eq!(module.language, "c");
        assert!(module.line_count > 0);
    }

    #[test]
    fn test_extract_with_syntax_error_strict() {
        let source = r#"
int broken( {
    // Missing closing brace
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.c"), &config);

        assert!(result.is_err());
        match result {
            Err(ParserError::SyntaxError(..)) => (),
            _ => panic!("Expected SyntaxError"),
        }
    }

    #[test]
    fn test_extract_with_syntax_error_tolerant() {
        let source = r#"
int valid_func() { return 0; }
int broken( {
int another_valid() { return 1; }
"#;
        let options = ExtractionOptions::tolerant();
        let result = extract_with_options(source, Path::new("test.c"), &options);

        assert!(result.is_ok());
        let extraction = result.unwrap();
        assert!(extraction.is_partial);
        assert!(extraction.error_count > 0);
        // Should still extract the valid functions
        assert!(!extraction.ir.functions.is_empty());
    }

    #[test]
    fn test_extract_kernel_code_simulation() {
        let source = r#"
static __init int my_module_init(void) {
    return 0;
}

static __exit void my_module_exit(void) {
}

MODULE_LICENSE("GPL");
"#;
        let options = ExtractionOptions::for_kernel_code();
        let result = extract_with_options(source, Path::new("test.c"), &options);

        // With preprocessing, this should parse better
        assert!(result.is_ok());
        let extraction = result.unwrap();
        // Check that macros were detected
        assert!(
            extraction.detected_macros.contains(&"__init".to_string())
                || extraction.detected_macros.contains(&"__exit".to_string())
        );
    }

    #[test]
    fn test_extract_pointer_params() {
        let source = r#"
void process(int *arr, const char *str) {
    // pointer parameters
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.c"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.functions[0].parameters.len(), 2);
    }

    #[test]
    fn test_extract_union() {
        let source = r#"
union Data {
    int i;
    float f;
    char c;
};
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.c"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.classes[0].name, "Data");
    }

    #[test]
    fn test_extract_function_with_complexity() {
        let source = r#"
int complex_func(int x) {
    if (x > 0) {
        for (int i = 0; i < x; i++) {
            if (i % 2 == 0) {
                continue;
            }
        }
        return 1;
    } else if (x < 0) {
        while (x < 0) {
            x++;
        }
        return -1;
    }
    return 0;
}
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.c"), &config);

        assert!(result.is_ok());
        let ir = result.unwrap();
        assert_eq!(ir.functions.len(), 1);
        // Check that complexity metrics are populated
        let func = &ir.functions[0];
        assert!(func.complexity.is_some());
        let complexity = func.complexity.as_ref().unwrap();
        assert!(complexity.cyclomatic_complexity > 1);
    }

    #[test]
    fn test_extract_vmk_function_strict_path() {
        // Test that strict extract() works with VMK-style code
        // Tree-sitter C grammar treats unknown identifiers as type_identifiers
        let source = "VMK_ReturnStatus\nirndrv_RDMAOpGetPrivStats(vmk_AddrCookie driverData, char *statBuf,\n                          vmk_ByteCount length)\n{\n   irndrv_Pf *pf = (irndrv_Pf *)driverData.ptr;\n   vmk_ByteCount outLen;\n   VMK_ReturnStatus status;\n\n   if (length < 100) {\n      return VMK_BAD_PARAM;\n   }\n\n   for (int i = 0; i < 10; i++) {\n      vmk_Memset(statBuf, 0, length);\n   }\n\n   return VMK_OK;\n}\n";

        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.c"), &config);

        match &result {
            Ok(ir) => {
                println!("STRICT OK: {} functions", ir.functions.len());
                for f in &ir.functions {
                    let cx = f.complexity.as_ref();
                    println!(
                        "  {}: complexity={}, branches={}, loops={}",
                        f.name,
                        cx.map_or(0, |c| c.cyclomatic_complexity),
                        cx.map_or(0, |c| c.branches),
                        cx.map_or(0, |c| c.loops),
                    );
                }
                assert!(!ir.functions.is_empty());
                let cx = ir.functions[0]
                    .complexity
                    .as_ref()
                    .expect("should have complexity");
                assert!(
                    cx.cyclomatic_complexity > 1,
                    "Expected strict path complexity > 1, got {}",
                    cx.cyclomatic_complexity
                );
            }
            Err(e) => {
                println!("STRICT FAILED: {:?}", e);
                // If strict fails, try tolerant
                let options = ExtractionOptions::for_kernel_code();
                let result2 = extract_with_options(source, Path::new("test.c"), &options).unwrap();
                println!(
                    "TOLERANT: {} functions, is_partial={}",
                    result2.ir.functions.len(),
                    result2.is_partial
                );
                for f in &result2.ir.functions {
                    let cx = f.complexity.as_ref();
                    println!(
                        "  {}: complexity={}",
                        f.name,
                        cx.map_or(0, |c| c.cyclomatic_complexity)
                    );
                }
                panic!("Expected strict to succeed for clean VMK code");
            }
        }
    }

    #[test]
    fn test_extract_vtable_function_pointers() {
        let source = r#"
static int my_get_stats(void *data) { return 42; }
static void my_attach(void *device) {}

static DeviceOps ops = {
    .getStats = my_get_stats,
    .attach = my_attach,
    .timeout = 100,
    .name = NULL,
};
"#;
        let config = ParserConfig::default();
        let result = extract(source, Path::new("test.c"), &config);
        assert!(result.is_ok());
        let ir = result.unwrap();

        let callees: Vec<&str> = ir.calls.iter().map(|c| c.callee.as_str()).collect();
        eprintln!("vtable calls: {:?}", callees);

        assert!(
            callees.contains(&"my_get_stats"),
            "Should detect .getStats = my_get_stats. Got: {:?}",
            callees
        );
        assert!(
            callees.contains(&"my_attach"),
            "Should detect .attach = my_attach. Got: {:?}",
            callees
        );
        // Should NOT include literals or NULL
        assert!(
            !callees.contains(&"NULL"),
            "Should not include NULL as a callee"
        );
        assert!(
            !callees.contains(&"100"),
            "Should not include numeric literals"
        );

        // Verify vtable assignments have a caller context
        let vtable_calls: Vec<_> = ir
            .calls
            .iter()
            .filter(|c| c.callee == "my_get_stats" || c.callee == "my_attach")
            .collect();
        assert!(!vtable_calls.is_empty(), "Should have vtable call entries");
    }
}

#[test]
fn test_split_return_type_function_name() {
    // Function with return type on separate line from name
    let source = r#"
static VMK_ReturnStatus
irndrv_Attach(vmk_Device device)
{
    return 0;
}

int
another_split(void)
{
    return 42;
}

int normal_func(void) {
    return 0;
}
"#;
    let config = ParserConfig::default();
    let result = extract(source, Path::new("test.c"), &config);
    assert!(result.is_ok(), "Failed to parse: {:?}", result.err());
    let ir = result.unwrap();

    let names: Vec<&str> = ir.functions.iter().map(|f| f.name.as_str()).collect();
    eprintln!("Functions found: {:?}", names);
    for f in &ir.functions {
        eprintln!(
            "  name='{}' sig='{}' ret={:?} line={}",
            f.name, f.signature, f.return_type, f.line_start
        );
    }

    assert!(
        names.contains(&"irndrv_Attach"),
        "Missing irndrv_Attach in {:?}",
        names
    );
    assert!(
        names.contains(&"another_split"),
        "Missing another_split in {:?}",
        names
    );
    assert!(
        names.contains(&"normal_func"),
        "Missing normal_func in {:?}",
        names
    );
}

#[test]
fn test_irndrv_verbs_calls() {
    let path =
        "/home/jason/projects/docs/drivers.ethernet.rdma.esxn/src/COMMON_RDMA/irndrv_verbs.c";
    let Ok(source) = std::fs::read_to_string(path) else {
        println!("Skipping: file not found");
        return;
    };
    let opts = ExtractionOptions::for_kernel_code();
    let result = extract_with_options(&source, Path::new(path), &opts).unwrap();
    println!(
        "functions={} total_calls={}",
        result.ir.functions.len(),
        result.ir.calls.len()
    );
    let priv_calls: Vec<_> = result
        .ir
        .calls
        .iter()
        .filter(|c| c.caller == "irndrv_RDMAOpGetPrivStats")
        .collect();
    println!("irndrv_RDMAOpGetPrivStats calls: {}", priv_calls.len());
    for c in &priv_calls {
        println!("  -> {} at line {}", c.callee, c.call_site_line);
    }
    println!("First 5 calls overall:");
    for c in result.ir.calls.iter().take(5) {
        println!(
            "  {} -> {} at line {}",
            c.caller, c.callee, c.call_site_line
        );
    }
    // Should have at least some calls
    assert!(
        !result.ir.calls.is_empty(),
        "No calls extracted from irndrv_verbs.c!"
    );
}

/// Diagnose where cross-file call edges are lost in the pipeline.
///
/// This test verifies:
/// 1. The extractor captures external callees in ir.calls
/// 2. The mapper stores unresolved_calls on the node
/// 3. After simulating resolve_cross_file_imports, Calls edges appear
#[test]
fn test_mapper_unresolved_calls() {
    use codegraph::{CodeGraph, EdgeType, NodeType};

    let path =
        "/home/jason/projects/docs/drivers.ethernet.rdma.esxn/src/COMMON_RDMA/irndrv_verbs.c";
    let Ok(source) = std::fs::read_to_string(path) else {
        println!("Skipping: file not found");
        return;
    };

    // Step 1: extract IR
    let opts = ExtractionOptions::for_kernel_code();
    let result = extract_with_options(&source, Path::new(path), &opts).unwrap();

    let priv_calls: Vec<_> = result
        .ir
        .calls
        .iter()
        .filter(|c| c.caller == "irndrv_RDMAOpGetPrivStats")
        .collect();
    println!(
        "IR: irndrv_RDMAOpGetPrivStats has {} calls",
        priv_calls.len()
    );
    for c in priv_calls.iter().take(5) {
        println!("  -> {} at line {}", c.callee, c.call_site_line);
    }

    // Check if irdma_report_pfc_stats appears in functions (it shouldn't - it's defined in utils.c)
    let report_in_functions = result
        .ir
        .functions
        .iter()
        .any(|f| f.name == "irdma_report_pfc_stats");
    println!(
        "irdma_report_pfc_stats in ir.functions: {}",
        report_in_functions
    );
    println!("Total functions in IR: {}", result.ir.functions.len());

    // Step 2: map to graph
    let mut graph = CodeGraph::in_memory().unwrap();
    let file_info = crate::mapper::ir_to_graph(&result.ir, &mut graph, Path::new(path)).unwrap();
    println!("Mapped: {} functions indexed", file_info.functions.len());

    // Step 3: check unresolved_calls on irndrv_RDMAOpGetPrivStats
    let mut privstats_id = None;
    for func_id in graph
        .query()
        .node_type(NodeType::Function)
        .execute()
        .unwrap_or_default()
    {
        if let Ok(node) = graph.get_node(func_id) {
            if node.properties.get_string("name") == Some("irndrv_RDMAOpGetPrivStats") {
                privstats_id = Some(func_id);
                let unresolved = node.properties.get_string("unresolved_calls").unwrap_or("");
                println!(
                    "Node unresolved_calls (first 300 chars): '{}'",
                    &unresolved[..unresolved.len().min(300)]
                );
                // Also check string_list variant
                if let Some(list) = node.properties.get_string_list_compat("unresolved_calls") {
                    println!("Node unresolved_calls as list: {} items", list.len());
                    for item in list.iter().take(5) {
                        println!("  - {}", item);
                    }
                }
                break;
            }
        }
    }
    assert!(
        privstats_id.is_some(),
        "irndrv_RDMAOpGetPrivStats not found in graph after mapping"
    );

    // Step 4: now simulate resolve_cross_file_imports manually
    // Add a stub node for irdma_report_pfc_stats (simulating it being indexed from utils.c)
    let stub_id = graph
        .add_node(codegraph::NodeType::Function, {
            codegraph::PropertyMap::new()
                .with("name", "irdma_report_pfc_stats")
                .with("path", "/fake/irndrv_utils.c")
        })
        .unwrap();
    println!("Added stub node for irdma_report_pfc_stats: {:?}", stub_id);

    // Now run the resolution logic
    let mut symbol_map = std::collections::HashMap::new();
    for func_id in graph
        .query()
        .node_type(NodeType::Function)
        .execute()
        .unwrap_or_default()
    {
        if let Ok(node) = graph.get_node(func_id) {
            if let Some(name) = node.properties.get_string("name") {
                symbol_map.insert(name.to_string(), func_id);
            }
        }
    }
    println!("symbol_map size: {}", symbol_map.len());
    println!(
        "irdma_report_pfc_stats in symbol_map: {}",
        symbol_map.contains_key("irdma_report_pfc_stats")
    );

    // Resolve unresolved calls
    let mut edges_added = 0;
    let func_ids: Vec<_> = graph
        .query()
        .node_type(NodeType::Function)
        .execute()
        .unwrap_or_default();
    let mut calls_to_add = Vec::new();
    for func_id in &func_ids {
        if let Ok(node) = graph.get_node(*func_id) {
            if let Some(unresolved) = node.properties.get_string_list_compat("unresolved_calls") {
                for callee_name in &unresolved {
                    if let Some(&callee_id) = symbol_map.get(callee_name.as_str()) {
                        calls_to_add.push((*func_id, callee_id));
                    }
                }
            }
        }
    }
    for (from, to) in calls_to_add {
        let _ = graph.add_edge(from, to, EdgeType::Calls, codegraph::PropertyMap::new());
        edges_added += 1;
    }
    println!("Calls edges added by resolution: {}", edges_added);

    // Step 5: check the edge exists
    let ps_id = privstats_id.unwrap();
    if let Ok(edges) = graph.get_edges_between(ps_id, stub_id) {
        let calls: Vec<_> = edges
            .iter()
            .filter(|&&e| {
                graph
                    .get_edge(e)
                    .map(|edge| edge.edge_type == EdgeType::Calls)
                    .unwrap_or(false)
            })
            .collect();
        println!(
            "Calls edges irndrv_RDMAOpGetPrivStats -> irdma_report_pfc_stats: {}",
            calls.len()
        );
        assert!(
            !calls.is_empty(),
            "Expected Calls edge after resolution, but got 0. \
             This means the mapper is NOT storing irdma_report_pfc_stats in unresolved_calls."
        );
    } else {
        panic!("get_edges_between failed");
    }
}
