// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Mapper for converting CodeIR to CodeGraph nodes and edges

use codegraph::{CodeGraph, EdgeType, NodeId, NodeType, PropertyMap};
use codegraph_parser_api::{CodeIR, FileInfo, ParserError};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

pub(crate) fn ir_to_graph(
    ir: &CodeIR,
    graph: &mut CodeGraph,
    file_path: &Path,
) -> Result<FileInfo, ParserError> {
    let mut node_map: HashMap<String, NodeId> = HashMap::new();
    let mut function_ids = Vec::new();
    let class_ids = Vec::new();
    let trait_ids = Vec::new();
    let mut import_ids = Vec::new();

    let file_id = if let Some(ref module) = ir.module {
        let mut props = PropertyMap::new()
            .with("name", module.name.clone())
            .with("path", module.path.clone())
            .with("language", module.language.clone())
            .with("line_count", module.line_count as i64);

        if let Some(ref doc) = module.doc_comment {
            props = props.with("doc", doc.clone());
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
            .with("language", "lua");

        let id = graph
            .add_node(NodeType::CodeFile, props)
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
        node_map.insert(name, id);
        id
    };

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
            .with("is_abstract", func.is_abstract);

        if let Some(ref doc) = func.doc_comment {
            props = props.with("doc", doc.clone());
        }
        if let Some(ref body) = func.body_prefix {
            props = props.with("body_prefix", body.clone());
        }
        if !func.parameters.is_empty() {
            let param_names: Vec<String> = func.parameters.iter().map(|p| p.name.clone()).collect();
            props = props.with("parameters", param_names);
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

        graph
            .add_edge(file_id, func_id, EdgeType::Contains, PropertyMap::new())
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
    }

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

        let edge_props = PropertyMap::new();
        graph
            .add_edge(file_id, import_id, EdgeType::Imports, edge_props)
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
    }

    for call in &ir.calls {
        if let Some(&caller_id) = node_map.get(&call.caller) {
            if let Some(&callee_id) = node_map.get(&call.callee) {
                let edge_props = PropertyMap::new()
                    .with("call_site_line", call.call_site_line as i64)
                    .with("is_direct", call.is_direct);

                graph
                    .add_edge(caller_id, callee_id, EdgeType::Calls, edge_props)
                    .map_err(|e| ParserError::GraphError(e.to_string()))?;
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
    use codegraph::{Direction, PropertyValue};
    use codegraph_parser_api::{
        CallRelation, ClassEntity, ComplexityMetrics, FunctionEntity, ImportRelation, ModuleEntity,
        Parameter, TraitEntity,
    };

    fn build(ir: &CodeIR) -> (CodeGraph, FileInfo) {
        let mut graph = CodeGraph::in_memory().unwrap();
        let info = ir_to_graph(ir, &mut graph, Path::new("main.lua")).unwrap();
        (graph, info)
    }

    fn name_of(graph: &CodeGraph, id: NodeId) -> String {
        match graph.get_node(id).unwrap().properties.get("name") {
            Some(PropertyValue::String(s)) => s.clone(),
            _ => String::new(),
        }
    }

    fn prop(graph: &CodeGraph, id: NodeId, key: &str) -> Option<PropertyValue> {
        graph.get_node(id).unwrap().properties.get(key).cloned()
    }

    #[test]
    fn empty_ir_creates_file_node_from_path_stem() {
        let ir = CodeIR::new(std::path::PathBuf::from("main.lua"));
        let (graph, info) = build(&ir);

        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(
            file.properties.get("name"),
            Some(&PropertyValue::String("main".to_string()))
        );
        assert_eq!(
            file.properties.get("language"),
            Some(&PropertyValue::String("lua".to_string()))
        );
        assert!(info.functions.is_empty());
        assert!(info.classes.is_empty());
        assert!(info.traits.is_empty());
        assert!(info.imports.is_empty());
        assert_eq!(info.line_count, 0);
        assert_eq!(graph.node_count(), 1);
    }

    #[test]
    fn module_drives_file_node_metadata_and_line_count() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("main.lua"));
        let mut module = ModuleEntity::new("main", "src/main.lua", "lua");
        module.line_count = 90;
        module.doc_comment = Some("module docs".to_string());
        ir.set_module(module);

        let (graph, info) = build(&ir);
        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(
            file.properties.get("name"),
            Some(&PropertyValue::String("main".to_string()))
        );
        assert_eq!(
            file.properties.get("path"),
            Some(&PropertyValue::String("src/main.lua".to_string()))
        );
        assert_eq!(
            file.properties.get("line_count"),
            Some(&PropertyValue::Int(90))
        );
        assert_eq!(
            file.properties.get("doc"),
            Some(&PropertyValue::String("module docs".to_string()))
        );
        assert_eq!(info.line_count, 90);
    }

    #[test]
    fn classes_are_ignored_by_the_mapper() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("main.lua"));
        let mut class = ClassEntity::new("Point", 1, 5).with_visibility("public");
        class
            .methods
            .push(FunctionEntity::new("norm", 2, 4).with_visibility("public"));
        ir.add_class(class);

        let (graph, info) = build(&ir);
        // The lua mapper never iterates ir.classes, so nothing is emitted.
        assert!(info.classes.is_empty());
        assert!(info.functions.is_empty());
        assert_eq!(graph.node_count(), 1);
    }

    #[test]
    fn traits_are_ignored_by_the_mapper() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("main.lua"));
        ir.add_trait(TraitEntity::new("Runnable", 1, 3));

        let (graph, info) = build(&ir);
        // The lua mapper never iterates ir.traits, so no Interface node exists.
        assert!(info.traits.is_empty());
        assert_eq!(graph.node_count(), 1);
        assert!(graph
            .nodes_iter()
            .all(|(_, node)| node.node_type != NodeType::Interface));
    }

    #[test]
    fn free_function_is_contained_by_file_with_complexity_and_flag_props() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("main.lua"));
        let metrics = ComplexityMetrics {
            cyclomatic_complexity: 12,
            branches: 6,
            loops: 2,
            ..Default::default()
        };
        let func = FunctionEntity::new("run", 1, 30)
            .with_signature("run()")
            .with_complexity(metrics);
        ir.add_function(func);

        let (graph, info) = build(&ir);
        assert_eq!(info.functions.len(), 1);

        let func_id = info.functions[0];
        // Lua keeps function names bare (no Class#/Class. qualification).
        assert_eq!(name_of(&graph, func_id), "run");
        let neighbors = graph
            .get_neighbors(info.file_id, Direction::Outgoing)
            .unwrap();
        assert!(neighbors.contains(&func_id));

        let node = graph.get_node(func_id).unwrap();
        assert_eq!(
            node.properties.get("complexity"),
            Some(&PropertyValue::Int(12))
        );
        // Grade 12 falls in the C band.
        assert_eq!(
            node.properties.get("complexity_grade"),
            Some(&PropertyValue::String("C".to_string()))
        );
        assert_eq!(
            node.properties.get("is_async"),
            Some(&PropertyValue::Bool(false))
        );
        assert_eq!(
            node.properties.get("is_static"),
            Some(&PropertyValue::Bool(false))
        );
    }

    #[test]
    fn import_creates_external_module_with_empty_edge_props() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("main.lua"));
        ir.add_import(ImportRelation::new("main", "utils").with_symbols(vec!["log".to_string()]));

        let (graph, info) = build(&ir);
        assert_eq!(info.imports.len(), 1);

        let import_id = info.imports[0];
        let import_node = graph.get_node(import_id).unwrap();
        assert_eq!(import_node.node_type, NodeType::Module);
        assert_eq!(
            import_node.properties.get("name"),
            Some(&PropertyValue::String("utils".to_string()))
        );
        assert_eq!(
            import_node.properties.get("is_external"),
            Some(&PropertyValue::String("true".to_string()))
        );

        let edge_ids = graph.get_edges_between(info.file_id, import_id).unwrap();
        assert_eq!(edge_ids.len(), 1);
        let edge = graph.get_edge(edge_ids[0]).unwrap();
        assert_eq!(edge.edge_type, EdgeType::Imports);
        // The lua mapper records NO props on the Imports edge (symbols dropped).
        assert_eq!(edge.properties.get("symbols"), None);
        assert_eq!(edge.properties.get("alias"), None);
        assert_eq!(edge.properties.get("is_wildcard"), None);
    }

    #[test]
    fn call_relation_wires_calls_edge_only_between_known_nodes() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("main.lua"));
        ir.add_function(FunctionEntity::new("caller", 1, 5));
        ir.add_function(FunctionEntity::new("callee", 6, 10));
        ir.add_call(CallRelation::new("caller", "callee", 3));
        // Unknown callee -> silently skipped.
        ir.add_call(CallRelation::new("caller", "ghost", 4));

        let (graph, info) = build(&ir);
        let caller_id = info
            .functions
            .iter()
            .copied()
            .find(|&id| name_of(&graph, id) == "caller")
            .unwrap();
        let callee_id = info
            .functions
            .iter()
            .copied()
            .find(|&id| name_of(&graph, id) == "callee")
            .unwrap();

        let call_edges: Vec<_> = graph
            .get_edges_between(caller_id, callee_id)
            .unwrap()
            .into_iter()
            .filter(|&e| graph.get_edge(e).unwrap().edge_type == EdgeType::Calls)
            .collect();
        assert_eq!(call_edges.len(), 1);
        let edge = graph.get_edge(call_edges[0]).unwrap();
        assert_eq!(
            edge.properties.get("call_site_line"),
            Some(&PropertyValue::Int(3))
        );

        let outgoing = graph.get_neighbors(caller_id, Direction::Outgoing).unwrap();
        assert_eq!(outgoing, vec![callee_id]);
    }

    #[test]
    fn duplicate_import_target_reuses_existing_node() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("main.lua"));
        ir.add_import(ImportRelation::new("main", "common"));
        ir.add_import(ImportRelation::new("main", "common"));

        let (graph, info) = build(&ir);
        assert_eq!(info.imports.len(), 2);
        assert_eq!(info.imports[0], info.imports[1]);
        let edges = graph
            .get_edges_between(info.file_id, info.imports[0])
            .unwrap();
        assert_eq!(edges.len(), 2);
    }

    #[test]
    fn function_optional_props_present() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("main.lua"));
        let mut func = FunctionEntity::new("run", 1, 10).with_signature("run(x)");
        func.doc_comment = Some("runs".to_string());
        func.body_prefix = Some("local y = x".to_string());
        func.parameters = vec![Parameter::new("x")];
        ir.add_function(func);

        let (graph, info) = build(&ir);
        let id = info.functions[0];
        assert_eq!(
            prop(&graph, id, "doc"),
            Some(PropertyValue::String("runs".to_string()))
        );
        assert_eq!(
            prop(&graph, id, "body_prefix"),
            Some(PropertyValue::String("local y = x".to_string()))
        );
        assert_eq!(
            prop(&graph, id, "parameters"),
            Some(PropertyValue::StringList(vec!["x".to_string()]))
        );
    }

    #[test]
    fn function_optional_props_absent() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("main.lua"));
        ir.add_function(FunctionEntity::new("run", 1, 10));

        let (graph, info) = build(&ir);
        let id = info.functions[0];
        assert_eq!(prop(&graph, id, "doc"), None);
        assert_eq!(prop(&graph, id, "body_prefix"), None);
        assert_eq!(prop(&graph, id, "parameters"), None);
        // The lua mapper never reads return_type or attributes on functions.
        assert_eq!(prop(&graph, id, "return_type"), None);
        assert_eq!(prop(&graph, id, "attributes"), None);
    }

    #[test]
    fn function_all_complexity_sub_props_stamped() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("main.lua"));
        let metrics = ComplexityMetrics {
            branches: 10,
            loops: 5,
            logical_operators: 4,
            max_nesting_depth: 3,
            exception_handlers: 2,
            early_returns: 1,
            ..Default::default()
        }
        .finalize();
        // 1 + 10 + 5 + 4 + 2 = 22 -> D band.
        ir.add_function(FunctionEntity::new("run", 1, 40).with_complexity(metrics));

        let (graph, info) = build(&ir);
        let id = info.functions[0];
        assert_eq!(prop(&graph, id, "complexity"), Some(PropertyValue::Int(22)));
        assert_eq!(
            prop(&graph, id, "complexity_grade"),
            Some(PropertyValue::String("D".to_string()))
        );
        assert_eq!(
            prop(&graph, id, "complexity_branches"),
            Some(PropertyValue::Int(10))
        );
        assert_eq!(
            prop(&graph, id, "complexity_loops"),
            Some(PropertyValue::Int(5))
        );
        assert_eq!(
            prop(&graph, id, "complexity_logical_ops"),
            Some(PropertyValue::Int(4))
        );
        assert_eq!(
            prop(&graph, id, "complexity_nesting"),
            Some(PropertyValue::Int(3))
        );
        assert_eq!(
            prop(&graph, id, "complexity_exceptions"),
            Some(PropertyValue::Int(2))
        );
        assert_eq!(
            prop(&graph, id, "complexity_early_returns"),
            Some(PropertyValue::Int(1))
        );
    }

    #[test]
    fn complexity_grade_bands_a_and_f() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("main.lua"));
        let low = ComplexityMetrics {
            cyclomatic_complexity: 3,
            ..Default::default()
        };
        let high = ComplexityMetrics {
            cyclomatic_complexity: 60,
            ..Default::default()
        };
        ir.add_function(FunctionEntity::new("simple", 1, 5).with_complexity(low));
        ir.add_function(FunctionEntity::new("hairy", 6, 90).with_complexity(high));

        let (graph, info) = build(&ir);
        let simple = info
            .functions
            .iter()
            .copied()
            .find(|&id| name_of(&graph, id) == "simple")
            .unwrap();
        let hairy = info
            .functions
            .iter()
            .copied()
            .find(|&id| name_of(&graph, id) == "hairy")
            .unwrap();
        assert_eq!(
            prop(&graph, simple, "complexity_grade"),
            Some(PropertyValue::String("A".to_string()))
        );
        assert_eq!(
            prop(&graph, hairy, "complexity_grade"),
            Some(PropertyValue::String("F".to_string()))
        );
    }

    #[test]
    fn function_without_complexity_omits_all_complexity_props() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("main.lua"));
        ir.add_function(FunctionEntity::new("run", 1, 10));

        let (graph, info) = build(&ir);
        let id = info.functions[0];
        assert_eq!(prop(&graph, id, "complexity"), None);
        assert_eq!(prop(&graph, id, "complexity_grade"), None);
        assert_eq!(prop(&graph, id, "complexity_branches"), None);
        assert_eq!(prop(&graph, id, "complexity_early_returns"), None);
    }

    #[test]
    fn function_boolean_flags_stamped() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("main.lua"));
        let mut func = FunctionEntity::new("run", 1, 10);
        func.is_async = true;
        func.is_static = true;
        func.is_abstract = true;
        ir.add_function(func);

        let (graph, info) = build(&ir);
        let id = info.functions[0];
        assert_eq!(
            prop(&graph, id, "is_async"),
            Some(PropertyValue::Bool(true))
        );
        assert_eq!(
            prop(&graph, id, "is_static"),
            Some(PropertyValue::Bool(true))
        );
        assert_eq!(
            prop(&graph, id, "is_abstract"),
            Some(PropertyValue::Bool(true))
        );
    }

    #[test]
    fn function_signature_visibility_and_line_bounds_stamped() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("main.lua"));
        ir.add_function(
            FunctionEntity::new("run", 7, 21)
                .with_signature("run(a, b)")
                .with_visibility("private"),
        );

        let (graph, info) = build(&ir);
        let id = info.functions[0];
        assert_eq!(
            prop(&graph, id, "signature"),
            Some(PropertyValue::String("run(a, b)".to_string()))
        );
        assert_eq!(
            prop(&graph, id, "visibility"),
            Some(PropertyValue::String("private".to_string()))
        );
        assert_eq!(prop(&graph, id, "line_start"), Some(PropertyValue::Int(7)));
        assert_eq!(prop(&graph, id, "line_end"), Some(PropertyValue::Int(21)));
    }

    #[test]
    fn import_matching_in_file_function_reuses_node() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("main.lua"));
        ir.add_function(FunctionEntity::new("helper", 1, 5));
        // Import name matches an already-mapped function -> reuse that node.
        ir.add_import(ImportRelation::new("main", "helper"));

        let (graph, info) = build(&ir);
        let func_id = info.functions[0];
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0], func_id);
        // Reused node is NOT re-stamped as external.
        assert_eq!(prop(&graph, func_id, "is_external"), None);
        assert_eq!(
            graph.get_node(func_id).unwrap().node_type,
            NodeType::Function
        );

        // Both a Contains and an Imports edge run file -> func.
        let edge_ids = graph.get_edges_between(info.file_id, func_id).unwrap();
        let types: Vec<EdgeType> = edge_ids
            .iter()
            .map(|&e| graph.get_edge(e).unwrap().edge_type)
            .collect();
        assert!(types.contains(&EdgeType::Contains));
        assert!(types.contains(&EdgeType::Imports));
    }

    #[test]
    fn indirect_call_records_is_direct_false() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("main.lua"));
        ir.add_function(FunctionEntity::new("caller", 1, 5));
        ir.add_function(FunctionEntity::new("callee", 6, 10));
        let mut call = CallRelation::new("caller", "callee", 3);
        call.is_direct = false;
        ir.add_call(call);

        let (graph, info) = build(&ir);
        let caller = info
            .functions
            .iter()
            .copied()
            .find(|&id| name_of(&graph, id) == "caller")
            .unwrap();
        let callee = info
            .functions
            .iter()
            .copied()
            .find(|&id| name_of(&graph, id) == "callee")
            .unwrap();
        let edge_ids = graph.get_edges_between(caller, callee).unwrap();
        let edge = graph.get_edge(edge_ids[0]).unwrap();
        assert_eq!(
            edge.properties.get("is_direct"),
            Some(&PropertyValue::Bool(false))
        );
    }

    #[test]
    fn multiple_functions_all_contained_by_file() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("main.lua"));
        ir.add_function(FunctionEntity::new("a", 1, 3));
        ir.add_function(FunctionEntity::new("b", 4, 6));
        ir.add_function(FunctionEntity::new("c", 7, 9));

        let (graph, info) = build(&ir);
        assert_eq!(info.functions.len(), 3);
        let neighbors = graph
            .get_neighbors(info.file_id, Direction::Outgoing)
            .unwrap();
        for &id in &info.functions {
            assert!(neighbors.contains(&id));
        }
    }

    #[test]
    fn module_without_doc_omits_doc_prop() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("main.lua"));
        let module = ModuleEntity::new("main", "src/main.lua", "lua");
        ir.set_module(module);

        let (graph, info) = build(&ir);
        assert_eq!(prop(&graph, info.file_id, "doc"), None);
    }
}
