// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Mapper for converting CodeIR + TclExtraData to CodeGraph nodes and edges

use codegraph::{CodeGraph, EdgeType, NodeId, NodeType, PropertyMap};
use codegraph_parser_api::{CodeIR, FileInfo, ParserError};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use crate::extractor::TclExtraData;

pub fn ir_to_graph(
    ir: &CodeIR,
    extra: &TclExtraData,
    graph: &mut CodeGraph,
    file_path: &Path,
) -> Result<FileInfo, ParserError> {
    let mut node_map: HashMap<String, NodeId> = HashMap::new();
    let mut function_ids = Vec::new();
    let mut class_ids = Vec::new();
    let trait_ids = Vec::new();
    let mut import_ids = Vec::new();

    // Create module/file node with SDC/EDA properties
    let file_id = if let Some(ref module) = ir.module {
        let mut props = PropertyMap::new()
            .with("name", module.name.clone())
            .with("path", module.path.clone())
            .with("language", module.language.clone())
            .with("line_count", module.line_count as i64);

        if let Some(ref doc) = module.doc_comment {
            props = props.with("doc", doc.clone());
        }

        // Attach SDC properties
        if !extra.sdc.clocks.is_empty() {
            props = props.with(
                "sdc_clocks",
                serde_json::to_string(&extra.sdc.clocks).unwrap_or_default(),
            );
        }
        if !extra.sdc.io_delays.is_empty() {
            props = props.with(
                "sdc_io_delays",
                serde_json::to_string(&extra.sdc.io_delays).unwrap_or_default(),
            );
        }
        if !extra.sdc.timing_exceptions.is_empty() {
            props = props.with(
                "sdc_timing_exceptions",
                serde_json::to_string(&extra.sdc.timing_exceptions).unwrap_or_default(),
            );
        }

        // Attach EDA data
        if !extra.eda.design_reads.is_empty() {
            props = props.with(
                "eda_design_reads",
                serde_json::to_string(&extra.eda.design_reads).unwrap_or_default(),
            );
        }
        if !extra.eda.design_writes.is_empty() {
            props = props.with(
                "eda_design_writes",
                serde_json::to_string(&extra.eda.design_writes).unwrap_or_default(),
            );
        }
        if !extra.eda.registered_commands.is_empty() {
            props = props.with(
                "eda_registered_commands",
                serde_json::to_string(&extra.eda.registered_commands).unwrap_or_default(),
            );
        }

        let id = graph
            .add_node(NodeType::CodeFile, props)
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
        node_map.insert(module.name.clone(), id);
        id
    } else {
        let name = file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();
        let props = PropertyMap::new()
            .with("name", name.clone())
            .with("path", file_path.display().to_string())
            .with("language", "tcl");

        let id = graph
            .add_node(NodeType::CodeFile, props)
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
        node_map.insert(name, id);
        id
    };

    // Add functions
    for func in &ir.functions {
        let mut props = PropertyMap::new()
            .with("name", func.name.clone())
            .with("path", file_path.display().to_string())
            .with("signature", func.signature.clone())
            .with("visibility", func.visibility.clone())
            .with("line_start", func.line_start as i64)
            .with("line_end", func.line_end as i64)
            .with("is_async", func.is_async)
            .with("is_static", func.is_static)
            .with("is_abstract", func.is_abstract)
            .with("is_test", func.is_test);

        if let Some(ref doc) = func.doc_comment {
            props = props.with("doc", doc.clone());
        }
        if let Some(ref parent) = func.parent_class {
            props = props.with("parent_class", parent.clone());
        }
        if !func.parameters.is_empty() {
            let param_names: Vec<String> = func.parameters.iter().map(|p| p.name.clone()).collect();
            props = props.with("parameters", param_names);
        }
        if !func.attributes.is_empty() {
            props = props.with("attributes", func.attributes.clone());
        }
        if let Some(ref body) = func.body_prefix {
            props = props.with("body_prefix", body.clone());
        }
        if let Some(ref complexity) = func.complexity {
            props = props
                .with("complexity", complexity.cyclomatic_complexity as i64)
                .with("complexity_grade", complexity.grade().to_string())
                .with("complexity_branches", complexity.branches as i64)
                .with("complexity_loops", complexity.loops as i64)
                .with(
                    "complexity_logical_ops",
                    complexity.logical_operators as i64,
                )
                .with("complexity_nesting", complexity.max_nesting_depth as i64)
                .with(
                    "complexity_exceptions",
                    complexity.exception_handlers as i64,
                )
                .with("complexity_early_returns", complexity.early_returns as i64);
        }

        let func_id = graph
            .add_node(NodeType::Function, props)
            .map_err(|e| ParserError::GraphError(e.to_string()))?;

        node_map.insert(func.name.clone(), func_id);
        function_ids.push(func_id);

        // Link function to parent namespace/class or file
        if let Some(ref parent_class) = func.parent_class {
            if let Some(&class_id) = node_map.get(parent_class) {
                graph
                    .add_edge(class_id, func_id, EdgeType::Contains, PropertyMap::new())
                    .map_err(|e| ParserError::GraphError(e.to_string()))?;
            }
        } else {
            graph
                .add_edge(file_id, func_id, EdgeType::Contains, PropertyMap::new())
                .map_err(|e| ParserError::GraphError(e.to_string()))?;
        }
    }

    // Add classes (namespaces in Tcl)
    for class in &ir.classes {
        let mut props = PropertyMap::new()
            .with("name", class.name.clone())
            .with("path", file_path.display().to_string())
            .with("visibility", class.visibility.clone())
            .with("line_start", class.line_start as i64)
            .with("line_end", class.line_end as i64)
            .with("is_abstract", class.is_abstract);

        if let Some(ref doc) = class.doc_comment {
            props = props.with("doc", doc.clone());
        }
        if !class.attributes.is_empty() {
            props = props.with("attributes", class.attributes.clone());
        }
        if let Some(ref body) = class.body_prefix {
            props = props.with("body_prefix", body.clone());
        }

        let class_id = graph
            .add_node(NodeType::Class, props)
            .map_err(|e| ParserError::GraphError(e.to_string()))?;

        node_map.insert(class.name.clone(), class_id);
        class_ids.push(class_id);

        // Link namespace to file
        graph
            .add_edge(file_id, class_id, EdgeType::Contains, PropertyMap::new())
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
    }

    // Add import relationships
    for import in &ir.imports {
        let imported_module = &import.imported;

        let import_id = if let Some(&existing_id) = node_map.get(imported_module) {
            existing_id
        } else {
            let props = PropertyMap::new()
                .with("name", imported_module.clone())
                .with("is_external", "true");

            let id = graph
                .add_node(NodeType::Module, props)
                .map_err(|e| ParserError::GraphError(e.to_string()))?;
            node_map.insert(imported_module.clone(), id);
            id
        };

        import_ids.push(import_id);

        let mut edge_props = PropertyMap::new();
        if import.is_wildcard {
            edge_props = edge_props.with("is_wildcard", "true");
        }
        if !import.symbols.is_empty() {
            edge_props = edge_props.with("symbols", import.symbols.clone());
        }

        graph
            .add_edge(file_id, import_id, EdgeType::Imports, edge_props)
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
    }

    // Add call relationships
    let mut unresolved_calls: HashMap<String, Vec<String>> = HashMap::new();

    for call in &ir.calls {
        if let Some(&caller_id) = node_map.get(&call.caller) {
            if let Some(&callee_id) = node_map.get(&call.callee) {
                let edge_props = PropertyMap::new()
                    .with("call_site_line", call.call_site_line as i64)
                    .with("is_direct", call.is_direct);

                graph
                    .add_edge(caller_id, callee_id, EdgeType::Calls, edge_props)
                    .map_err(|e| ParserError::GraphError(e.to_string()))?;
            } else {
                unresolved_calls
                    .entry(call.caller.clone())
                    .or_default()
                    .push(call.callee.clone());
            }
        }
    }

    // Store unresolved calls on caller nodes
    for (caller_name, callees) in unresolved_calls {
        if let Some(&caller_id) = node_map.get(&caller_name) {
            if let Ok(node) = graph.get_node(caller_id) {
                let mut all_callees: Vec<String> = node
                    .properties
                    .get_string_list_compat("unresolved_calls")
                    .unwrap_or_default();
                for callee in &callees {
                    if !all_callees.iter().any(|c| c == callee) {
                        all_callees.push(callee.clone());
                    }
                }
                let new_props = node
                    .properties
                    .clone()
                    .with("unresolved_calls", all_callees);
                let _ = graph.update_node_properties(caller_id, new_props);
            }
        }
    }

    let line_count = if let Some(ref module) = ir.module {
        module.line_count
    } else {
        0
    };

    Ok(FileInfo {
        file_path: file_path.to_path_buf(),
        file_id,
        functions: function_ids,
        classes: class_ids,
        traits: trait_ids,
        imports: import_ids,
        parse_time: Duration::ZERO,
        line_count,
        byte_count: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eda::EdaData;
    use crate::sdc::{SdcClock, SdcData};
    use codegraph::PropertyValue;
    use codegraph_parser_api::{
        CallRelation, ClassEntity, ComplexityMetrics, FunctionEntity, ImportRelation, ModuleEntity,
        Parameter, TraitEntity,
    };
    use std::path::PathBuf;

    fn default_extra() -> TclExtraData {
        TclExtraData {
            sdc: SdcData::default(),
            eda: EdaData::default(),
        }
    }

    fn map_with(ir: &CodeIR, extra: &TclExtraData) -> (CodeGraph, FileInfo) {
        let mut graph = CodeGraph::in_memory().unwrap();
        let info = ir_to_graph(ir, extra, &mut graph, Path::new("test.tcl")).unwrap();
        (graph, info)
    }

    fn map(ir: &CodeIR) -> (CodeGraph, FileInfo) {
        map_with(ir, &default_extra())
    }

    /// Return the single edge between two nodes (fails if not exactly one).
    fn edge_between(graph: &CodeGraph, src: NodeId, dst: NodeId) -> &codegraph::Edge {
        let ids = graph.get_edges_between(src, dst).unwrap();
        assert_eq!(ids.len(), 1, "expected exactly one edge {src}->{dst}");
        graph.get_edge(ids[0]).unwrap()
    }

    #[test]
    fn test_property_types() {
        let mut ir = CodeIR::new(PathBuf::from("test.tcl"));
        ir.set_module(ModuleEntity::new("test", "test.tcl", "tcl").with_line_count(100));
        let func = FunctionEntity::new("test_fn", 10, 20)
            .with_signature("proc test_fn {}")
            .with_visibility("public")
            .async_fn();
        ir.add_function(func);

        let (graph, file_info) = map(&ir);

        // Verify file node line_count is Int
        let file_node = graph.get_node(file_info.file_id).unwrap();
        assert!(matches!(
            file_node.properties.get("line_count"),
            Some(PropertyValue::Int(100))
        ));

        // Verify function properties are correct types
        let func_node = graph.get_node(file_info.functions[0]).unwrap();
        assert!(matches!(
            func_node.properties.get("line_start"),
            Some(PropertyValue::Int(10))
        ));
        assert!(matches!(
            func_node.properties.get("line_end"),
            Some(PropertyValue::Int(20))
        ));
        assert!(matches!(
            func_node.properties.get("is_async"),
            Some(PropertyValue::Bool(true))
        ));
    }

    #[test]
    fn empty_ir_builds_file_node_from_path_stem() {
        // No module set: name is derived from the file stem, language is
        // hard-coded to "tcl", and the graph holds only the file node.
        let ir = CodeIR::new(PathBuf::from("test.tcl"));
        let (graph, info) = map(&ir);

        assert_eq!(graph.node_count(), 1);
        assert_eq!(graph.edge_count(), 0);
        assert_eq!(info.line_count, 0);
        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(file.properties.get_string("name"), Some("test"));
        assert_eq!(file.properties.get_string("language"), Some("tcl"));
        assert!(matches!(file.node_type, NodeType::CodeFile));
    }

    #[test]
    fn free_function_gets_file_contains_edge() {
        let mut ir = CodeIR::new(PathBuf::from("test.tcl"));
        ir.add_function(FunctionEntity::new("free", 1, 2));
        let (graph, info) = map(&ir);

        let func_id = info.functions[0];
        let edge = edge_between(&graph, info.file_id, func_id);
        assert!(matches!(edge.edge_type, EdgeType::Contains));
    }

    #[test]
    fn class_emits_node_and_contains_edge_but_drops_methods() {
        // The tcl mapper (unlike swift/cpp) never iterates class.methods: a
        // namespace becomes a bare Class node wired to the file, and any
        // methods it carries are silently dropped (no Function nodes).
        let mut ir = CodeIR::new(PathBuf::from("test.tcl"));
        let class = ClassEntity::new("Widget", 1, 30)
            .with_visibility("public")
            .with_methods(vec![FunctionEntity::new("render", 5, 9)]);
        ir.add_class(class);
        let (graph, info) = map(&ir);

        // file + class only - the method is dropped
        assert_eq!(graph.node_count(), 2);
        assert_eq!(info.classes.len(), 1);
        assert!(info.functions.is_empty());

        let class_id = info.classes[0];
        let class_node = graph.get_node(class_id).unwrap();
        assert!(matches!(class_node.node_type, NodeType::Class));
        assert_eq!(class_node.properties.get_string("name"), Some("Widget"));
        assert!(matches!(
            edge_between(&graph, info.file_id, class_id).edge_type,
            EdgeType::Contains
        ));
    }

    #[test]
    fn trait_is_ignored() {
        // tcl has no trait handling: trait_ids is always empty and no
        // Interface node is emitted.
        let mut ir = CodeIR::new(PathBuf::from("test.tcl"));
        ir.add_trait(TraitEntity::new("Drawable", 1, 5));
        let (graph, info) = map(&ir);

        assert_eq!(graph.node_count(), 1);
        assert!(info.traits.is_empty());
    }

    #[test]
    fn import_creates_external_module_with_symbols_edge() {
        let mut ir = CodeIR::new(PathBuf::from("test.tcl"));
        ir.add_import(
            ImportRelation::new("test", "http")
                .with_symbols(vec!["geturl".to_string(), "config".to_string()]),
        );
        let (graph, info) = map(&ir);

        let module_id = info.imports[0];
        let module = graph.get_node(module_id).unwrap();
        assert!(matches!(module.node_type, NodeType::Module));
        assert_eq!(module.properties.get_string("name"), Some("http"));
        assert_eq!(module.properties.get_string("is_external"), Some("true"));

        let edge = edge_between(&graph, info.file_id, module_id);
        assert!(matches!(edge.edge_type, EdgeType::Imports));
        assert_eq!(
            edge.properties.get_string_list_compat("symbols"),
            Some(vec!["geturl".to_string(), "config".to_string()])
        );
        // A non-wildcard import records no is_wildcard prop.
        assert_eq!(edge.properties.get_string("is_wildcard"), None);
    }

    #[test]
    fn wildcard_import_tags_edge() {
        let mut ir = CodeIR::new(PathBuf::from("test.tcl"));
        ir.add_import(ImportRelation::new("test", "pkg").wildcard());
        let (graph, info) = map(&ir);

        let edge = edge_between(&graph, info.file_id, info.imports[0]);
        assert_eq!(edge.properties.get_string("is_wildcard"), Some("true"));
    }

    #[test]
    fn duplicate_imports_reuse_one_module_node() {
        let mut ir = CodeIR::new(PathBuf::from("test.tcl"));
        ir.add_import(ImportRelation::new("test", "http"));
        ir.add_import(ImportRelation::new("test", "http"));
        let (graph, info) = map(&ir);

        // file + single deduped module node
        assert_eq!(graph.node_count(), 2);
        assert_eq!(info.imports.len(), 2);
        assert_eq!(info.imports[0], info.imports[1]);
        // Two Imports edges to the same module.
        let edges = graph
            .get_edges_between(info.file_id, info.imports[0])
            .unwrap();
        assert_eq!(edges.len(), 2);
    }

    #[test]
    fn resolved_call_creates_calls_edge() {
        let mut ir = CodeIR::new(PathBuf::from("test.tcl"));
        ir.add_function(FunctionEntity::new("caller", 1, 5));
        ir.add_function(FunctionEntity::new("callee", 6, 9));
        ir.add_call(CallRelation::new("caller", "callee", 3));
        let (graph, info) = map(&ir);

        let caller_id = info.functions[0];
        let callee_id = info.functions[1];
        let edge = edge_between(&graph, caller_id, callee_id);
        assert!(matches!(edge.edge_type, EdgeType::Calls));
        assert!(matches!(
            edge.properties.get("call_site_line"),
            Some(PropertyValue::Int(3))
        ));
    }

    #[test]
    fn unresolved_call_stored_on_caller_without_edge() {
        // A call whose callee is not in node_map is recorded as an
        // unresolved_calls string list on the caller, not a Calls edge.
        let mut ir = CodeIR::new(PathBuf::from("test.tcl"));
        ir.add_function(FunctionEntity::new("caller", 1, 5));
        ir.add_call(CallRelation::new("caller", "external_proc", 2));
        let (graph, info) = map(&ir);

        // file + caller only (no callee node created)
        assert_eq!(graph.node_count(), 2);
        assert_eq!(graph.edge_count(), 1); // just the file->caller Contains

        let caller = graph.get_node(info.functions[0]).unwrap();
        assert_eq!(
            caller.properties.get_string_list_compat("unresolved_calls"),
            Some(vec!["external_proc".to_string()])
        );
    }

    #[test]
    fn sdc_and_eda_data_attached_to_file_node() {
        // tcl-specific: SDC constraints and EDA commands serialize onto the
        // file node as JSON-string properties.
        let mut ir = CodeIR::new(PathBuf::from("test.tcl"));
        ir.set_module(ModuleEntity::new("test", "test.tcl", "tcl").with_line_count(10));

        let mut extra = default_extra();
        extra.sdc.clocks.push(SdcClock {
            name: "clk".to_string(),
            period: "10".to_string(),
            port: "clk_port".to_string(),
        });
        extra
            .eda
            .design_reads
            .push(("verilog".to_string(), "top.v".to_string()));

        let (graph, info) = map_with(&ir, &extra);
        let file = graph.get_node(info.file_id).unwrap();

        let clocks = file.properties.get_string("sdc_clocks").unwrap();
        assert!(clocks.contains("clk_port"));
        let reads = file.properties.get_string("eda_design_reads").unwrap();
        assert!(reads.contains("top.v"));
    }

    #[test]
    fn function_complexity_attaches_all_metric_props() {
        // A function carrying ComplexityMetrics expands into eight numeric
        // props plus the letter grade. cyclomatic = 1+4+2+3+1 = 11 -> grade C.
        let mut metrics = ComplexityMetrics::new()
            .with_branches(4)
            .with_loops(2)
            .with_logical_operators(3)
            .with_nesting_depth(5)
            .with_exception_handlers(1)
            .with_early_returns(2);
        metrics.calculate_cyclomatic();

        let mut ir = CodeIR::new(PathBuf::from("test.tcl"));
        ir.add_function(FunctionEntity::new("busy", 1, 40).with_complexity(metrics));
        let (graph, info) = map(&ir);

        let func = graph.get_node(info.functions[0]).unwrap();
        let p = &func.properties;
        assert!(matches!(p.get("complexity"), Some(PropertyValue::Int(11))));
        assert_eq!(p.get_string("complexity_grade"), Some("C"));
        assert!(matches!(
            p.get("complexity_branches"),
            Some(PropertyValue::Int(4))
        ));
        assert!(matches!(
            p.get("complexity_loops"),
            Some(PropertyValue::Int(2))
        ));
        assert!(matches!(
            p.get("complexity_logical_ops"),
            Some(PropertyValue::Int(3))
        ));
        assert!(matches!(
            p.get("complexity_nesting"),
            Some(PropertyValue::Int(5))
        ));
        assert!(matches!(
            p.get("complexity_exceptions"),
            Some(PropertyValue::Int(1))
        ));
        assert!(matches!(
            p.get("complexity_early_returns"),
            Some(PropertyValue::Int(2))
        ));
    }

    #[test]
    fn function_optional_props_attached_when_present() {
        // doc/parameters/attributes/body_prefix are only emitted when set.
        let func = FunctionEntity::new("proc", 1, 5)
            .with_doc("does a thing")
            .with_parameters(vec![Parameter::new("a"), Parameter::new("b")])
            .with_attributes(vec!["export".to_string()])
            .with_body_prefix("set x 1");
        let mut ir = CodeIR::new(PathBuf::from("test.tcl"));
        ir.add_function(func);
        let (graph, info) = map(&ir);

        let p = &graph.get_node(info.functions[0]).unwrap().properties;
        assert_eq!(p.get_string("doc"), Some("does a thing"));
        assert_eq!(
            p.get_string_list_compat("parameters"),
            Some(vec!["a".to_string(), "b".to_string()])
        );
        assert_eq!(
            p.get_string_list_compat("attributes"),
            Some(vec!["export".to_string()])
        );
        assert_eq!(p.get_string("body_prefix"), Some("set x 1"));
    }

    #[test]
    fn function_parent_class_links_to_earlier_function() {
        // Classes are mapped AFTER functions, so a parent_class name only
        // resolves in node_map if it belongs to an EARLIER function. Here the
        // child's parent is a previously-added function, so the Contains edge
        // comes from that function node, not the file.
        let mut ir = CodeIR::new(PathBuf::from("test.tcl"));
        ir.add_function(FunctionEntity::new("parent", 1, 20));
        ir.add_function(FunctionEntity::new("child", 5, 9).with_parent_class("parent"));
        let (graph, info) = map(&ir);

        let parent_id = info.functions[0];
        let child_id = info.functions[1];
        // child is contained by parent, not by the file.
        assert!(matches!(
            edge_between(&graph, parent_id, child_id).edge_type,
            EdgeType::Contains
        ));
        assert!(graph
            .get_edges_between(info.file_id, child_id)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn function_parent_class_unresolved_leaves_function_orphaned() {
        // When parent_class is set but never resolves in node_map (e.g. a
        // namespace mapped later, or a missing name), the inner lookup has no
        // else arm, so NO Contains edge is emitted - the function is orphaned
        // (not wired to the file).
        let mut ir = CodeIR::new(PathBuf::from("test.tcl"));
        ir.add_function(FunctionEntity::new("child", 5, 9).with_parent_class("Missing"));
        let (graph, info) = map(&ir);

        // file + function nodes, but zero edges.
        assert_eq!(graph.node_count(), 2);
        assert_eq!(graph.edge_count(), 0);
        assert!(graph
            .get_edges_between(info.file_id, info.functions[0])
            .unwrap()
            .is_empty());
    }

    #[test]
    fn module_doc_comment_attached_to_file_node() {
        let mut ir = CodeIR::new(PathBuf::from("test.tcl"));
        ir.set_module(
            ModuleEntity::new("test", "test.tcl", "tcl")
                .with_line_count(7)
                .with_doc("module docs"),
        );
        let (graph, info) = map(&ir);

        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(file.properties.get_string("doc"), Some("module docs"));
        assert_eq!(info.line_count, 7);
    }

    #[test]
    fn class_optional_props_attached_when_present() {
        let mut ir = CodeIR::new(PathBuf::from("test.tcl"));
        ir.add_class(
            ClassEntity::new("NS", 1, 30)
                .with_doc("namespace docs")
                .with_attributes(vec!["public".to_string()])
                .with_body_prefix("variable state"),
        );
        let (graph, info) = map(&ir);

        let p = &graph.get_node(info.classes[0]).unwrap().properties;
        assert_eq!(p.get_string("doc"), Some("namespace docs"));
        assert_eq!(
            p.get_string_list_compat("attributes"),
            Some(vec!["public".to_string()])
        );
        assert_eq!(p.get_string("body_prefix"), Some("variable state"));
    }
}
