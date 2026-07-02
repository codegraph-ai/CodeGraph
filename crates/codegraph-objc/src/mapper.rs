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
            .with("language", "objc");

        let id = graph
            .add_node(NodeType::CodeFile, props)
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
        node_map.insert(name, id);
        id
    };

    // Add classes (@interface)
    for class in &ir.classes {
        let mut props = PropertyMap::new()
            .with("name", class.name.clone())
            .with("path", file_path.display().to_string())
            .with("visibility", class.visibility.clone())
            .with("line_start", class.line_start as i64)
            .with("line_end", class.line_end as i64)
            .with("is_abstract", class.is_abstract)
            .with("is_interface", class.is_interface);

        if !class.base_classes.is_empty() {
            props = props.with("superclass", class.base_classes[0].clone());
        }
        if let Some(ref doc) = class.doc_comment {
            props = props.with("doc", doc.clone());
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

    // Add protocols (@protocol → Interface node)
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

    // Add functions/methods
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
        if let Some(ref parent) = func.parent_class {
            props = props.with("parent_class", parent.clone());
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

        // Determine parent container (class/protocol or file)
        let parent_id = func
            .parent_class
            .as_ref()
            .and_then(|pc| node_map.get(pc))
            .copied()
            .unwrap_or(file_id);

        // Unique key: parent::name to avoid collision when same method name exists
        // in multiple classes
        let key = if let Some(ref pc) = func.parent_class {
            format!("{}::{}", pc, func.name)
        } else {
            func.name.clone()
        };
        node_map.insert(key, func_id);
        function_ids.push(func_id);

        graph
            .add_edge(parent_id, func_id, EdgeType::Contains, PropertyMap::new())
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

        graph
            .add_edge(file_id, import_id, EdgeType::Imports, PropertyMap::new())
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
    }

    // Add call edges
    for call in &ir.calls {
        let caller_id = node_map.get(&call.caller).copied();
        let callee_id = node_map.get(&call.callee).copied();

        if let (Some(caller_id), Some(callee_id)) = (caller_id, callee_id) {
            let edge_props = PropertyMap::new()
                .with("call_site_line", call.call_site_line as i64)
                .with("is_direct", call.is_direct);

            graph
                .add_edge(caller_id, callee_id, EdgeType::Calls, edge_props)
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
        CallRelation, ClassEntity, ComplexityMetrics, FunctionEntity, ImportRelation, ModuleEntity,
        Parameter, TraitEntity,
    };

    fn build(ir: &CodeIR) -> (CodeGraph, FileInfo) {
        let mut graph = CodeGraph::in_memory().unwrap();
        let info = ir_to_graph(ir, &mut graph, Path::new("Widget.m")).unwrap();
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
        let ir = CodeIR::new(std::path::PathBuf::from("Widget.m"));
        let (graph, info) = build(&ir);

        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(
            file.properties.get("name"),
            Some(&PropertyValue::String("Widget".to_string()))
        );
        assert_eq!(
            file.properties.get("language"),
            Some(&PropertyValue::String("objc".to_string()))
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
        let mut ir = CodeIR::new(std::path::PathBuf::from("Widget.m"));
        let mut module = ModuleEntity::new("Widget", "src/Widget.m", "objc");
        module.line_count = 140;
        module.doc_comment = Some("widget impl".to_string());
        ir.set_module(module);

        let (graph, info) = build(&ir);
        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(
            file.properties.get("name"),
            Some(&PropertyValue::String("Widget".to_string()))
        );
        assert_eq!(
            file.properties.get("path"),
            Some(&PropertyValue::String("src/Widget.m".to_string()))
        );
        assert_eq!(
            file.properties.get("line_count"),
            Some(&PropertyValue::Int(140))
        );
        assert_eq!(
            file.properties.get("doc"),
            Some(&PropertyValue::String("widget impl".to_string()))
        );
        assert_eq!(info.line_count, 140);
    }

    #[test]
    fn interface_class_is_contained_by_file_with_superclass_but_no_methods() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Widget.m"));
        let mut class = ClassEntity::new("Widget", 1, 20)
            .with_visibility("public")
            .with_bases(vec!["NSObject".to_string()]);
        // Objective-C @interface methods that the mapper must NOT emit as nodes.
        class
            .methods
            .push(FunctionEntity::new("render", 2, 4).with_visibility("public"));
        ir.add_class(class);

        let (graph, info) = build(&ir);
        assert_eq!(info.classes.len(), 1);
        // The objc mapper never iterates class.methods, so no Function node exists.
        assert!(info.functions.is_empty());
        assert_eq!(graph.node_count(), 2);

        let class_id = info.classes[0];
        let node = graph.get_node(class_id).unwrap();
        assert_eq!(node.node_type, NodeType::Class);
        // base_classes[0] is recorded as the "superclass" property.
        assert_eq!(
            node.properties.get("superclass"),
            Some(&PropertyValue::String("NSObject".to_string()))
        );
        assert_eq!(
            node.properties.get("is_interface"),
            Some(&PropertyValue::Bool(false))
        );

        let neighbors = graph
            .get_neighbors(info.file_id, Direction::Outgoing)
            .unwrap();
        assert!(neighbors.contains(&class_id));
    }

    #[test]
    fn protocol_creates_interface_node_contained_by_file() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Widget.m"));
        ir.add_trait(TraitEntity::new("Drawable", 1, 5).with_visibility("public"));

        let (graph, info) = build(&ir);
        // Unlike the minimal mappers, objc maps @protocol -> Interface node.
        assert_eq!(info.traits.len(), 1);
        assert_eq!(graph.node_count(), 2);

        let trait_id = info.traits[0];
        let node = graph.get_node(trait_id).unwrap();
        assert_eq!(node.node_type, NodeType::Interface);
        assert_eq!(
            node.properties.get("name"),
            Some(&PropertyValue::String("Drawable".to_string()))
        );

        let neighbors = graph
            .get_neighbors(info.file_id, Direction::Outgoing)
            .unwrap();
        assert!(neighbors.contains(&trait_id));
    }

    #[test]
    fn free_function_is_contained_by_file_with_complexity_and_flag_props() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Widget.m"));
        let metrics = ComplexityMetrics {
            cyclomatic_complexity: 12,
            branches: 6,
            loops: 2,
            ..Default::default()
        };
        let func = FunctionEntity::new("compute_area", 1, 30)
            .with_signature("int compute_area(void)")
            .with_complexity(metrics);
        ir.add_function(func);

        let (graph, info) = build(&ir);
        assert_eq!(info.functions.len(), 1);

        let func_id = info.functions[0];
        // A function with no parent_class keeps its bare name as node_map key.
        assert_eq!(name_of(&graph, func_id), "compute_area");
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
    fn method_is_contained_by_its_parent_class_not_the_file() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Widget.m"));
        ir.add_class(ClassEntity::new("Widget", 1, 40).with_visibility("public"));
        let method = FunctionEntity::new("render", 5, 10)
            .with_signature("- (void)render")
            .with_parent_class("Widget");
        ir.add_function(method);

        let (graph, info) = build(&ir);
        assert_eq!(info.classes.len(), 1);
        assert_eq!(info.functions.len(), 1);

        let class_id = info.classes[0];
        let func_id = info.functions[0];
        // The method is a child of the class node, not of the file node.
        let class_children = graph.get_neighbors(class_id, Direction::Outgoing).unwrap();
        assert!(class_children.contains(&func_id));

        let node = graph.get_node(func_id).unwrap();
        assert_eq!(
            node.properties.get("parent_class"),
            Some(&PropertyValue::String("Widget".to_string()))
        );
    }

    #[test]
    fn import_creates_external_module_with_empty_edge_props() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Widget.m"));
        ir.add_import(
            ImportRelation::new("Widget", "Foundation").with_symbols(vec!["NSString".to_string()]),
        );

        let (graph, info) = build(&ir);
        assert_eq!(info.imports.len(), 1);

        let import_id = info.imports[0];
        let import_node = graph.get_node(import_id).unwrap();
        assert_eq!(import_node.node_type, NodeType::Module);
        assert_eq!(
            import_node.properties.get("name"),
            Some(&PropertyValue::String("Foundation".to_string()))
        );
        assert_eq!(
            import_node.properties.get("is_external"),
            Some(&PropertyValue::String("true".to_string()))
        );

        let edge_ids = graph.get_edges_between(info.file_id, import_id).unwrap();
        assert_eq!(edge_ids.len(), 1);
        let edge = graph.get_edge(edge_ids[0]).unwrap();
        assert_eq!(edge.edge_type, EdgeType::Imports);
        // The objc mapper records NO props on the Imports edge (symbols dropped).
        assert_eq!(edge.properties.get("symbols"), None);
        assert_eq!(edge.properties.get("alias"), None);
        assert_eq!(edge.properties.get("is_wildcard"), None);
    }

    #[test]
    fn call_relation_wires_calls_edge_only_between_known_nodes() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Widget.m"));
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
        let mut ir = CodeIR::new(std::path::PathBuf::from("Widget.m"));
        ir.add_import(ImportRelation::new("Widget", "UIKit"));
        ir.add_import(ImportRelation::new("Widget", "UIKit"));

        let (graph, info) = build(&ir);
        assert_eq!(info.imports.len(), 2);
        assert_eq!(info.imports[0], info.imports[1]);
        let edges = graph
            .get_edges_between(info.file_id, info.imports[0])
            .unwrap();
        assert_eq!(edges.len(), 2);
    }

    #[test]
    fn class_optional_doc_and_flags_are_stamped_when_present() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Widget.m"));
        let mut class = ClassEntity::new("Shape", 3, 30)
            .with_visibility("private")
            .with_doc("a shape");
        class.is_abstract = true;
        class.is_interface = true;
        ir.add_class(class);

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.classes[0]).unwrap();
        assert_eq!(
            node.properties.get("doc"),
            Some(&PropertyValue::String("a shape".to_string()))
        );
        assert_eq!(
            node.properties.get("visibility"),
            Some(&PropertyValue::String("private".to_string()))
        );
        assert_eq!(
            node.properties.get("is_abstract"),
            Some(&PropertyValue::Bool(true))
        );
        assert_eq!(
            node.properties.get("is_interface"),
            Some(&PropertyValue::Bool(true))
        );
        assert_eq!(
            node.properties.get("line_start"),
            Some(&PropertyValue::Int(3))
        );
        assert_eq!(
            node.properties.get("line_end"),
            Some(&PropertyValue::Int(30))
        );
    }

    #[test]
    fn class_without_bases_or_doc_omits_superclass_and_doc_props() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Widget.m"));
        ir.add_class(ClassEntity::new("Standalone", 1, 10).with_visibility("public"));

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.classes[0]).unwrap();
        // No base_classes -> no superclass prop; no doc_comment -> no doc prop.
        assert_eq!(node.properties.get("superclass"), None);
        assert_eq!(node.properties.get("doc"), None);
    }

    #[test]
    fn protocol_doc_is_stamped_when_present_and_omitted_when_absent() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Widget.m"));
        ir.add_trait(TraitEntity::new("Drawable", 1, 5).with_doc("can draw"));
        ir.add_trait(TraitEntity::new("Sizable", 6, 9));

        let (graph, info) = build(&ir);
        assert_eq!(info.traits.len(), 2);

        let with_doc = graph.get_node(info.traits[0]).unwrap();
        assert_eq!(
            with_doc.properties.get("doc"),
            Some(&PropertyValue::String("can draw".to_string()))
        );
        let without_doc = graph.get_node(info.traits[1]).unwrap();
        assert_eq!(without_doc.properties.get("doc"), None);
    }

    #[test]
    fn function_optional_props_are_stamped_when_present() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Widget.m"));
        let func = FunctionEntity::new("draw", 1, 8)
            .with_doc("draws it")
            .with_body_prefix("{ paint(); }")
            .with_parameters(vec![Parameter::new("ctx"), Parameter::new("rect")]);
        ir.add_function(func);

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.functions[0]).unwrap();
        assert_eq!(
            node.properties.get("doc"),
            Some(&PropertyValue::String("draws it".to_string()))
        );
        assert_eq!(
            node.properties.get("body_prefix"),
            Some(&PropertyValue::String("{ paint(); }".to_string()))
        );
        assert_eq!(
            node.properties.get("parameters"),
            Some(&PropertyValue::StringList(vec![
                "ctx".to_string(),
                "rect".to_string()
            ]))
        );
    }

    #[test]
    fn function_optional_props_are_omitted_when_absent() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Widget.m"));
        ir.add_function(FunctionEntity::new("bare", 1, 3));

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.functions[0]).unwrap();
        assert_eq!(node.properties.get("doc"), None);
        assert_eq!(node.properties.get("body_prefix"), None);
        assert_eq!(node.properties.get("parameters"), None);
        assert_eq!(node.properties.get("complexity"), None);
    }

    #[test]
    fn function_static_and_abstract_flags_are_stamped() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Widget.m"));
        let mut func = FunctionEntity::new("factory", 1, 5);
        func.is_static = true;
        func.is_async = true;
        func.is_abstract = true;
        ir.add_function(func);

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.functions[0]).unwrap();
        assert_eq!(
            node.properties.get("is_static"),
            Some(&PropertyValue::Bool(true))
        );
        assert_eq!(
            node.properties.get("is_async"),
            Some(&PropertyValue::Bool(true))
        );
        assert_eq!(
            node.properties.get("is_abstract"),
            Some(&PropertyValue::Bool(true))
        );
    }

    #[test]
    fn all_eight_complexity_sub_props_are_recorded() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Widget.m"));
        let metrics = ComplexityMetrics {
            cyclomatic_complexity: 25,
            branches: 7,
            loops: 3,
            logical_operators: 4,
            max_nesting_depth: 5,
            exception_handlers: 2,
            early_returns: 6,
        };
        ir.add_function(FunctionEntity::new("heavy", 1, 40).with_complexity(metrics));

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.functions[0]).unwrap();
        assert_eq!(
            node.properties.get("complexity"),
            Some(&PropertyValue::Int(25))
        );
        // 25 falls in the D band.
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
    fn complexity_grade_bands_a_and_f() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Widget.m"));
        ir.add_function(
            FunctionEntity::new("simple", 1, 3).with_complexity(ComplexityMetrics {
                cyclomatic_complexity: 3,
                ..Default::default()
            }),
        );
        ir.add_function(FunctionEntity::new("monster", 4, 200).with_complexity(
            ComplexityMetrics {
                cyclomatic_complexity: 80,
                ..Default::default()
            },
        ));

        let (graph, info) = build(&ir);
        let a = graph.get_node(info.functions[0]).unwrap();
        assert_eq!(
            a.properties.get("complexity_grade"),
            Some(&PropertyValue::String("A".to_string()))
        );
        let f = graph.get_node(info.functions[1]).unwrap();
        assert_eq!(
            f.properties.get("complexity_grade"),
            Some(&PropertyValue::String("F".to_string()))
        );
    }

    #[test]
    fn import_reuses_in_file_node_without_marking_external() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Widget.m"));
        ir.add_class(ClassEntity::new("Widget", 1, 20).with_visibility("public"));
        // An import whose target name matches an already-mapped class reuses that node.
        ir.add_import(ImportRelation::new("Widget", "Widget"));

        let (graph, info) = build(&ir);
        assert_eq!(info.imports.len(), 1);
        // The import reuses the class node rather than creating an external Module.
        assert_eq!(info.imports[0], info.classes[0]);
        let node = graph.get_node(info.imports[0]).unwrap();
        assert_eq!(node.node_type, NodeType::Class);
        assert_eq!(node.properties.get("is_external"), None);

        // Both a Contains and an Imports edge now connect the file to that node.
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
    fn call_edge_records_is_direct_flag() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Widget.m"));
        ir.add_function(FunctionEntity::new("caller", 1, 5));
        ir.add_function(FunctionEntity::new("target", 6, 10));
        ir.add_call(CallRelation::new("caller", "target", 3).indirect());

        let (graph, info) = build(&ir);
        let caller_id = info
            .functions
            .iter()
            .copied()
            .find(|&id| name_of(&graph, id) == "caller")
            .unwrap();
        let target_id = info
            .functions
            .iter()
            .copied()
            .find(|&id| name_of(&graph, id) == "target")
            .unwrap();

        let edge_ids = graph.get_edges_between(caller_id, target_id).unwrap();
        assert_eq!(edge_ids.len(), 1);
        let edge = graph.get_edge(edge_ids[0]).unwrap();
        assert_eq!(
            edge.properties.get("is_direct"),
            Some(&PropertyValue::Bool(false))
        );
    }

    #[test]
    fn same_method_name_in_two_classes_keyed_by_parent() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Widget.m"));
        ir.add_class(ClassEntity::new("Alpha", 1, 20).with_visibility("public"));
        ir.add_class(ClassEntity::new("Beta", 21, 40).with_visibility("public"));
        ir.add_function(FunctionEntity::new("run", 5, 8).with_parent_class("Alpha"));
        ir.add_function(FunctionEntity::new("run", 25, 28).with_parent_class("Beta"));

        let (graph, info) = build(&ir);
        assert_eq!(info.functions.len(), 2);

        // Each "run" method is contained by its own class, not collapsed together.
        let alpha_children = graph
            .get_neighbors(info.classes[0], Direction::Outgoing)
            .unwrap();
        let beta_children = graph
            .get_neighbors(info.classes[1], Direction::Outgoing)
            .unwrap();
        assert_eq!(alpha_children.len(), 1);
        assert_eq!(beta_children.len(), 1);
        assert_ne!(alpha_children[0], beta_children[0]);
    }
}
