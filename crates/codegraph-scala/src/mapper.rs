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
            .with("language", "scala");

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
        let info = ir_to_graph(ir, &mut graph, Path::new("Service.scala")).unwrap();
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
        let ir = CodeIR::new(std::path::PathBuf::from("Service.scala"));
        let (graph, info) = build(&ir);

        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(
            file.properties.get("name"),
            Some(&PropertyValue::String("Service".to_string()))
        );
        assert_eq!(
            file.properties.get("language"),
            Some(&PropertyValue::String("scala".to_string()))
        );
        assert!(info.functions.is_empty());
        assert!(info.classes.is_empty());
        assert!(info.traits.is_empty());
        assert!(info.imports.is_empty());
        assert_eq!(info.line_count, 0);
    }

    #[test]
    fn module_drives_file_node_metadata_and_line_count() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.scala"));
        let mut module = ModuleEntity::new("my.service", "src/Service.scala", "scala");
        module.line_count = 120;
        module.doc_comment = Some("package docs".to_string());
        ir.set_module(module);

        let (graph, info) = build(&ir);
        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(
            file.properties.get("name"),
            Some(&PropertyValue::String("my.service".to_string()))
        );
        assert_eq!(
            file.properties.get("path"),
            Some(&PropertyValue::String("src/Service.scala".to_string()))
        );
        assert_eq!(
            file.properties.get("line_count"),
            Some(&PropertyValue::Int(120))
        );
        assert_eq!(
            file.properties.get("doc"),
            Some(&PropertyValue::String("package docs".to_string()))
        );
        assert_eq!(info.line_count, 120);
    }

    #[test]
    fn class_with_method_links_via_contains_edges_with_hash_name() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.scala"));
        let mut class = ClassEntity::new("Repo", 1, 20)
            .with_visibility("public")
            .abstract_class();
        class
            .methods
            .push(FunctionEntity::new("save", 5, 10).with_visibility("public"));
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

        // Scala qualifies method names as Class#method (hash separator).
        let method_id = info.functions[0];
        assert_eq!(name_of(&graph, method_id), "Repo#save");
        let method_node = graph.get_node(method_id).unwrap();
        assert_eq!(method_node.node_type, NodeType::Function);
        assert_eq!(
            method_node.properties.get("parent_class"),
            Some(&PropertyValue::String("Repo".to_string()))
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
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.scala"));
        let mut tr = TraitEntity::new("Ordered", 1, 8);
        tr.required_methods
            .push(FunctionEntity::new("compare", 2, 3));
        ir.add_trait(tr);

        let (graph, info) = build(&ir);
        assert_eq!(info.traits.len(), 1);
        // The scala mapper does not emit interface method nodes.
        assert!(info.functions.is_empty());

        let iface_node = graph.get_node(info.traits[0]).unwrap();
        assert_eq!(iface_node.node_type, NodeType::Interface);
        assert_eq!(
            iface_node.properties.get("name"),
            Some(&PropertyValue::String("Ordered".to_string()))
        );
        assert!(!graph
            .get_edges_between(info.file_id, info.traits[0])
            .unwrap()
            .is_empty());
    }

    #[test]
    fn free_function_is_contained_by_file_with_complexity_and_flag_props() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.scala"));
        let metrics = ComplexityMetrics {
            cyclomatic_complexity: 12,
            branches: 6,
            loops: 2,
            ..Default::default()
        };
        let func = FunctionEntity::new("run", 1, 30)
            .with_signature("def run(): Unit")
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
    fn import_creates_external_module_and_records_symbols_edge_prop() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.scala"));
        ir.add_import(
            ImportRelation::new("Service", "scala.collection.mutable")
                .with_symbols(vec!["Map".to_string(), "Set".to_string()]),
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
        // The scala mapper records only `symbols` on the import edge (no alias/wildcard).
        assert_eq!(
            edge.properties.get("symbols"),
            Some(&PropertyValue::StringList(vec![
                "Map".to_string(),
                "Set".to_string()
            ]))
        );
    }

    #[test]
    fn call_relation_wires_calls_edge_only_between_known_nodes() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.scala"));
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
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.scala"));
        ir.add_import(ImportRelation::new("Service", "scala.util"));
        ir.add_import(ImportRelation::new("Service", "scala.util"));

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
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.scala"));
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
    fn free_function_records_all_optional_props_and_flags() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.scala"));
        let func = FunctionEntity::new("compute", 4, 9)
            .with_signature("def compute(x: Int): Int")
            .with_visibility("private")
            .with_doc("computes a value")
            .with_return_type("Int")
            .with_body_prefix("val y = x + 1")
            .with_parameters(vec![Parameter::new("x").with_type("Int")])
            .static_fn()
            .abstract_fn();
        ir.add_function(func);

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.functions[0]).unwrap();
        assert_eq!(
            node.properties.get("doc"),
            Some(&PropertyValue::String("computes a value".to_string()))
        );
        assert_eq!(
            node.properties.get("return_type"),
            Some(&PropertyValue::String("Int".to_string()))
        );
        assert_eq!(
            node.properties.get("visibility"),
            Some(&PropertyValue::String("private".to_string()))
        );
        assert_eq!(
            node.properties.get("body_prefix"),
            Some(&PropertyValue::String("val y = x + 1".to_string()))
        );
        assert_eq!(
            node.properties.get("parameters"),
            Some(&PropertyValue::StringList(vec!["x".to_string()]))
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
    fn free_function_omits_optional_props_when_absent() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.scala"));
        ir.add_function(FunctionEntity::new("bare", 1, 2));

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
    fn free_function_records_all_eight_complexity_sub_props() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.scala"));
        let metrics = ComplexityMetrics {
            cyclomatic_complexity: 25,
            branches: 7,
            loops: 3,
            logical_operators: 4,
            max_nesting_depth: 5,
            exception_handlers: 2,
            early_returns: 6,
        };
        ir.add_function(FunctionEntity::new("big", 1, 40).with_complexity(metrics));

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.functions[0]).unwrap();
        assert_eq!(
            node.properties.get("complexity"),
            Some(&PropertyValue::Int(25))
        );
        // Cyclomatic 25 falls in the D band.
        assert_eq!(
            node.properties.get("complexity_grade"),
            Some(&PropertyValue::String("D".to_string()))
        );
        assert_eq!(
            node.properties.get("complexity_branches"),
            Some(&PropertyValue::Int(7))
        );
        assert_eq!(
            node.properties.get("complexity_loops"),
            Some(&PropertyValue::Int(3))
        );
        assert_eq!(
            node.properties.get("complexity_logical_ops"),
            Some(&PropertyValue::Int(4))
        );
        assert_eq!(
            node.properties.get("complexity_nesting"),
            Some(&PropertyValue::Int(5))
        );
        assert_eq!(
            node.properties.get("complexity_exceptions"),
            Some(&PropertyValue::Int(2))
        );
        assert_eq!(
            node.properties.get("complexity_early_returns"),
            Some(&PropertyValue::Int(6))
        );
    }

    #[test]
    fn free_function_with_unknown_parent_class_gets_no_contains_edge() {
        // Functions are mapped before classes, so a parent_class that names a
        // class is never in node_map yet: neither the class branch nor the
        // file-fallback else runs, leaving the function orphaned.
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.scala"));
        ir.add_function(FunctionEntity::new("orphan", 3, 8).with_parent_class("Repo"));
        ir.add_class(ClassEntity::new("Repo", 1, 20));

        let (graph, info) = build(&ir);
        let func_id = info
            .functions
            .iter()
            .copied()
            .find(|&id| name_of(&graph, id) == "orphan")
            .unwrap();
        assert!(graph
            .get_edges_between(info.file_id, func_id)
            .unwrap()
            .is_empty());
        let class_id = info.classes[0];
        assert!(graph
            .get_edges_between(class_id, func_id)
            .unwrap()
            .is_empty());
        // The parent_class prop is still stamped on the node.
        let node = graph.get_node(func_id).unwrap();
        assert_eq!(
            node.properties.get("parent_class"),
            Some(&PropertyValue::String("Repo".to_string()))
        );
    }

    #[test]
    fn class_records_doc_attributes_and_body_prefix() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.scala"));
        let class = ClassEntity::new("Repo", 1, 20)
            .with_doc("a repository")
            .with_attributes(vec!["@deprecated".to_string()])
            .with_body_prefix("val db = ...");
        ir.add_class(class);

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.classes[0]).unwrap();
        assert_eq!(
            node.properties.get("doc"),
            Some(&PropertyValue::String("a repository".to_string()))
        );
        assert_eq!(
            node.properties.get("attributes"),
            Some(&PropertyValue::StringList(vec!["@deprecated".to_string()]))
        );
        assert_eq!(
            node.properties.get("body_prefix"),
            Some(&PropertyValue::String("val db = ...".to_string()))
        );
    }

    #[test]
    fn class_omits_optional_props_when_absent() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.scala"));
        ir.add_class(ClassEntity::new("Repo", 1, 5));

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.classes[0]).unwrap();
        assert!(node.properties.get("doc").is_none());
        assert!(node.properties.get("attributes").is_none());
        assert!(node.properties.get("body_prefix").is_none());
    }

    #[test]
    fn class_method_records_doc_body_prefix_and_method_props() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.scala"));
        let mut class = ClassEntity::new("Repo", 1, 20);
        class.methods.push(
            FunctionEntity::new("save", 5, 10)
                .with_signature("def save(): Unit")
                .with_visibility("protected")
                .with_doc("persists")
                .with_body_prefix("db.write()"),
        );
        ir.add_class(class);

        let (graph, info) = build(&ir);
        let method_id = info.functions[0];
        let node = graph.get_node(method_id).unwrap();
        assert_eq!(
            node.properties.get("is_method"),
            Some(&PropertyValue::String("true".to_string()))
        );
        assert_eq!(
            node.properties.get("parent_class"),
            Some(&PropertyValue::String("Repo".to_string()))
        );
        assert_eq!(
            node.properties.get("signature"),
            Some(&PropertyValue::String("def save(): Unit".to_string()))
        );
        assert_eq!(
            node.properties.get("visibility"),
            Some(&PropertyValue::String("protected".to_string()))
        );
        assert_eq!(
            node.properties.get("doc"),
            Some(&PropertyValue::String("persists".to_string()))
        );
        assert_eq!(
            node.properties.get("body_prefix"),
            Some(&PropertyValue::String("db.write()".to_string()))
        );
    }

    #[test]
    fn trait_records_doc_when_present_and_omits_when_absent() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.scala"));
        ir.add_trait(TraitEntity::new("Named", 1, 3).with_doc("has a name"));
        ir.add_trait(TraitEntity::new("Bare", 4, 6));

        let (graph, info) = build(&ir);
        let named = info
            .traits
            .iter()
            .copied()
            .find(|&id| name_of(&graph, id) == "Named")
            .unwrap();
        let bare = info
            .traits
            .iter()
            .copied()
            .find(|&id| name_of(&graph, id) == "Bare")
            .unwrap();
        assert_eq!(
            graph.get_node(named).unwrap().properties.get("doc"),
            Some(&PropertyValue::String("has a name".to_string()))
        );
        assert!(graph
            .get_node(bare)
            .unwrap()
            .properties
            .get("doc")
            .is_none());
    }

    #[test]
    fn import_reuses_in_file_node_without_marking_external() {
        // An import whose target matches an already-mapped class reuses that
        // node instead of creating an external Module.
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.scala"));
        ir.add_class(ClassEntity::new("Repo", 1, 20));
        ir.add_import(ImportRelation::new("Service", "Repo"));

        let (graph, info) = build(&ir);
        assert_eq!(info.imports.len(), 1);
        let import_id = info.imports[0];
        // Reused class node: same id, Class type, no is_external stamp.
        assert_eq!(import_id, info.classes[0]);
        let node = graph.get_node(import_id).unwrap();
        assert_eq!(node.node_type, NodeType::Class);
        assert!(node.properties.get("is_external").is_none());

        // A fresh Imports edge is added alongside the file->class Contains edge.
        let import_edges: Vec<_> = graph
            .get_edges_between(info.file_id, import_id)
            .unwrap()
            .into_iter()
            .filter(|&e| graph.get_edge(e).unwrap().edge_type == EdgeType::Imports)
            .collect();
        assert_eq!(import_edges.len(), 1);
    }

    #[test]
    fn bare_import_records_no_symbols_edge_prop() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.scala"));
        ir.add_import(ImportRelation::new("Service", "scala.util"));

        let (graph, info) = build(&ir);
        let edge_ids = graph
            .get_edges_between(info.file_id, info.imports[0])
            .unwrap();
        assert_eq!(edge_ids.len(), 1);
        let edge = graph.get_edge(edge_ids[0]).unwrap();
        assert_eq!(edge.edge_type, EdgeType::Imports);
        assert!(edge.properties.get("symbols").is_none());
    }

    #[test]
    fn call_edge_records_is_direct_flag() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.scala"));
        ir.add_function(FunctionEntity::new("caller", 1, 5));
        ir.add_function(FunctionEntity::new("direct", 6, 10));
        ir.add_function(FunctionEntity::new("indirect", 11, 15));
        ir.add_call(CallRelation::new("caller", "direct", 3));
        ir.add_call(CallRelation::new("caller", "indirect", 4).indirect());

        let (graph, info) = build(&ir);
        let id_of = |name: &str| -> NodeId {
            info.functions
                .iter()
                .copied()
                .find(|&id| name_of(&graph, id) == name)
                .unwrap()
        };
        let caller = id_of("caller");

        let is_direct = |callee: NodeId| -> bool {
            let edge = graph
                .get_edges_between(caller, callee)
                .unwrap()
                .into_iter()
                .map(|e| graph.get_edge(e).unwrap())
                .find(|e| e.edge_type == EdgeType::Calls)
                .unwrap();
            matches!(
                edge.properties.get("is_direct"),
                Some(&PropertyValue::Bool(true))
            )
        };
        assert!(is_direct(id_of("direct")));
        assert!(!is_direct(id_of("indirect")));
    }

    #[test]
    fn free_function_records_line_signature_and_path_props() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.scala"));
        ir.add_function(FunctionEntity::new("run", 7, 42).with_signature("def run(): Unit"));

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.functions[0]).unwrap();
        assert_eq!(
            node.properties.get("line_start"),
            Some(&PropertyValue::Int(7))
        );
        assert_eq!(
            node.properties.get("line_end"),
            Some(&PropertyValue::Int(42))
        );
        assert_eq!(
            node.properties.get("signature"),
            Some(&PropertyValue::String("def run(): Unit".to_string()))
        );
        // The path prop mirrors the file path passed to ir_to_graph.
        assert_eq!(
            node.properties.get("path"),
            Some(&PropertyValue::String("Service.scala".to_string()))
        );
    }

    #[test]
    fn class_records_line_visibility_and_path_props() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.scala"));
        ir.add_class(ClassEntity::new("Repo", 3, 27).with_visibility("private"));

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.classes[0]).unwrap();
        assert_eq!(
            node.properties.get("line_start"),
            Some(&PropertyValue::Int(3))
        );
        assert_eq!(
            node.properties.get("line_end"),
            Some(&PropertyValue::Int(27))
        );
        assert_eq!(
            node.properties.get("visibility"),
            Some(&PropertyValue::String("private".to_string()))
        );
        assert_eq!(
            node.properties.get("path"),
            Some(&PropertyValue::String("Service.scala".to_string()))
        );
        // A class with no abstract flag still stamps is_abstract=false.
        assert_eq!(
            node.properties.get("is_abstract"),
            Some(&PropertyValue::Bool(false))
        );
    }

    #[test]
    fn trait_records_line_visibility_and_path_props() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.scala"));
        ir.add_trait(TraitEntity::new("Ordered", 2, 9).with_visibility("protected"));

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.traits[0]).unwrap();
        assert_eq!(
            node.properties.get("line_start"),
            Some(&PropertyValue::Int(2))
        );
        assert_eq!(
            node.properties.get("line_end"),
            Some(&PropertyValue::Int(9))
        );
        assert_eq!(
            node.properties.get("visibility"),
            Some(&PropertyValue::String("protected".to_string()))
        );
        assert_eq!(
            node.properties.get("path"),
            Some(&PropertyValue::String("Service.scala".to_string()))
        );
    }

    #[test]
    fn class_method_records_line_and_path_but_omits_function_only_props() {
        // The method loop stamps a narrower prop set than free functions: no
        // complexity, is_async/is_static/is_abstract flags, parameters, or
        // return_type are ever written onto a method node.
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.scala"));
        let metrics = ComplexityMetrics {
            cyclomatic_complexity: 9,
            branches: 4,
            ..Default::default()
        };
        let mut class = ClassEntity::new("Repo", 1, 30);
        class.methods.push(
            FunctionEntity::new("save", 12, 18)
                .with_complexity(metrics)
                .with_return_type("Unit")
                .with_parameters(vec![Parameter::new("row").with_type("Row")])
                .async_fn()
                .static_fn(),
        );
        ir.add_class(class);

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.functions[0]).unwrap();
        assert_eq!(
            node.properties.get("line_start"),
            Some(&PropertyValue::Int(12))
        );
        assert_eq!(
            node.properties.get("line_end"),
            Some(&PropertyValue::Int(18))
        );
        assert_eq!(
            node.properties.get("path"),
            Some(&PropertyValue::String("Service.scala".to_string()))
        );
        assert!(node.properties.get("complexity").is_none());
        assert!(node.properties.get("is_async").is_none());
        assert!(node.properties.get("is_static").is_none());
        assert!(node.properties.get("is_abstract").is_none());
        assert!(node.properties.get("parameters").is_none());
        assert!(node.properties.get("return_type").is_none());
    }

    #[test]
    fn inheritance_with_unknown_child_or_parent_creates_no_edge() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.scala"));
        ir.add_class(ClassEntity::new("Derived", 1, 5));
        // Parent names a class that was never mapped -> no Extends edge.
        ir.add_inheritance(InheritanceRelation::new("Derived", "Ghost"));
        // Child names an unmapped class -> also skipped.
        ir.add_inheritance(InheritanceRelation::new("Phantom", "Derived"));

        let (graph, info) = build(&ir);
        let derived = info.classes[0];
        let extends: Vec<_> = graph
            .get_neighbors(derived, Direction::Outgoing)
            .unwrap()
            .into_iter()
            .filter(|&n| {
                graph
                    .get_edges_between(derived, n)
                    .unwrap()
                    .iter()
                    .any(|&e| graph.get_edge(e).unwrap().edge_type == EdgeType::Extends)
            })
            .collect();
        assert!(extends.is_empty());
    }

    #[test]
    fn implementation_with_unknown_names_creates_no_edge() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.scala"));
        ir.add_class(ClassEntity::new("Repo", 1, 5));
        // Trait name is unmapped -> no Implements edge.
        ir.add_implementation(ImplementationRelation::new("Repo", "Ghost"));
        // Implementor is unmapped -> also skipped.
        ir.add_implementation(ImplementationRelation::new("Phantom", "Repo"));

        let (graph, info) = build(&ir);
        let repo = info.classes[0];
        let has_impl = graph
            .get_neighbors(repo, Direction::Outgoing)
            .unwrap()
            .into_iter()
            .any(|n| {
                graph
                    .get_edges_between(repo, n)
                    .unwrap()
                    .iter()
                    .any(|&e| graph.get_edge(e).unwrap().edge_type == EdgeType::Implements)
            });
        assert!(!has_impl);
    }

    #[test]
    fn import_reuses_trait_node_without_marking_external() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.scala"));
        ir.add_trait(TraitEntity::new("Drawable", 1, 4));
        ir.add_import(ImportRelation::new("Service", "Drawable"));

        let (graph, info) = build(&ir);
        assert_eq!(info.imports.len(), 1);
        let import_id = info.imports[0];
        assert_eq!(import_id, info.traits[0]);
        let node = graph.get_node(import_id).unwrap();
        assert_eq!(node.node_type, NodeType::Interface);
        assert!(node.properties.get("is_external").is_none());
    }

    #[test]
    fn import_reuses_function_node_without_marking_external() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.scala"));
        ir.add_function(FunctionEntity::new("helper", 1, 3));
        ir.add_import(ImportRelation::new("Service", "helper"));

        let (graph, info) = build(&ir);
        assert_eq!(info.imports.len(), 1);
        let import_id = info.imports[0];
        assert_eq!(import_id, info.functions[0]);
        let node = graph.get_node(import_id).unwrap();
        assert_eq!(node.node_type, NodeType::Function);
        assert!(node.properties.get("is_external").is_none());
    }

    #[test]
    fn multiple_free_functions_all_contained_by_file() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.scala"));
        ir.add_function(FunctionEntity::new("a", 1, 2));
        ir.add_function(FunctionEntity::new("b", 3, 4));
        ir.add_function(FunctionEntity::new("c", 5, 6));

        let (graph, info) = build(&ir);
        assert_eq!(info.functions.len(), 3);
        let contained = graph
            .get_neighbors(info.file_id, Direction::Outgoing)
            .unwrap();
        for id in &info.functions {
            assert!(contained.contains(id));
        }
    }

    #[test]
    fn import_matching_module_name_reuses_file_node() {
        // When an import targets the module name, node_map already holds the
        // file node, so the import edge is a self-loop on the CodeFile node.
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.scala"));
        let module = ModuleEntity::new("my.pkg", "src/Service.scala", "scala");
        ir.set_module(module);
        ir.add_import(ImportRelation::new("Service", "my.pkg"));

        let (graph, info) = build(&ir);
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0], info.file_id);
        let node = graph.get_node(info.imports[0]).unwrap();
        assert_eq!(node.node_type, NodeType::CodeFile);
        assert!(node.properties.get("is_external").is_none());
    }
}
