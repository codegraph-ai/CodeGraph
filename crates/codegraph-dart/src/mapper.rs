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

    // Create module/file node
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
            .with("language", "dart");

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

    // Add classes
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

        for method in &class.methods {
            let method_name = format!("{}#{}", class.name, method.name);
            let mut method_props = PropertyMap::new()
                .with("name", method_name.clone())
                .with("path", file_path.display().to_string())
                .with("signature", method.signature.clone())
                .with("visibility", method.visibility.clone())
                .with("line_start", method.line_start as i64)
                .with("line_end", method.line_end as i64)
                .with("is_method", "true")
                .with("parent_class", class.name.clone());

            if let Some(ref doc) = method.doc_comment {
                method_props = method_props.with("doc", doc.clone());
            }
            if let Some(ref body) = method.body_prefix {
                method_props = method_props.with("body_prefix", body.clone());
            }

            let method_id = graph
                .add_node(NodeType::Function, method_props)
                .map_err(|e| ParserError::GraphError(e.to_string()))?;

            node_map.insert(method_name, method_id);
            function_ids.push(method_id);

            graph
                .add_edge(class_id, method_id, EdgeType::Contains, PropertyMap::new())
                .map_err(|e| ParserError::GraphError(e.to_string()))?;
        }
    }

    // Add traits/interfaces
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

    // Add imports
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
        if let Some(ref alias) = import.alias {
            edge_props = edge_props.with("alias", alias.clone());
        }
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

    // Add inheritance relationships
    for inheritance in &ir.inheritance {
        if let (Some(&child_id), Some(&parent_id)) = (
            node_map.get(&inheritance.child),
            node_map.get(&inheritance.parent),
        ) {
            let edge_props = PropertyMap::new().with("order", inheritance.order as i64);

            graph
                .add_edge(child_id, parent_id, EdgeType::Extends, edge_props)
                .map_err(|e| ParserError::GraphError(e.to_string()))?;
        }
    }

    // Add implementation relationships
    for impl_rel in &ir.implementations {
        if let (Some(&implementor_id), Some(&trait_id)) = (
            node_map.get(&impl_rel.implementor),
            node_map.get(&impl_rel.trait_name),
        ) {
            graph
                .add_edge(
                    implementor_id,
                    trait_id,
                    EdgeType::Implements,
                    PropertyMap::new(),
                )
                .map_err(|e| ParserError::GraphError(e.to_string()))?;
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
        CallRelation, ClassEntity, ComplexityMetrics, FunctionEntity, ImplementationRelation,
        ImportRelation, InheritanceRelation, ModuleEntity, Parameter, TraitEntity,
    };

    fn build(ir: &CodeIR) -> (CodeGraph, FileInfo) {
        let mut graph = CodeGraph::in_memory().unwrap();
        let info = ir_to_graph(ir, &mut graph, Path::new("widget.dart")).unwrap();
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
        let ir = CodeIR::new(std::path::PathBuf::from("widget.dart"));
        let (graph, info) = build(&ir);

        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(
            file.properties.get("name"),
            Some(&PropertyValue::String("widget".to_string()))
        );
        assert_eq!(
            file.properties.get("language"),
            Some(&PropertyValue::String("dart".to_string()))
        );
        assert!(info.functions.is_empty());
        assert!(info.classes.is_empty());
        assert!(info.traits.is_empty());
        assert!(info.imports.is_empty());
        assert_eq!(info.line_count, 0);
    }

    #[test]
    fn module_drives_file_node_metadata_and_line_count() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("widget.dart"));
        let mut module = ModuleEntity::new("my_widget", "lib/widget.dart", "dart");
        module.line_count = 88;
        module.doc_comment = Some("library docs".to_string());
        ir.set_module(module);

        let (graph, info) = build(&ir);
        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(
            file.properties.get("name"),
            Some(&PropertyValue::String("my_widget".to_string()))
        );
        assert_eq!(
            file.properties.get("path"),
            Some(&PropertyValue::String("lib/widget.dart".to_string()))
        );
        assert_eq!(
            file.properties.get("line_count"),
            Some(&PropertyValue::Int(88))
        );
        assert_eq!(
            file.properties.get("doc"),
            Some(&PropertyValue::String("library docs".to_string()))
        );
        assert_eq!(info.line_count, 88);
    }

    #[test]
    fn class_with_method_links_via_contains_edges_with_hash_name() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("widget.dart"));
        let mut class = ClassEntity::new("Counter", 1, 20)
            .with_visibility("public")
            .abstract_class();
        class
            .methods
            .push(FunctionEntity::new("increment", 5, 10).with_visibility("public"));
        ir.add_class(class);

        let (graph, info) = build(&ir);
        assert_eq!(info.classes.len(), 1);
        assert_eq!(info.functions.len(), 1);

        let class_id = info.classes[0];
        let class_node = graph.get_node(class_id).unwrap();
        assert_eq!(class_node.node_type, NodeType::Class);
        assert_eq!(
            class_node.properties.get("is_abstract"),
            Some(&PropertyValue::Bool(true))
        );
        assert!(!graph
            .get_edges_between(info.file_id, class_id)
            .unwrap()
            .is_empty());

        // Dart qualifies method names as Class#method (hash separator).
        let method_id = info.functions[0];
        assert_eq!(name_of(&graph, method_id), "Counter#increment");
        let method_node = graph.get_node(method_id).unwrap();
        assert_eq!(method_node.node_type, NodeType::Function);
        assert_eq!(
            method_node.properties.get("parent_class"),
            Some(&PropertyValue::String("Counter".to_string()))
        );
        assert_eq!(
            method_node.properties.get("is_method"),
            Some(&PropertyValue::String("true".to_string()))
        );
        assert!(!graph
            .get_edges_between(class_id, method_id)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn trait_maps_to_interface_node_without_methods() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("widget.dart"));
        let mut mixin = TraitEntity::new("Comparable", 1, 8);
        mixin
            .required_methods
            .push(FunctionEntity::new("compareTo", 2, 3));
        ir.add_trait(mixin);

        let (graph, info) = build(&ir);
        assert_eq!(info.traits.len(), 1);
        // The dart mapper does not emit interface method nodes.
        assert!(info.functions.is_empty());

        let iface_node = graph.get_node(info.traits[0]).unwrap();
        assert_eq!(iface_node.node_type, NodeType::Interface);
        assert_eq!(
            iface_node.properties.get("name"),
            Some(&PropertyValue::String("Comparable".to_string()))
        );
        assert!(!graph
            .get_edges_between(info.file_id, info.traits[0])
            .unwrap()
            .is_empty());
    }

    #[test]
    fn free_function_is_contained_by_file_with_complexity_and_flag_props() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("widget.dart"));
        let metrics = ComplexityMetrics {
            cyclomatic_complexity: 12,
            branches: 6,
            loops: 2,
            ..Default::default()
        };
        let func = FunctionEntity::new("build", 1, 30)
            .with_signature("Widget build(BuildContext ctx)")
            .with_complexity(metrics)
            .async_fn();
        ir.add_function(func);

        let (graph, info) = build(&ir);
        assert_eq!(info.functions.len(), 1);

        let func_id = info.functions[0];
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
    fn import_creates_external_module_and_records_edge_props() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("widget.dart"));
        ir.add_import(
            ImportRelation::new("widget", "package:flutter/material.dart")
                .with_alias("m")
                .wildcard()
                .with_symbols(vec!["Widget".to_string(), "State".to_string()]),
        );

        let (graph, info) = build(&ir);
        assert_eq!(info.imports.len(), 1);

        let import_id = info.imports[0];
        let import_node = graph.get_node(import_id).unwrap();
        assert_eq!(import_node.node_type, NodeType::Module);
        assert_eq!(
            import_node.properties.get("is_external"),
            Some(&PropertyValue::String("true".to_string()))
        );

        let edge_ids = graph.get_edges_between(info.file_id, import_id).unwrap();
        assert_eq!(edge_ids.len(), 1);
        let edge = graph.get_edge(edge_ids[0]).unwrap();
        assert_eq!(edge.edge_type, EdgeType::Imports);
        assert_eq!(
            edge.properties.get("alias"),
            Some(&PropertyValue::String("m".to_string()))
        );
        assert_eq!(
            edge.properties.get("is_wildcard"),
            Some(&PropertyValue::String("true".to_string()))
        );
    }

    #[test]
    fn call_relation_wires_calls_edge_only_between_known_nodes() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("widget.dart"));
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
        let mut ir = CodeIR::new(std::path::PathBuf::from("widget.dart"));
        ir.add_import(ImportRelation::new("widget", "dart:core"));
        ir.add_import(ImportRelation::new("widget", "dart:core"));

        let (graph, info) = build(&ir);
        assert_eq!(info.imports.len(), 2);
        assert_eq!(info.imports[0], info.imports[1]);
        let edges = graph
            .get_edges_between(info.file_id, info.imports[0])
            .unwrap();
        assert_eq!(edges.len(), 2);
    }

    #[test]
    fn inheritance_and_implementation_wire_extends_and_implements_edges() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("widget.dart"));
        ir.add_class(ClassEntity::new("Base", 1, 5));
        ir.add_class(ClassEntity::new("Derived", 6, 10));
        ir.add_trait(TraitEntity::new("Drawable", 11, 12));
        ir.add_inheritance(InheritanceRelation::new("Derived", "Base").with_order(2));
        ir.add_implementation(ImplementationRelation::new("Derived", "Drawable"));

        let (graph, info) = build(&ir);

        // Classes are added in IR order (Base, Derived); the trait is the sole interface.
        let find = |ids: &[NodeId], name: &str| -> NodeId {
            ids.iter()
                .copied()
                .find(|&id| name_of(&graph, id) == name)
                .unwrap()
        };
        let base = find(&info.classes, "Base");
        let derived = find(&info.classes, "Derived");
        let drawable = find(&info.traits, "Drawable");

        let extends: Vec<_> = graph
            .get_edges_between(derived, base)
            .unwrap()
            .into_iter()
            .filter(|&e| graph.get_edge(e).unwrap().edge_type == EdgeType::Extends)
            .collect();
        assert_eq!(extends.len(), 1);
        assert_eq!(
            graph.get_edge(extends[0]).unwrap().properties.get("order"),
            Some(&PropertyValue::Int(2))
        );

        let implements: Vec<_> = graph
            .get_edges_between(derived, drawable)
            .unwrap()
            .into_iter()
            .filter(|&e| graph.get_edge(e).unwrap().edge_type == EdgeType::Implements)
            .collect();
        assert_eq!(implements.len(), 1);
    }

    #[test]
    fn free_function_records_optional_doc_return_type_params_body_prefix() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("widget.dart"));
        let func = FunctionEntity::new("sum", 1, 4)
            .with_doc("Adds two ints")
            .with_return_type("int")
            .with_body_prefix("return a + b;")
            .with_parameters(vec![
                Parameter::new("a").with_type("int"),
                Parameter::new("b").with_type("int"),
            ]);
        ir.add_function(func);

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.functions[0]).unwrap();
        assert_eq!(
            node.properties.get("doc"),
            Some(&PropertyValue::String("Adds two ints".to_string()))
        );
        assert_eq!(
            node.properties.get("return_type"),
            Some(&PropertyValue::String("int".to_string()))
        );
        assert_eq!(
            node.properties.get("body_prefix"),
            Some(&PropertyValue::String("return a + b;".to_string()))
        );
        // Only parameter names are stored, as a string list.
        assert_eq!(
            node.properties.get("parameters"),
            Some(&PropertyValue::StringList(vec![
                "a".to_string(),
                "b".to_string()
            ]))
        );
    }

    #[test]
    fn free_function_omits_optional_props_when_absent() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("widget.dart"));
        ir.add_function(FunctionEntity::new("noop", 1, 2));

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.functions[0]).unwrap();
        assert!(node.properties.get("doc").is_none());
        assert!(node.properties.get("return_type").is_none());
        assert!(node.properties.get("parent_class").is_none());
        assert!(node.properties.get("body_prefix").is_none());
        assert!(node.properties.get("parameters").is_none());
        assert!(node.properties.get("complexity").is_none());
    }

    #[test]
    fn free_function_with_parent_class_gets_no_contains_edge() {
        // Functions are mapped BEFORE classes, so a free function whose
        // parent_class names a class not yet in the node_map wires neither a
        // class Contains edge nor the file fallback edge - it is left orphaned.
        let mut ir = CodeIR::new(std::path::PathBuf::from("widget.dart"));
        ir.add_function(FunctionEntity::new("method", 2, 4).with_parent_class("Counter"));
        ir.add_class(ClassEntity::new("Counter", 1, 5));

        let (graph, info) = build(&ir);
        let func_id = info.functions[0];
        let class_id = info.classes[0];

        let node = graph.get_node(func_id).unwrap();
        assert_eq!(
            node.properties.get("parent_class"),
            Some(&PropertyValue::String("Counter".to_string()))
        );

        let from_file = graph
            .get_neighbors(info.file_id, Direction::Outgoing)
            .unwrap();
        assert!(!from_file.contains(&func_id));
        let from_class = graph.get_neighbors(class_id, Direction::Outgoing).unwrap();
        assert!(!from_class.contains(&func_id));
    }

    #[test]
    fn function_records_all_eight_complexity_subproperties() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("widget.dart"));
        let metrics = ComplexityMetrics {
            cyclomatic_complexity: 9,
            branches: 3,
            loops: 2,
            logical_operators: 1,
            max_nesting_depth: 4,
            exception_handlers: 2,
            early_returns: 5,
        };
        ir.add_function(FunctionEntity::new("busy", 1, 40).with_complexity(metrics));

        let (graph, info) = build(&ir);
        let p = &graph.get_node(info.functions[0]).unwrap().properties;
        assert_eq!(p.get("complexity"), Some(&PropertyValue::Int(9)));
        // 9 falls in the B band.
        assert_eq!(
            p.get("complexity_grade"),
            Some(&PropertyValue::String("B".to_string()))
        );
        assert_eq!(p.get("complexity_branches"), Some(&PropertyValue::Int(3)));
        assert_eq!(p.get("complexity_loops"), Some(&PropertyValue::Int(2)));
        assert_eq!(
            p.get("complexity_logical_ops"),
            Some(&PropertyValue::Int(1))
        );
        assert_eq!(p.get("complexity_nesting"), Some(&PropertyValue::Int(4)));
        assert_eq!(p.get("complexity_exceptions"), Some(&PropertyValue::Int(2)));
        assert_eq!(
            p.get("complexity_early_returns"),
            Some(&PropertyValue::Int(5))
        );
    }

    #[test]
    fn function_complexity_grade_spans_all_bands() {
        let grade_for = |cc: u32| -> String {
            let mut ir = CodeIR::new(std::path::PathBuf::from("widget.dart"));
            let metrics = ComplexityMetrics {
                cyclomatic_complexity: cc,
                ..Default::default()
            };
            ir.add_function(FunctionEntity::new("f", 1, 2).with_complexity(metrics));
            let (graph, info) = build(&ir);
            match graph
                .get_node(info.functions[0])
                .unwrap()
                .properties
                .get("complexity_grade")
            {
                Some(PropertyValue::String(s)) => s.clone(),
                _ => String::new(),
            }
        };
        assert_eq!(grade_for(3), "A");
        assert_eq!(grade_for(15), "C");
        assert_eq!(grade_for(30), "D");
        assert_eq!(grade_for(60), "F");
    }

    #[test]
    fn static_function_sets_is_static_flag() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("widget.dart"));
        ir.add_function(FunctionEntity::new("create", 1, 2).static_fn());

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.functions[0]).unwrap();
        assert_eq!(
            node.properties.get("is_static"),
            Some(&PropertyValue::Bool(true))
        );
    }

    #[test]
    fn class_records_optional_doc_attributes_and_body_prefix() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("widget.dart"));
        let class = ClassEntity::new("Counter", 1, 20)
            .with_doc("A counter")
            .with_attributes(vec!["@immutable".to_string()])
            .with_body_prefix("int value = 0;");
        ir.add_class(class);

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.classes[0]).unwrap();
        assert_eq!(
            node.properties.get("doc"),
            Some(&PropertyValue::String("A counter".to_string()))
        );
        assert_eq!(
            node.properties.get("attributes"),
            Some(&PropertyValue::StringList(vec!["@immutable".to_string()]))
        );
        assert_eq!(
            node.properties.get("body_prefix"),
            Some(&PropertyValue::String("int value = 0;".to_string()))
        );
    }

    #[test]
    fn class_omits_optional_props_when_absent() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("widget.dart"));
        ir.add_class(ClassEntity::new("Bare", 1, 2));

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.classes[0]).unwrap();
        assert!(node.properties.get("doc").is_none());
        assert!(node.properties.get("attributes").is_none());
        assert!(node.properties.get("body_prefix").is_none());
    }

    #[test]
    fn method_records_optional_doc_and_body_prefix() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("widget.dart"));
        let mut class = ClassEntity::new("Counter", 1, 20);
        class.methods.push(
            FunctionEntity::new("increment", 5, 10)
                .with_doc("bump the value")
                .with_body_prefix("value++;"),
        );
        ir.add_class(class);

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.functions[0]).unwrap();
        assert_eq!(
            node.properties.get("doc"),
            Some(&PropertyValue::String("bump the value".to_string()))
        );
        assert_eq!(
            node.properties.get("body_prefix"),
            Some(&PropertyValue::String("value++;".to_string()))
        );
    }

    #[test]
    fn trait_records_optional_doc_prop() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("widget.dart"));
        ir.add_trait(TraitEntity::new("Drawable", 1, 5).with_doc("can draw"));

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.traits[0]).unwrap();
        assert_eq!(
            node.properties.get("doc"),
            Some(&PropertyValue::String("can draw".to_string()))
        );
    }

    #[test]
    fn bare_import_creates_external_module_with_empty_edge_props() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("widget.dart"));
        ir.add_import(ImportRelation::new("widget", "dart:async"));

        let (graph, info) = build(&ir);
        let import_node = graph.get_node(info.imports[0]).unwrap();
        assert_eq!(import_node.node_type, NodeType::Module);
        assert_eq!(
            import_node.properties.get("is_external"),
            Some(&PropertyValue::String("true".to_string()))
        );

        let edge_ids = graph
            .get_edges_between(info.file_id, info.imports[0])
            .unwrap();
        let edge = graph.get_edge(edge_ids[0]).unwrap();
        assert!(edge.properties.get("alias").is_none());
        assert!(edge.properties.get("is_wildcard").is_none());
        assert!(edge.properties.get("symbols").is_none());
    }

    #[test]
    fn import_with_only_symbols_records_symbols_edge_prop() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("widget.dart"));
        ir.add_import(
            ImportRelation::new("widget", "dart:math")
                .with_symbols(vec!["pi".to_string(), "sqrt".to_string()]),
        );

        let (graph, info) = build(&ir);
        let edge_ids = graph
            .get_edges_between(info.file_id, info.imports[0])
            .unwrap();
        let edge = graph.get_edge(edge_ids[0]).unwrap();
        assert_eq!(
            edge.properties.get("symbols"),
            Some(&PropertyValue::StringList(vec![
                "pi".to_string(),
                "sqrt".to_string()
            ]))
        );
        assert!(edge.properties.get("alias").is_none());
        assert!(edge.properties.get("is_wildcard").is_none());
    }

    #[test]
    fn import_matching_in_file_node_reuses_it_without_external_flag() {
        // When the imported name matches an already-mapped in-file node (here a
        // class), the mapper reuses that node instead of creating an external
        // Module - so no is_external flag is stamped.
        let mut ir = CodeIR::new(std::path::PathBuf::from("widget.dart"));
        ir.add_class(ClassEntity::new("Helper", 1, 5));
        ir.add_import(ImportRelation::new("widget", "Helper"));

        let (graph, info) = build(&ir);
        let import_id = info.imports[0];
        assert_eq!(import_id, info.classes[0]);
        let node = graph.get_node(import_id).unwrap();
        assert_eq!(node.node_type, NodeType::Class);
        assert!(node.properties.get("is_external").is_none());

        let import_edges: Vec<_> = graph
            .get_edges_between(info.file_id, import_id)
            .unwrap()
            .into_iter()
            .filter(|&e| graph.get_edge(e).unwrap().edge_type == EdgeType::Imports)
            .collect();
        assert_eq!(import_edges.len(), 1);
    }

    #[test]
    fn call_edge_records_is_direct_flag() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("widget.dart"));
        ir.add_function(FunctionEntity::new("caller", 1, 5));
        ir.add_function(FunctionEntity::new("direct", 6, 8));
        ir.add_function(FunctionEntity::new("dynamic", 9, 11));
        ir.add_call(CallRelation::new("caller", "direct", 2));
        ir.add_call(CallRelation::new("caller", "dynamic", 3).indirect());

        let (graph, info) = build(&ir);
        let id_of = |name: &str| -> NodeId {
            info.functions
                .iter()
                .copied()
                .find(|&id| name_of(&graph, id) == name)
                .unwrap()
        };
        let caller = id_of("caller");

        let is_direct_between = |callee: NodeId| -> PropertyValue {
            let e = graph
                .get_edges_between(caller, callee)
                .unwrap()
                .into_iter()
                .find(|&e| graph.get_edge(e).unwrap().edge_type == EdgeType::Calls)
                .unwrap();
            graph
                .get_edge(e)
                .unwrap()
                .properties
                .get("is_direct")
                .unwrap()
                .clone()
        };
        assert_eq!(
            is_direct_between(id_of("direct")),
            PropertyValue::Bool(true)
        );
        assert_eq!(
            is_direct_between(id_of("dynamic")),
            PropertyValue::Bool(false)
        );
    }
}
