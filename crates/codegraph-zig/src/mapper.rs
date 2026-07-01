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
            .with("language", "zig");

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
        if let Some(ref return_type) = func.return_type {
            props = props.with("return_type", return_type.clone());
        }
        if let Some(ref parent) = func.parent_class {
            props = props.with("parent_class", parent.clone());
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
        let info = ir_to_graph(ir, &mut graph, Path::new("core.zig")).unwrap();
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
        let ir = CodeIR::new(std::path::PathBuf::from("core.zig"));
        let (graph, info) = build(&ir);

        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(
            file.properties.get("name"),
            Some(&PropertyValue::String("core".to_string()))
        );
        assert_eq!(
            file.properties.get("language"),
            Some(&PropertyValue::String("zig".to_string()))
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
        let mut ir = CodeIR::new(std::path::PathBuf::from("core.zig"));
        let mut module = ModuleEntity::new("myapp.core", "src/myapp/core.zig", "zig");
        module.line_count = 120;
        module.doc_comment = Some("core module".to_string());
        ir.set_module(module);

        let (graph, info) = build(&ir);
        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(
            file.properties.get("name"),
            Some(&PropertyValue::String("myapp.core".to_string()))
        );
        assert_eq!(
            file.properties.get("path"),
            Some(&PropertyValue::String("src/myapp/core.zig".to_string()))
        );
        assert_eq!(
            file.properties.get("line_count"),
            Some(&PropertyValue::Int(120))
        );
        assert_eq!(
            file.properties.get("doc"),
            Some(&PropertyValue::String("core module".to_string()))
        );
        assert_eq!(info.line_count, 120);
    }

    #[test]
    fn class_is_contained_by_file_with_abstract_flag_but_no_methods() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("core.zig"));
        let mut class = ClassEntity::new("Shape", 1, 10).with_visibility("public");
        // struct-level method that the mapper must NOT emit as a node.
        class
            .methods
            .push(FunctionEntity::new("area", 2, 4).with_visibility("public"));
        ir.add_class(class);

        let (graph, info) = build(&ir);
        assert_eq!(info.classes.len(), 1);
        // The zig mapper never iterates class.methods, so no Function node exists.
        assert!(info.functions.is_empty());
        assert_eq!(graph.node_count(), 2);

        let class_id = info.classes[0];
        let node = graph.get_node(class_id).unwrap();
        assert_eq!(node.node_type, NodeType::Class);
        assert_eq!(
            node.properties.get("is_abstract"),
            Some(&PropertyValue::Bool(false))
        );
        // The zig mapper does not emit an is_interface prop on Class nodes.
        assert_eq!(node.properties.get("is_interface"), None);

        let neighbors = graph
            .get_neighbors(info.file_id, Direction::Outgoing)
            .unwrap();
        assert!(neighbors.contains(&class_id));
    }

    #[test]
    fn traits_are_ignored_by_the_mapper() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("core.zig"));
        ir.add_trait(TraitEntity::new("Comparable", 1, 3));

        let (graph, info) = build(&ir);
        // The zig mapper leaves trait_ids empty and never emits an Interface node.
        assert!(info.traits.is_empty());
        assert_eq!(graph.node_count(), 1);
        assert!(graph
            .nodes_iter()
            .all(|(_, node)| node.node_type != NodeType::Interface));
    }

    #[test]
    fn free_function_is_contained_by_file_with_complexity_and_flag_props() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("core.zig"));
        let metrics = ComplexityMetrics {
            cyclomatic_complexity: 12,
            branches: 6,
            loops: 2,
            ..Default::default()
        };
        let func = FunctionEntity::new("solve", 1, 30)
            .with_signature("fn solve(x: i32) i32")
            .with_complexity(metrics);
        ir.add_function(func);

        let (graph, info) = build(&ir);
        assert_eq!(info.functions.len(), 1);

        let func_id = info.functions[0];
        // Zig keeps function names bare (no Class#/Class. qualification).
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
            Some(&PropertyValue::Bool(false))
        );
        assert_eq!(
            node.properties.get("is_static"),
            Some(&PropertyValue::Bool(false))
        );
    }

    #[test]
    fn import_creates_external_module_with_empty_edge_props() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("core.zig"));
        ir.add_import(
            ImportRelation::new("myapp.core", "std").with_symbols(vec!["debug".to_string()]),
        );

        let (graph, info) = build(&ir);
        assert_eq!(info.imports.len(), 1);

        let import_id = info.imports[0];
        let import_node = graph.get_node(import_id).unwrap();
        assert_eq!(import_node.node_type, NodeType::Module);
        assert_eq!(
            import_node.properties.get("name"),
            Some(&PropertyValue::String("std".to_string()))
        );
        assert_eq!(
            import_node.properties.get("is_external"),
            Some(&PropertyValue::String("true".to_string()))
        );

        let edge_ids = graph.get_edges_between(info.file_id, import_id).unwrap();
        assert_eq!(edge_ids.len(), 1);
        let edge = graph.get_edge(edge_ids[0]).unwrap();
        assert_eq!(edge.edge_type, EdgeType::Imports);
        // The zig mapper records NO props on the Imports edge (symbols dropped).
        assert_eq!(edge.properties.get("symbols"), None);
        assert_eq!(edge.properties.get("alias"), None);
        assert_eq!(edge.properties.get("is_wildcard"), None);
    }

    #[test]
    fn call_relation_wires_calls_edge_only_between_known_nodes() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("core.zig"));
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
        let mut ir = CodeIR::new(std::path::PathBuf::from("core.zig"));
        ir.add_import(ImportRelation::new("myapp.core", "builtin"));
        ir.add_import(ImportRelation::new("myapp.core", "builtin"));

        let (graph, info) = build(&ir);
        assert_eq!(info.imports.len(), 2);
        assert_eq!(info.imports[0], info.imports[1]);
        let edges = graph
            .get_edges_between(info.file_id, info.imports[0])
            .unwrap();
        assert_eq!(edges.len(), 2);
    }

    #[test]
    fn function_optional_props_present_are_stamped() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("core.zig"));
        let func = FunctionEntity::new("run", 1, 8)
            .with_doc("runs the thing")
            .with_return_type("void")
            .with_body_prefix("{ return; }")
            .with_parameters(vec![Parameter::new("arg").with_type("i32")]);
        ir.add_function(func);

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.functions[0]).unwrap();
        assert_eq!(
            node.properties.get("doc"),
            Some(&PropertyValue::String("runs the thing".to_string()))
        );
        assert_eq!(
            node.properties.get("return_type"),
            Some(&PropertyValue::String("void".to_string()))
        );
        assert_eq!(
            node.properties.get("body_prefix"),
            Some(&PropertyValue::String("{ return; }".to_string()))
        );
        assert_eq!(
            node.properties.get("parameters"),
            Some(&PropertyValue::StringList(vec!["arg".to_string()]))
        );
    }

    #[test]
    fn function_optional_props_absent_are_omitted() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("core.zig"));
        ir.add_function(FunctionEntity::new("bare", 1, 2));

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.functions[0]).unwrap();
        assert_eq!(node.properties.get("doc"), None);
        assert_eq!(node.properties.get("return_type"), None);
        assert_eq!(node.properties.get("body_prefix"), None);
        assert_eq!(node.properties.get("parameters"), None);
        assert_eq!(node.properties.get("parent_class"), None);
        // Complexity props are only stamped when func.complexity is Some.
        assert_eq!(node.properties.get("complexity"), None);
        assert_eq!(node.properties.get("complexity_grade"), None);
    }

    #[test]
    fn all_eight_complexity_sub_props_are_stamped_with_d_grade() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("core.zig"));
        let metrics = ComplexityMetrics::new()
            .with_branches(20)
            .with_loops(2)
            .with_logical_operators(1)
            .with_nesting_depth(4)
            .with_exception_handlers(1)
            .with_early_returns(3)
            .finalize();
        // cyclomatic = 1 + 20 + 2 + 1 + 1 = 25 -> D band.
        ir.add_function(FunctionEntity::new("heavy", 1, 40).with_complexity(metrics));

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.functions[0]).unwrap();
        assert_eq!(
            node.properties.get("complexity"),
            Some(&PropertyValue::Int(25))
        );
        assert_eq!(
            node.properties.get("complexity_grade"),
            Some(&PropertyValue::String("D".to_string()))
        );
        assert_eq!(
            node.properties.get("complexity_branches"),
            Some(&PropertyValue::Int(20))
        );
        assert_eq!(
            node.properties.get("complexity_loops"),
            Some(&PropertyValue::Int(2))
        );
        assert_eq!(
            node.properties.get("complexity_logical_ops"),
            Some(&PropertyValue::Int(1))
        );
        assert_eq!(
            node.properties.get("complexity_nesting"),
            Some(&PropertyValue::Int(4))
        );
        assert_eq!(
            node.properties.get("complexity_exceptions"),
            Some(&PropertyValue::Int(1))
        );
        assert_eq!(
            node.properties.get("complexity_early_returns"),
            Some(&PropertyValue::Int(3))
        );
    }

    #[test]
    fn complexity_grade_bands_a_and_f() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("core.zig"));
        ir.add_function(
            FunctionEntity::new("simple", 1, 2)
                .with_complexity(ComplexityMetrics::new().finalize()),
        );
        ir.add_function(
            FunctionEntity::new("nightmare", 3, 4)
                .with_complexity(ComplexityMetrics::new().with_branches(60).finalize()),
        );

        let (graph, info) = build(&ir);
        let simple = graph.get_node(info.functions[0]).unwrap();
        assert_eq!(
            simple.properties.get("complexity_grade"),
            Some(&PropertyValue::String("A".to_string()))
        );
        let nightmare = graph.get_node(info.functions[1]).unwrap();
        assert_eq!(
            nightmare.properties.get("complexity_grade"),
            Some(&PropertyValue::String("F".to_string()))
        );
    }

    #[test]
    fn function_boolean_flags_are_stamped() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("core.zig"));
        ir.add_function(
            FunctionEntity::new("flagged", 1, 2)
                .async_fn()
                .static_fn()
                .abstract_fn(),
        );

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.functions[0]).unwrap();
        assert_eq!(
            node.properties.get("is_async"),
            Some(&PropertyValue::Bool(true))
        );
        assert_eq!(
            node.properties.get("is_static"),
            Some(&PropertyValue::Bool(true))
        );
        assert_eq!(
            node.properties.get("is_abstract"),
            Some(&PropertyValue::Bool(true))
        );
    }

    #[test]
    fn class_optional_props_present_are_stamped() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("core.zig"));
        let class = ClassEntity::new("Widget", 1, 20)
            .with_visibility("public")
            .with_doc("a widget")
            .with_attributes(vec!["packed".to_string()])
            .with_body_prefix("{ x: i32 }");
        ir.add_class(class);

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.classes[0]).unwrap();
        assert_eq!(
            node.properties.get("doc"),
            Some(&PropertyValue::String("a widget".to_string()))
        );
        assert_eq!(
            node.properties.get("attributes"),
            Some(&PropertyValue::StringList(vec!["packed".to_string()]))
        );
        assert_eq!(
            node.properties.get("body_prefix"),
            Some(&PropertyValue::String("{ x: i32 }".to_string()))
        );
    }

    #[test]
    fn class_optional_props_absent_are_omitted() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("core.zig"));
        ir.add_class(ClassEntity::new("Bare", 1, 2));

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.classes[0]).unwrap();
        assert_eq!(node.properties.get("doc"), None);
        assert_eq!(node.properties.get("attributes"), None);
        assert_eq!(node.properties.get("body_prefix"), None);
    }

    #[test]
    fn import_matching_in_file_name_reuses_node_without_external_flag() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("core.zig"));
        // A function is mapped before imports, so an import of the same name reuses it.
        ir.add_function(FunctionEntity::new("helper", 1, 3));
        ir.add_import(ImportRelation::new("myapp.core", "helper"));

        let (graph, info) = build(&ir);
        assert_eq!(info.imports.len(), 1);
        // The reused node is the function node, not a fresh Module node.
        assert_eq!(info.imports[0], info.functions[0]);
        let node = graph.get_node(info.imports[0]).unwrap();
        assert_eq!(node.node_type, NodeType::Function);
        // No is_external stamped because the existing node was reused.
        assert_eq!(node.properties.get("is_external"), None);

        // File -> helper carries both the Contains and the Imports edge.
        let edge_ids = graph
            .get_edges_between(info.file_id, info.imports[0])
            .unwrap();
        let kinds: Vec<_> = edge_ids
            .iter()
            .map(|&e| graph.get_edge(e).unwrap().edge_type)
            .collect();
        assert!(kinds.contains(&EdgeType::Contains));
        assert!(kinds.contains(&EdgeType::Imports));
    }

    #[test]
    fn indirect_call_records_is_direct_false() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("core.zig"));
        ir.add_function(FunctionEntity::new("caller", 1, 5));
        ir.add_function(FunctionEntity::new("callee", 6, 10));
        ir.add_call(CallRelation::new("caller", "callee", 3).indirect());

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
        let edge_ids = graph.get_edges_between(caller_id, callee_id).unwrap();
        let edge = graph.get_edge(edge_ids[0]).unwrap();
        assert_eq!(
            edge.properties.get("is_direct"),
            Some(&PropertyValue::Bool(false))
        );
    }

    #[test]
    fn function_with_unmapped_parent_class_is_orphaned() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("core.zig"));
        // Functions are mapped before classes, so the parent class is not yet in
        // node_map: the function gets NO Contains edge from either the class or file.
        ir.add_function(FunctionEntity::new("method", 2, 4).with_parent_class("Owner"));
        ir.add_class(ClassEntity::new("Owner", 1, 10));

        let (graph, info) = build(&ir);
        let func_id = info.functions[0];
        let class_id = info.classes[0];
        assert_eq!(
            graph
                .get_node(func_id)
                .unwrap()
                .properties
                .get("parent_class"),
            Some(&PropertyValue::String("Owner".to_string()))
        );
        // No incoming Contains edge from the class.
        assert!(graph
            .get_edges_between(class_id, func_id)
            .unwrap()
            .is_empty());
        // And none from the file either (the else branch is skipped when parent_class is Some).
        assert!(graph
            .get_edges_between(info.file_id, func_id)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn multiple_functions_and_classes_are_all_contained_by_file() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("core.zig"));
        ir.add_function(FunctionEntity::new("f1", 1, 2));
        ir.add_function(FunctionEntity::new("f2", 3, 4));
        ir.add_class(ClassEntity::new("C1", 5, 6));
        ir.add_class(ClassEntity::new("C2", 7, 8));

        let (graph, info) = build(&ir);
        assert_eq!(info.functions.len(), 2);
        assert_eq!(info.classes.len(), 2);
        let neighbors = graph
            .get_neighbors(info.file_id, Direction::Outgoing)
            .unwrap();
        for id in info.functions.iter().chain(info.classes.iter()) {
            assert!(neighbors.contains(id));
        }
    }
}
