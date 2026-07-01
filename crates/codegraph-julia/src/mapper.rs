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
    let mut class_ids = Vec::new();
    let mut trait_ids = Vec::new();
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
            .with("language", "julia");

        let id = graph
            .add_node(NodeType::CodeFile, props)
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
        node_map.insert(name, id);
        id
    };

    // Map structs / mutable structs → Class nodes
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

        graph
            .add_edge(file_id, class_id, EdgeType::Contains, PropertyMap::new())
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
    }

    // Map abstract types → Interface nodes
    for trait_entity in &ir.traits {
        let mut props = PropertyMap::new()
            .with("name", trait_entity.name.clone())
            .with("path", file_path.display().to_string())
            .with("visibility", trait_entity.visibility.clone())
            .with("line_start", trait_entity.line_start as i64)
            .with("line_end", trait_entity.line_end as i64);

        if let Some(ref doc) = trait_entity.doc_comment {
            props = props.with("doc", doc.clone());
        }

        let trait_id = graph
            .add_node(NodeType::Interface, props)
            .map_err(|e| ParserError::GraphError(e.to_string()))?;

        node_map.insert(trait_entity.name.clone(), trait_id);
        trait_ids.push(trait_id);

        graph
            .add_edge(file_id, trait_id, EdgeType::Contains, PropertyMap::new())
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
    }

    // Map functions
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
        if let Some(ref ret) = func.return_type {
            props = props.with("return_type", ret.clone());
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

    // Map imports
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
        if !import.symbols.is_empty() {
            edge_props = edge_props.with("symbols", import.symbols.clone());
        }
        graph
            .add_edge(file_id, import_id, EdgeType::Imports, edge_props)
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
    }

    // Map calls
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
        let info = ir_to_graph(ir, &mut graph, Path::new("Solver.jl")).unwrap();
        (graph, info)
    }

    fn name_of(graph: &CodeGraph, id: NodeId) -> String {
        match graph.get_node(id).unwrap().properties.get("name") {
            Some(PropertyValue::String(s)) => s.clone(),
            _ => String::new(),
        }
    }

    #[test]
    fn empty_ir_creates_file_node_from_path_stem() {
        let ir = CodeIR::new(std::path::PathBuf::from("Solver.jl"));
        let (graph, info) = build(&ir);

        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(
            file.properties.get("name"),
            Some(&PropertyValue::String("Solver".to_string()))
        );
        assert_eq!(
            file.properties.get("language"),
            Some(&PropertyValue::String("julia".to_string()))
        );
        assert!(info.functions.is_empty());
        assert!(info.classes.is_empty());
        assert!(info.traits.is_empty());
        assert!(info.imports.is_empty());
        assert_eq!(info.line_count, 0);
    }

    #[test]
    fn module_drives_file_node_metadata_and_line_count() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Solver.jl"));
        let mut module = ModuleEntity::new("Solver", "src/Solver.jl", "julia");
        module.line_count = 120;
        module.doc_comment = Some("module docs".to_string());
        ir.set_module(module);

        let (graph, info) = build(&ir);
        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(
            file.properties.get("name"),
            Some(&PropertyValue::String("Solver".to_string()))
        );
        assert_eq!(
            file.properties.get("path"),
            Some(&PropertyValue::String("src/Solver.jl".to_string()))
        );
        assert_eq!(
            file.properties.get("line_count"),
            Some(&PropertyValue::Int(120))
        );
        assert_eq!(
            file.properties.get("doc"),
            Some(&PropertyValue::String("module docs".to_string()))
        );
        assert_eq!(info.line_count, 120);
    }

    #[test]
    fn struct_maps_to_class_node_and_drops_its_methods() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Solver.jl"));
        let mut class = ClassEntity::new("Point", 1, 5)
            .with_visibility("public")
            .abstract_class();
        // The julia mapper never iterates class.methods, so this is dropped.
        class
            .methods
            .push(FunctionEntity::new("norm", 2, 4).with_visibility("public"));
        ir.add_class(class);

        let (graph, info) = build(&ir);
        assert_eq!(info.classes.len(), 1);
        // Methods on the ClassEntity are silently dropped by the julia mapper.
        assert!(info.functions.is_empty());
        // Only the file node and the class node exist.
        assert_eq!(graph.node_count(), 2);

        let class_id = info.classes[0];
        let class_node = graph.get_node(class_id).unwrap();
        assert_eq!(class_node.node_type, NodeType::Class);
        assert_eq!(name_of(&graph, class_id), "Point");
        assert_eq!(
            class_node.properties.get("is_abstract"),
            Some(&PropertyValue::Bool(true))
        );
        assert!(!graph
            .get_edges_between(info.file_id, class_id)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn abstract_type_maps_to_interface_node_contained_by_file() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Solver.jl"));
        let mut tr = TraitEntity::new("AbstractShape", 1, 3);
        tr.doc_comment = Some("abstract type".to_string());
        ir.add_trait(tr);

        let (graph, info) = build(&ir);
        assert_eq!(info.traits.len(), 1);

        let trait_id = info.traits[0];
        let trait_node = graph.get_node(trait_id).unwrap();
        assert_eq!(trait_node.node_type, NodeType::Interface);
        assert_eq!(name_of(&graph, trait_id), "AbstractShape");
        assert_eq!(
            trait_node.properties.get("doc"),
            Some(&PropertyValue::String("abstract type".to_string()))
        );

        let edge_ids = graph.get_edges_between(info.file_id, trait_id).unwrap();
        assert_eq!(edge_ids.len(), 1);
        assert_eq!(
            graph.get_edge(edge_ids[0]).unwrap().edge_type,
            EdgeType::Contains
        );
    }

    #[test]
    fn free_function_is_contained_by_file_with_complexity_and_flag_props() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Solver.jl"));
        let metrics = ComplexityMetrics {
            cyclomatic_complexity: 12,
            branches: 6,
            loops: 2,
            ..Default::default()
        };
        let func = FunctionEntity::new("solve", 1, 30)
            .with_signature("function solve(x)")
            .with_complexity(metrics)
            .async_fn();
        ir.add_function(func);

        let (graph, info) = build(&ir);
        assert_eq!(info.functions.len(), 1);

        let func_id = info.functions[0];
        // Julia keeps function names bare (no Class#/Class. qualification).
        assert_eq!(name_of(&graph, func_id), "solve");
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
            Some(&PropertyValue::Bool(true))
        );
        assert_eq!(
            node.properties.get("is_static"),
            Some(&PropertyValue::Bool(false))
        );
    }

    #[test]
    fn import_creates_external_module_with_symbols_edge_prop() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Solver.jl"));
        ir.add_import(
            ImportRelation::new("Solver", "LinearAlgebra").with_symbols(vec!["dot".to_string()]),
        );

        let (graph, info) = build(&ir);
        assert_eq!(info.imports.len(), 1);

        let import_id = info.imports[0];
        let import_node = graph.get_node(import_id).unwrap();
        assert_eq!(import_node.node_type, NodeType::Module);
        assert_eq!(
            import_node.properties.get("name"),
            Some(&PropertyValue::String("LinearAlgebra".to_string()))
        );
        assert_eq!(
            import_node.properties.get("is_external"),
            Some(&PropertyValue::String("true".to_string()))
        );

        let edge_ids = graph.get_edges_between(info.file_id, import_id).unwrap();
        assert_eq!(edge_ids.len(), 1);
        let edge = graph.get_edge(edge_ids[0]).unwrap();
        assert_eq!(edge.edge_type, EdgeType::Imports);
        // The julia mapper records the imported symbols on the edge.
        assert_eq!(
            edge.properties.get("symbols"),
            Some(&PropertyValue::StringList(vec!["dot".to_string()]))
        );
    }

    #[test]
    fn call_relation_wires_calls_edge_only_between_known_nodes() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Solver.jl"));
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

    fn prop(graph: &CodeGraph, id: NodeId, key: &str) -> Option<PropertyValue> {
        graph.get_node(id).unwrap().properties.get(key).cloned()
    }

    #[test]
    fn module_without_doc_omits_doc_prop() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Solver.jl"));
        let module = ModuleEntity::new("Solver", "src/Solver.jl", "julia");
        ir.set_module(module);

        let (graph, info) = build(&ir);
        assert_eq!(prop(&graph, info.file_id, "doc"), None);
    }

    #[test]
    fn function_optional_props_present() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Solver.jl"));
        let func = FunctionEntity::new("solve", 1, 5)
            .with_doc("solves it")
            .with_body_prefix("x + 1")
            .with_return_type("Int")
            .with_parameters(vec![Parameter::new("x"), Parameter::new("y")]);
        ir.add_function(func);

        let (graph, info) = build(&ir);
        let id = info.functions[0];
        assert_eq!(
            prop(&graph, id, "doc"),
            Some(PropertyValue::String("solves it".to_string()))
        );
        assert_eq!(
            prop(&graph, id, "body_prefix"),
            Some(PropertyValue::String("x + 1".to_string()))
        );
        assert_eq!(
            prop(&graph, id, "return_type"),
            Some(PropertyValue::String("Int".to_string()))
        );
        assert_eq!(
            prop(&graph, id, "parameters"),
            Some(PropertyValue::StringList(vec![
                "x".to_string(),
                "y".to_string()
            ]))
        );
    }

    #[test]
    fn function_optional_props_absent() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Solver.jl"));
        ir.add_function(FunctionEntity::new("solve", 1, 5));

        let (graph, info) = build(&ir);
        let id = info.functions[0];
        assert_eq!(prop(&graph, id, "doc"), None);
        assert_eq!(prop(&graph, id, "body_prefix"), None);
        assert_eq!(prop(&graph, id, "return_type"), None);
        assert_eq!(prop(&graph, id, "parameters"), None);
        assert_eq!(prop(&graph, id, "complexity"), None);
    }

    #[test]
    fn all_eight_complexity_sub_props_grade_d() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Solver.jl"));
        let metrics = ComplexityMetrics::new()
            .with_branches(15)
            .with_loops(4)
            .with_logical_operators(3)
            .with_exception_handlers(2)
            .with_nesting_depth(5)
            .with_early_returns(6)
            .finalize();
        // cyclomatic = 1 + 15 + 4 + 3 + 2 = 25 -> D band
        ir.add_function(FunctionEntity::new("f", 1, 40).with_complexity(metrics));

        let (graph, info) = build(&ir);
        let id = info.functions[0];
        assert_eq!(prop(&graph, id, "complexity"), Some(PropertyValue::Int(25)));
        assert_eq!(
            prop(&graph, id, "complexity_grade"),
            Some(PropertyValue::String("D".to_string()))
        );
        assert_eq!(
            prop(&graph, id, "complexity_branches"),
            Some(PropertyValue::Int(15))
        );
        assert_eq!(
            prop(&graph, id, "complexity_loops"),
            Some(PropertyValue::Int(4))
        );
        assert_eq!(
            prop(&graph, id, "complexity_logical_ops"),
            Some(PropertyValue::Int(3))
        );
        assert_eq!(
            prop(&graph, id, "complexity_exceptions"),
            Some(PropertyValue::Int(2))
        );
        assert_eq!(
            prop(&graph, id, "complexity_nesting"),
            Some(PropertyValue::Int(5))
        );
        assert_eq!(
            prop(&graph, id, "complexity_early_returns"),
            Some(PropertyValue::Int(6))
        );
    }

    #[test]
    fn complexity_grade_bands_a_and_f() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Solver.jl"));
        ir.add_function(
            FunctionEntity::new("simple", 1, 3)
                .with_complexity(ComplexityMetrics::new().with_branches(2).finalize()),
        );
        ir.add_function(
            FunctionEntity::new("beast", 4, 90)
                .with_complexity(ComplexityMetrics::new().with_branches(60).finalize()),
        );

        let (graph, info) = build(&ir);
        let simple = info
            .functions
            .iter()
            .copied()
            .find(|&id| name_of(&graph, id) == "simple")
            .unwrap();
        let beast = info
            .functions
            .iter()
            .copied()
            .find(|&id| name_of(&graph, id) == "beast")
            .unwrap();
        assert_eq!(
            prop(&graph, simple, "complexity_grade"),
            Some(PropertyValue::String("A".to_string()))
        );
        assert_eq!(
            prop(&graph, beast, "complexity_grade"),
            Some(PropertyValue::String("F".to_string()))
        );
    }

    #[test]
    fn function_static_and_abstract_flags() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Solver.jl"));
        ir.add_function(FunctionEntity::new("f", 1, 3).static_fn().abstract_fn());

        let (graph, info) = build(&ir);
        let id = info.functions[0];
        assert_eq!(
            prop(&graph, id, "is_static"),
            Some(PropertyValue::Bool(true))
        );
        assert_eq!(
            prop(&graph, id, "is_abstract"),
            Some(PropertyValue::Bool(true))
        );
        assert_eq!(
            prop(&graph, id, "is_async"),
            Some(PropertyValue::Bool(false))
        );
    }

    #[test]
    fn class_optional_props_present_and_absent() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Solver.jl"));
        ir.add_class(
            ClassEntity::new("Rich", 1, 5)
                .with_doc("a struct")
                .with_attributes(vec!["export".to_string()])
                .with_body_prefix("x::Int"),
        );
        ir.add_class(ClassEntity::new("Bare", 6, 8));

        let (graph, info) = build(&ir);
        let rich = info
            .classes
            .iter()
            .copied()
            .find(|&id| name_of(&graph, id) == "Rich")
            .unwrap();
        let bare = info
            .classes
            .iter()
            .copied()
            .find(|&id| name_of(&graph, id) == "Bare")
            .unwrap();
        assert_eq!(
            prop(&graph, rich, "doc"),
            Some(PropertyValue::String("a struct".to_string()))
        );
        assert_eq!(
            prop(&graph, rich, "attributes"),
            Some(PropertyValue::StringList(vec!["export".to_string()]))
        );
        assert_eq!(
            prop(&graph, rich, "body_prefix"),
            Some(PropertyValue::String("x::Int".to_string()))
        );
        assert_eq!(prop(&graph, bare, "doc"), None);
        assert_eq!(prop(&graph, bare, "attributes"), None);
        assert_eq!(prop(&graph, bare, "body_prefix"), None);
    }

    #[test]
    fn abstract_type_without_doc_omits_doc_prop() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Solver.jl"));
        ir.add_trait(TraitEntity::new("AbstractShape", 1, 3));

        let (graph, info) = build(&ir);
        let id = info.traits[0];
        assert_eq!(prop(&graph, id, "doc"), None);
        assert_eq!(
            prop(&graph, id, "visibility"),
            Some(PropertyValue::String("public".to_string()))
        );
    }

    #[test]
    fn import_matching_in_file_name_reuses_node_without_external_flag() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Solver.jl"));
        ir.add_function(FunctionEntity::new("helper", 1, 3));
        // Import target matches the already-mapped function name.
        ir.add_import(ImportRelation::new("Solver", "helper"));

        let (graph, info) = build(&ir);
        // No new Module node: file + function only.
        assert_eq!(graph.node_count(), 2);
        let import_id = info.imports[0];
        assert_eq!(import_id, info.functions[0]);
        // Reused node keeps its Function type, no is_external stamped.
        assert_eq!(
            graph.get_node(import_id).unwrap().node_type,
            NodeType::Function
        );
        assert_eq!(prop(&graph, import_id, "is_external"), None);
        // Both a Contains and an Imports edge now connect file -> node.
        let edge_ids = graph.get_edges_between(info.file_id, import_id).unwrap();
        assert_eq!(edge_ids.len(), 2);
        let kinds: Vec<_> = edge_ids
            .iter()
            .map(|&e| graph.get_edge(e).unwrap().edge_type)
            .collect();
        assert!(kinds.contains(&EdgeType::Contains));
        assert!(kinds.contains(&EdgeType::Imports));
    }

    #[test]
    fn bare_import_has_empty_edge_props() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Solver.jl"));
        ir.add_import(ImportRelation::new("Solver", "Printf"));

        let (graph, info) = build(&ir);
        let edge_ids = graph
            .get_edges_between(info.file_id, info.imports[0])
            .unwrap();
        assert_eq!(edge_ids.len(), 1);
        let edge = graph.get_edge(edge_ids[0]).unwrap();
        assert_eq!(edge.edge_type, EdgeType::Imports);
        // No symbols provided -> no symbols prop on the edge.
        assert_eq!(edge.properties.get("symbols"), None);
    }

    #[test]
    fn indirect_call_records_is_direct_false() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Solver.jl"));
        ir.add_function(FunctionEntity::new("caller", 1, 5));
        ir.add_function(FunctionEntity::new("callee", 6, 10));
        ir.add_call(CallRelation::new("caller", "callee", 3).indirect());

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
        assert_eq!(edge_ids.len(), 1);
        assert_eq!(
            graph
                .get_edge(edge_ids[0])
                .unwrap()
                .properties
                .get("is_direct"),
            Some(&PropertyValue::Bool(false))
        );
    }

    #[test]
    fn multiple_classes_functions_traits_all_contained_by_file() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Solver.jl"));
        ir.add_class(ClassEntity::new("Point", 1, 2));
        ir.add_class(ClassEntity::new("Line", 3, 4));
        ir.add_trait(TraitEntity::new("Shape", 5, 6));
        ir.add_function(FunctionEntity::new("area", 7, 8));

        let (graph, info) = build(&ir);
        assert_eq!(info.classes.len(), 2);
        assert_eq!(info.traits.len(), 1);
        assert_eq!(info.functions.len(), 1);
        // file + 2 classes + 1 trait + 1 function = 5 nodes.
        assert_eq!(graph.node_count(), 5);
        let neighbors = graph
            .get_neighbors(info.file_id, Direction::Outgoing)
            .unwrap();
        for id in info
            .classes
            .iter()
            .chain(info.traits.iter())
            .chain(info.functions.iter())
        {
            assert!(neighbors.contains(id));
        }
    }

    #[test]
    fn duplicate_import_target_reuses_existing_node() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Solver.jl"));
        ir.add_import(ImportRelation::new("Solver", "Base"));
        ir.add_import(ImportRelation::new("Solver", "Base"));

        let (graph, info) = build(&ir);
        assert_eq!(info.imports.len(), 2);
        assert_eq!(info.imports[0], info.imports[1]);
        let edges = graph
            .get_edges_between(info.file_id, info.imports[0])
            .unwrap();
        assert_eq!(edges.len(), 2);
    }
}
