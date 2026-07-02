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

    // -----------------------------------------------------------------------
    // File / module node
    // -----------------------------------------------------------------------
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
            .with("language", "erlang");

        let id = graph
            .add_node(NodeType::CodeFile, props)
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
        node_map.insert(name, id);
        id
    };

    // -----------------------------------------------------------------------
    // Functions
    // -----------------------------------------------------------------------
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

    // -----------------------------------------------------------------------
    // Records (ClassEntity → Struct node)
    // -----------------------------------------------------------------------
    for class in &ir.classes {
        let mut props = PropertyMap::new()
            .with("name", class.name.clone())
            .with("path", file_path.display().to_string())
            .with("visibility", class.visibility.clone())
            .with("line_start", class.line_start as i64)
            .with("line_end", class.line_end as i64);

        if !class.attributes.is_empty() {
            props = props.with("attributes", class.attributes.clone());
        }
        if let Some(ref doc) = class.doc_comment {
            props = props.with("doc", doc.clone());
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

    // -----------------------------------------------------------------------
    // Behaviours (TraitEntity → Trait node)
    // -----------------------------------------------------------------------
    for trait_entity in &ir.traits {
        let props = PropertyMap::new()
            .with("name", trait_entity.name.clone())
            .with("line_start", trait_entity.line_start as i64)
            .with("line_end", trait_entity.line_end as i64);

        let trait_id = if let Some(&existing) = node_map.get(&trait_entity.name) {
            existing
        } else {
            let id = graph
                .add_node(NodeType::Interface, props)
                .map_err(|e| ParserError::GraphError(e.to_string()))?;
            node_map.insert(trait_entity.name.clone(), id);
            id
        };

        trait_ids.push(trait_id);

        // The module implements (uses) this behaviour
        graph
            .add_edge(file_id, trait_id, EdgeType::Implements, PropertyMap::new())
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
    }

    // -----------------------------------------------------------------------
    // Imports
    // -----------------------------------------------------------------------
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

        graph
            .add_edge(file_id, import_id, EdgeType::Imports, PropertyMap::new())
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
    }

    // -----------------------------------------------------------------------
    // Calls
    // -----------------------------------------------------------------------
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
        let info = ir_to_graph(ir, &mut graph, Path::new("service.erl")).unwrap();
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
        let ir = CodeIR::new(std::path::PathBuf::from("service.erl"));
        let (graph, info) = build(&ir);

        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(
            file.properties.get("name"),
            Some(&PropertyValue::String("service".to_string()))
        );
        assert_eq!(
            file.properties.get("language"),
            Some(&PropertyValue::String("erlang".to_string()))
        );
        assert!(info.functions.is_empty());
        assert!(info.classes.is_empty());
        assert!(info.traits.is_empty());
        assert!(info.imports.is_empty());
        assert_eq!(info.line_count, 0);
    }

    #[test]
    fn module_drives_file_node_metadata_and_line_count() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("service.erl"));
        let mut module = ModuleEntity::new("service", "src/service.erl", "erlang");
        module.line_count = 120;
        module.doc_comment = Some("gen_server".to_string());
        ir.set_module(module);

        let (graph, info) = build(&ir);
        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(
            file.properties.get("name"),
            Some(&PropertyValue::String("service".to_string()))
        );
        assert_eq!(
            file.properties.get("path"),
            Some(&PropertyValue::String("src/service.erl".to_string()))
        );
        assert_eq!(
            file.properties.get("line_count"),
            Some(&PropertyValue::Int(120))
        );
        assert_eq!(
            file.properties.get("doc"),
            Some(&PropertyValue::String("gen_server".to_string()))
        );
        assert_eq!(info.line_count, 120);
    }

    #[test]
    fn free_function_is_contained_by_file_with_complexity_and_flag_props() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("service.erl"));
        let metrics = ComplexityMetrics {
            cyclomatic_complexity: 12,
            branches: 6,
            loops: 2,
            ..Default::default()
        };
        let func = FunctionEntity::new("handle_call", 1, 30)
            .with_signature("handle_call/3")
            .with_complexity(metrics);
        ir.add_function(func);

        let (graph, info) = build(&ir);
        assert_eq!(info.functions.len(), 1);

        let func_id = info.functions[0];
        let neighbors = graph
            .get_neighbors(info.file_id, Direction::Outgoing)
            .unwrap();
        assert!(neighbors.contains(&func_id));

        let node = graph.get_node(func_id).unwrap();
        // Erlang does not qualify function names (no methods-in-class); bare name kept.
        assert_eq!(name_of(&graph, func_id), "handle_call");
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
    }

    #[test]
    fn record_maps_to_class_node_without_emitting_methods() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("service.erl"));
        let mut class = ClassEntity::new("state", 1, 5).with_visibility("public");
        // Erlang record processing ignores class.methods entirely.
        class.methods.push(FunctionEntity::new("ignored", 2, 3));
        ir.add_class(class);

        let (graph, info) = build(&ir);
        assert_eq!(info.classes.len(), 1);
        // No function nodes are emitted for record fields/methods.
        assert!(info.functions.is_empty());

        let class_id = info.classes[0];
        let class_node = graph.get_node(class_id).unwrap();
        assert_eq!(class_node.node_type, NodeType::Class);
        assert_eq!(name_of(&graph, class_id), "state");
        assert!(!graph
            .get_edges_between(info.file_id, class_id)
            .unwrap()
            .is_empty());
        // Only file + class nodes exist; the class's method was dropped.
        assert_eq!(graph.node_count(), 2);
    }

    #[test]
    fn behaviour_trait_creates_interface_node_with_implements_edge() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("service.erl"));
        let mut tr = TraitEntity::new("gen_server", 1, 1);
        // required_methods are not emitted as nodes by the erlang mapper.
        tr.required_methods.push(FunctionEntity::new("init", 2, 3));
        ir.add_trait(tr);

        let (graph, info) = build(&ir);
        assert_eq!(info.traits.len(), 1);
        // The behaviour's required methods are not turned into function nodes.
        assert!(info.functions.is_empty());

        let trait_id = info.traits[0];
        let trait_node = graph.get_node(trait_id).unwrap();
        assert_eq!(trait_node.node_type, NodeType::Interface);
        assert_eq!(name_of(&graph, trait_id), "gen_server");

        // The module implements (uses) the behaviour via an Implements edge.
        let edge_ids = graph.get_edges_between(info.file_id, trait_id).unwrap();
        assert_eq!(edge_ids.len(), 1);
        assert_eq!(
            graph.get_edge(edge_ids[0]).unwrap().edge_type,
            EdgeType::Implements
        );
        // Only file + interface nodes; no method node from required_methods.
        assert_eq!(graph.node_count(), 2);
    }

    #[test]
    fn import_creates_external_module_with_is_external_prop() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("service.erl"));
        ir.add_import(ImportRelation::new("service", "lists"));

        let (graph, info) = build(&ir);
        assert_eq!(info.imports.len(), 1);

        let import_id = info.imports[0];
        let import_node = graph.get_node(import_id).unwrap();
        assert_eq!(import_node.node_type, NodeType::Module);
        assert_eq!(
            import_node.properties.get("name"),
            Some(&PropertyValue::String("lists".to_string()))
        );
        assert_eq!(
            import_node.properties.get("is_external"),
            Some(&PropertyValue::String("true".to_string()))
        );

        let edge_ids = graph.get_edges_between(info.file_id, import_id).unwrap();
        assert_eq!(edge_ids.len(), 1);
        let edge = graph.get_edge(edge_ids[0]).unwrap();
        assert_eq!(edge.edge_type, EdgeType::Imports);
        // The erlang mapper records no properties on the import edge.
        assert!(edge.properties.get("symbols").is_none());
        assert!(edge.properties.get("alias").is_none());
    }

    #[test]
    fn call_relation_wires_calls_edge_only_between_known_nodes() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("service.erl"));
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
        let mut ir = CodeIR::new(std::path::PathBuf::from("service.erl"));
        ir.add_import(ImportRelation::new("service", "lists"));
        ir.add_import(ImportRelation::new("service", "lists"));

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
        let mut ir = CodeIR::new(std::path::PathBuf::from("service.erl"));
        let func = FunctionEntity::new("start", 1, 4)
            .with_doc("starts the server")
            .with_body_prefix("start() ->")
            .with_parameters(vec![Parameter::new("arg")]);
        ir.add_function(func);

        let (graph, info) = build(&ir);
        let id = info.functions[0];
        assert_eq!(
            prop(&graph, id, "doc"),
            Some(PropertyValue::String("starts the server".to_string()))
        );
        assert_eq!(
            prop(&graph, id, "body_prefix"),
            Some(PropertyValue::String("start() ->".to_string()))
        );
        assert_eq!(
            prop(&graph, id, "parameters"),
            Some(PropertyValue::StringList(vec!["arg".to_string()]))
        );
    }

    #[test]
    fn function_optional_props_absent() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("service.erl"));
        ir.add_function(FunctionEntity::new("stop", 1, 2));

        let (graph, info) = build(&ir);
        let id = info.functions[0];
        assert!(prop(&graph, id, "doc").is_none());
        assert!(prop(&graph, id, "body_prefix").is_none());
        assert!(prop(&graph, id, "parameters").is_none());
        // The erlang function loop never reads return_type or attributes.
        assert!(prop(&graph, id, "return_type").is_none());
        assert!(prop(&graph, id, "attributes").is_none());
        // No complexity supplied -> no complexity props.
        assert!(prop(&graph, id, "complexity").is_none());
        assert!(prop(&graph, id, "complexity_grade").is_none());
    }

    #[test]
    fn function_stamps_all_eight_complexity_sub_props() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("service.erl"));
        let metrics = ComplexityMetrics {
            cyclomatic_complexity: 25,
            branches: 8,
            loops: 3,
            logical_operators: 4,
            max_nesting_depth: 5,
            exception_handlers: 2,
            early_returns: 6,
        };
        ir.add_function(FunctionEntity::new("loop", 1, 40).with_complexity(metrics));

        let (graph, info) = build(&ir);
        let id = info.functions[0];
        assert_eq!(prop(&graph, id, "complexity"), Some(PropertyValue::Int(25)));
        // Cyclomatic 25 falls in the D band.
        assert_eq!(
            prop(&graph, id, "complexity_grade"),
            Some(PropertyValue::String("D".to_string()))
        );
        assert_eq!(
            prop(&graph, id, "complexity_branches"),
            Some(PropertyValue::Int(8))
        );
        assert_eq!(
            prop(&graph, id, "complexity_loops"),
            Some(PropertyValue::Int(3))
        );
        assert_eq!(
            prop(&graph, id, "complexity_logical_ops"),
            Some(PropertyValue::Int(4))
        );
        assert_eq!(
            prop(&graph, id, "complexity_nesting"),
            Some(PropertyValue::Int(5))
        );
        assert_eq!(
            prop(&graph, id, "complexity_exceptions"),
            Some(PropertyValue::Int(2))
        );
        assert_eq!(
            prop(&graph, id, "complexity_early_returns"),
            Some(PropertyValue::Int(6))
        );
    }

    #[test]
    fn function_complexity_grade_band_a() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("service.erl"));
        let metrics = ComplexityMetrics {
            cyclomatic_complexity: 3,
            ..Default::default()
        };
        ir.add_function(FunctionEntity::new("simple", 1, 2).with_complexity(metrics));

        let (graph, info) = build(&ir);
        assert_eq!(
            prop(&graph, info.functions[0], "complexity_grade"),
            Some(PropertyValue::String("A".to_string()))
        );
    }

    #[test]
    fn function_complexity_grade_band_f() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("service.erl"));
        let metrics = ComplexityMetrics {
            cyclomatic_complexity: 80,
            ..Default::default()
        };
        ir.add_function(FunctionEntity::new("monster", 1, 200).with_complexity(metrics));

        let (graph, info) = build(&ir);
        assert_eq!(
            prop(&graph, info.functions[0], "complexity_grade"),
            Some(PropertyValue::String("F".to_string()))
        );
    }

    #[test]
    fn function_boolean_flags_are_stamped() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("service.erl"));
        let func = FunctionEntity::new("spawn_worker", 1, 3)
            .async_fn()
            .static_fn()
            .abstract_fn();
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
    fn class_optional_props_present() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("service.erl"));
        let class = ClassEntity::new("state", 1, 5)
            .with_doc("server state record")
            .with_attributes(vec!["id".to_string(), "name".to_string()])
            .with_body_prefix("-record(state, {");
        ir.add_class(class);

        let (graph, info) = build(&ir);
        let id = info.classes[0];
        assert_eq!(
            prop(&graph, id, "doc"),
            Some(PropertyValue::String("server state record".to_string()))
        );
        assert_eq!(
            prop(&graph, id, "attributes"),
            Some(PropertyValue::StringList(vec![
                "id".to_string(),
                "name".to_string()
            ]))
        );
        assert_eq!(
            prop(&graph, id, "body_prefix"),
            Some(PropertyValue::String("-record(state, {".to_string()))
        );
    }

    #[test]
    fn class_optional_props_absent() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("service.erl"));
        ir.add_class(ClassEntity::new("state", 1, 5));

        let (graph, info) = build(&ir);
        let id = info.classes[0];
        assert!(prop(&graph, id, "doc").is_none());
        assert!(prop(&graph, id, "attributes").is_none());
        assert!(prop(&graph, id, "body_prefix").is_none());
        // The class loop stamps no boolean flags on record nodes.
        assert!(prop(&graph, id, "is_abstract").is_none());
        assert!(prop(&graph, id, "is_interface").is_none());
    }

    #[test]
    fn import_matching_in_file_name_reuses_node_without_is_external() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("service.erl"));
        ir.add_function(FunctionEntity::new("helper", 1, 3));
        // Import target matches an already-mapped function name -> node reused.
        ir.add_import(ImportRelation::new("service", "helper"));

        let (graph, info) = build(&ir);
        let func_id = info.functions[0];
        assert_eq!(info.imports[0], func_id);
        // The reused node keeps its Function type and carries no is_external prop.
        assert_eq!(
            graph.get_node(func_id).unwrap().node_type,
            NodeType::Function
        );
        assert!(prop(&graph, func_id, "is_external").is_none());
        // Both a Contains and an Imports edge connect the file to the reused node.
        let edges = graph.get_edges_between(info.file_id, func_id).unwrap();
        let types: Vec<_> = edges
            .iter()
            .map(|&e| graph.get_edge(e).unwrap().edge_type)
            .collect();
        assert!(types.contains(&EdgeType::Contains));
        assert!(types.contains(&EdgeType::Imports));
    }

    #[test]
    fn indirect_call_records_is_direct_false() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("service.erl"));
        ir.add_function(FunctionEntity::new("caller", 1, 5));
        ir.add_function(FunctionEntity::new("callee", 6, 10));
        let mut call = CallRelation::new("caller", "callee", 3);
        call.is_direct = false;
        ir.add_call(call);

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
    fn multiple_functions_and_classes_are_all_contained_by_file() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("service.erl"));
        ir.add_function(FunctionEntity::new("f1", 1, 2));
        ir.add_function(FunctionEntity::new("f2", 3, 4));
        ir.add_class(ClassEntity::new("state", 5, 6));
        ir.add_class(ClassEntity::new("config", 7, 8));

        let (graph, info) = build(&ir);
        assert_eq!(info.functions.len(), 2);
        assert_eq!(info.classes.len(), 2);

        let neighbors = graph
            .get_neighbors(info.file_id, Direction::Outgoing)
            .unwrap();
        for id in info.functions.iter().chain(info.classes.iter()) {
            assert!(neighbors.contains(id));
        }
        // file + 2 functions + 2 classes.
        assert_eq!(graph.node_count(), 5);
    }

    #[test]
    fn duplicate_behaviour_reuses_interface_node() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("service.erl"));
        ir.add_trait(TraitEntity::new("gen_server", 1, 1));
        ir.add_trait(TraitEntity::new("gen_server", 1, 1));

        let (graph, info) = build(&ir);
        assert_eq!(info.traits.len(), 2);
        // The second behaviour with the same name reuses the first interface node.
        assert_eq!(info.traits[0], info.traits[1]);
        // file + single interface node.
        assert_eq!(graph.node_count(), 2);
        // Two Implements edges from the file to the shared interface.
        let edges = graph
            .get_edges_between(info.file_id, info.traits[0])
            .unwrap();
        assert_eq!(edges.len(), 2);
    }
}
