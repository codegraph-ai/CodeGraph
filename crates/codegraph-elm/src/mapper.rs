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
            .with("language", "elm");

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
        if let Some(ref rt) = func.return_type {
            props = props.with("return_type", rt.clone());
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

    for class in &ir.classes {
        let mut props = PropertyMap::new()
            .with("name", class.name.clone())
            .with("path", file_path.display().to_string())
            .with("visibility", class.visibility.clone())
            .with("line_start", class.line_start as i64)
            .with("line_end", class.line_end as i64);

        if let Some(ref doc) = class.doc_comment {
            props = props.with("doc", doc.clone());
        }

        let class_id = graph
            .add_node(NodeType::Type, props)
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

        graph
            .add_edge(file_id, import_id, EdgeType::Imports, PropertyMap::new())
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
    }

    let line_count = ir.module.as_ref().map(|m| m.line_count).unwrap_or(0);

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
        ClassEntity, ComplexityMetrics, FunctionEntity, ImportRelation, ModuleEntity, Parameter,
        TraitEntity,
    };

    fn build(ir: &CodeIR) -> (CodeGraph, FileInfo) {
        let mut graph = CodeGraph::in_memory().unwrap();
        let info = ir_to_graph(ir, &mut graph, Path::new("Main.elm")).unwrap();
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
        let ir = CodeIR::new(std::path::PathBuf::from("Main.elm"));
        let (graph, info) = build(&ir);

        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(
            file.properties.get("name"),
            Some(&PropertyValue::String("Main".to_string()))
        );
        assert_eq!(
            file.properties.get("language"),
            Some(&PropertyValue::String("elm".to_string()))
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
        let mut ir = CodeIR::new(std::path::PathBuf::from("Main.elm"));
        let mut module = ModuleEntity::new("Main", "src/Main.elm", "elm");
        module.line_count = 120;
        module.doc_comment = Some("app entry".to_string());
        ir.set_module(module);

        let (graph, info) = build(&ir);
        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(
            file.properties.get("name"),
            Some(&PropertyValue::String("Main".to_string()))
        );
        assert_eq!(
            file.properties.get("path"),
            Some(&PropertyValue::String("src/Main.elm".to_string()))
        );
        assert_eq!(
            file.properties.get("line_count"),
            Some(&PropertyValue::Int(120))
        );
        assert_eq!(
            file.properties.get("doc"),
            Some(&PropertyValue::String("app entry".to_string()))
        );
        assert_eq!(info.line_count, 120);
    }

    #[test]
    fn class_maps_to_type_node_dropping_methods() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Main.elm"));
        let mut class = ClassEntity::new("Model", 1, 5).with_visibility("public");
        class
            .methods
            .push(FunctionEntity::new("update", 2, 4).with_visibility("public"));
        ir.add_class(class);

        let (graph, info) = build(&ir);
        // The elm mapper emits a Type node per class but never iterates class.methods.
        assert_eq!(info.classes.len(), 1);
        assert!(info.functions.is_empty());

        let class_id = info.classes[0];
        let node = graph.get_node(class_id).unwrap();
        assert_eq!(node.node_type, NodeType::Type);
        assert_eq!(name_of(&graph, class_id), "Model");
        assert_eq!(
            node.properties.get("visibility"),
            Some(&PropertyValue::String("public".to_string()))
        );

        let neighbors = graph
            .get_neighbors(info.file_id, Direction::Outgoing)
            .unwrap();
        assert!(neighbors.contains(&class_id));
        // file + type node only, no method Function node.
        assert_eq!(graph.node_count(), 2);
    }

    #[test]
    fn traits_are_ignored_by_the_mapper() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Main.elm"));
        ir.add_trait(TraitEntity::new("Comparable", 1, 3));

        let (graph, info) = build(&ir);
        // The elm mapper never iterates ir.traits, so no Interface node exists.
        assert!(info.traits.is_empty());
        assert_eq!(graph.node_count(), 1);
        assert!(graph
            .nodes_iter()
            .all(|(_, node)| node.node_type != NodeType::Interface));
    }

    #[test]
    fn free_function_is_contained_by_file_with_complexity_and_flags() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Main.elm"));
        let metrics = ComplexityMetrics {
            cyclomatic_complexity: 12,
            branches: 6,
            loops: 2,
            ..Default::default()
        };
        let func = FunctionEntity::new("update", 1, 30)
            .with_signature("update : Msg -> Model -> Model")
            .with_complexity(metrics);
        ir.add_function(func);

        let (graph, info) = build(&ir);
        assert_eq!(info.functions.len(), 1);

        let func_id = info.functions[0];
        // Elm keeps function names bare (no qualification).
        assert_eq!(name_of(&graph, func_id), "update");
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
    fn function_records_signature_visibility_and_line_bounds() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Main.elm"));
        let func = FunctionEntity::new("view", 3, 9)
            .with_signature("view : Model -> Html Msg")
            .with_visibility("public");
        ir.add_function(func);

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.functions[0]).unwrap();
        assert_eq!(
            node.properties.get("signature"),
            Some(&PropertyValue::String(
                "view : Model -> Html Msg".to_string()
            ))
        );
        assert_eq!(
            node.properties.get("visibility"),
            Some(&PropertyValue::String("public".to_string()))
        );
        assert_eq!(
            node.properties.get("line_start"),
            Some(&PropertyValue::Int(3))
        );
        assert_eq!(
            node.properties.get("line_end"),
            Some(&PropertyValue::Int(9))
        );
        assert_eq!(
            node.properties.get("is_abstract"),
            Some(&PropertyValue::Bool(false))
        );
        // No complexity supplied, so complexity props are absent.
        assert_eq!(node.properties.get("complexity"), None);
        assert_eq!(node.properties.get("complexity_grade"), None);
    }

    #[test]
    fn import_creates_external_module_with_empty_edge_props() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Main.elm"));
        ir.add_import(ImportRelation::new("Main", "Html").with_symbols(vec!["div".to_string()]));

        let (graph, info) = build(&ir);
        assert_eq!(info.imports.len(), 1);

        let import_id = info.imports[0];
        let import_node = graph.get_node(import_id).unwrap();
        assert_eq!(import_node.node_type, NodeType::Module);
        assert_eq!(
            import_node.properties.get("name"),
            Some(&PropertyValue::String("Html".to_string()))
        );
        assert_eq!(
            import_node.properties.get("is_external"),
            Some(&PropertyValue::String("true".to_string()))
        );

        let edge_ids = graph.get_edges_between(info.file_id, import_id).unwrap();
        assert_eq!(edge_ids.len(), 1);
        let edge = graph.get_edge(edge_ids[0]).unwrap();
        assert_eq!(edge.edge_type, EdgeType::Imports);
        // The elm mapper records NO props on the Imports edge (symbols dropped).
        assert_eq!(edge.properties.get("symbols"), None);
        assert_eq!(edge.properties.get("alias"), None);
        assert_eq!(edge.properties.get("is_wildcard"), None);
    }

    #[test]
    fn duplicate_import_target_reuses_existing_node() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Main.elm"));
        ir.add_import(ImportRelation::new("Main", "Json.Decode"));
        ir.add_import(ImportRelation::new("Main", "Json.Decode"));

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
        let mut ir = CodeIR::new(std::path::PathBuf::from("Main.elm"));
        let func = FunctionEntity::new("view", 1, 4)
            .with_doc("renders the view")
            .with_body_prefix("view model =")
            .with_return_type("Html Msg")
            .with_parameters(vec![
                Parameter::new("model"),
                Parameter::new("flags").with_type("Flags"),
            ]);
        ir.add_function(func);

        let (graph, info) = build(&ir);
        let id = info.functions[0];
        assert_eq!(
            prop(&graph, id, "doc"),
            Some(PropertyValue::String("renders the view".to_string()))
        );
        assert_eq!(
            prop(&graph, id, "body_prefix"),
            Some(PropertyValue::String("view model =".to_string()))
        );
        assert_eq!(
            prop(&graph, id, "return_type"),
            Some(PropertyValue::String("Html Msg".to_string()))
        );
        assert_eq!(
            prop(&graph, id, "parameters"),
            Some(PropertyValue::StringList(vec![
                "model".to_string(),
                "flags".to_string()
            ]))
        );
    }

    #[test]
    fn function_optional_props_absent() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Main.elm"));
        ir.add_function(FunctionEntity::new("init", 1, 2));

        let (graph, info) = build(&ir);
        let id = info.functions[0];
        assert_eq!(prop(&graph, id, "doc"), None);
        assert_eq!(prop(&graph, id, "body_prefix"), None);
        assert_eq!(prop(&graph, id, "return_type"), None);
        // Empty parameter list yields no parameters prop.
        assert_eq!(prop(&graph, id, "parameters"), None);
    }

    #[test]
    fn all_eight_complexity_subprops_stamped() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Main.elm"));
        let metrics = ComplexityMetrics {
            cyclomatic_complexity: 25,
            branches: 8,
            loops: 3,
            logical_operators: 4,
            max_nesting_depth: 5,
            exception_handlers: 2,
            early_returns: 6,
        };
        ir.add_function(FunctionEntity::new("reduce", 1, 40).with_complexity(metrics));

        let (graph, info) = build(&ir);
        let id = info.functions[0];
        // cyclomatic 25 falls in the D band.
        assert_eq!(prop(&graph, id, "complexity"), Some(PropertyValue::Int(25)));
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
    fn complexity_grade_band_a() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Main.elm"));
        let metrics = ComplexityMetrics {
            cyclomatic_complexity: 3,
            ..Default::default()
        };
        ir.add_function(FunctionEntity::new("noop", 1, 2).with_complexity(metrics));

        let (graph, info) = build(&ir);
        assert_eq!(
            prop(&graph, info.functions[0], "complexity_grade"),
            Some(PropertyValue::String("A".to_string()))
        );
    }

    #[test]
    fn complexity_grade_band_f() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Main.elm"));
        let metrics = ComplexityMetrics {
            cyclomatic_complexity: 60,
            ..Default::default()
        };
        ir.add_function(FunctionEntity::new("giant", 1, 200).with_complexity(metrics));

        let (graph, info) = build(&ir);
        assert_eq!(
            prop(&graph, info.functions[0], "complexity_grade"),
            Some(PropertyValue::String("F".to_string()))
        );
    }

    #[test]
    fn function_boolean_flags_stamped() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Main.elm"));
        let func = FunctionEntity::new("effect", 1, 3)
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
    fn class_doc_present_on_type_node() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Main.elm"));
        ir.add_class(ClassEntity::new("Model", 1, 5).with_doc("app model"));

        let (graph, info) = build(&ir);
        let id = info.classes[0];
        assert_eq!(graph.get_node(id).unwrap().node_type, NodeType::Type);
        assert_eq!(
            prop(&graph, id, "doc"),
            Some(PropertyValue::String("app model".to_string()))
        );
    }

    #[test]
    fn class_type_node_omits_unread_props() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Main.elm"));
        // attributes/body_prefix/type_parameters are set but the elm class loop
        // only reads name/path/visibility/line bounds/doc.
        let class = ClassEntity::new("Msg", 2, 8)
            .with_attributes(vec!["deriving".to_string()])
            .with_body_prefix("type Msg =");
        ir.add_class(class);

        let (graph, info) = build(&ir);
        let id = info.classes[0];
        assert_eq!(prop(&graph, id, "doc"), None);
        assert_eq!(prop(&graph, id, "attributes"), None);
        assert_eq!(prop(&graph, id, "body_prefix"), None);
        assert_eq!(prop(&graph, id, "is_abstract"), None);
        assert_eq!(prop(&graph, id, "line_start"), Some(PropertyValue::Int(2)));
        assert_eq!(prop(&graph, id, "line_end"), Some(PropertyValue::Int(8)));
    }

    #[test]
    fn import_reuses_in_file_function_node() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Main.elm"));
        ir.add_function(FunctionEntity::new("decode", 1, 3));
        // The import target matches an already-mapped function name, so the
        // mapper reuses that node instead of creating an external Module.
        ir.add_import(ImportRelation::new("Main", "decode"));

        let (graph, info) = build(&ir);
        assert_eq!(info.imports.len(), 1);
        let reused = info.imports[0];
        assert_eq!(reused, info.functions[0]);
        // Reused node keeps its Function type and gets no is_external stamp.
        assert_eq!(
            graph.get_node(reused).unwrap().node_type,
            NodeType::Function
        );
        assert_eq!(prop(&graph, reused, "is_external"), None);

        // Both a Contains and an Imports edge now connect file -> node.
        let edge_ids = graph.get_edges_between(info.file_id, reused).unwrap();
        assert_eq!(edge_ids.len(), 2);
        let types: Vec<EdgeType> = edge_ids
            .iter()
            .map(|&e| graph.get_edge(e).unwrap().edge_type)
            .collect();
        assert!(types.contains(&EdgeType::Contains));
        assert!(types.contains(&EdgeType::Imports));
    }

    #[test]
    fn multiple_functions_and_classes_contained_by_file() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Main.elm"));
        ir.add_function(FunctionEntity::new("update", 1, 3));
        ir.add_function(FunctionEntity::new("view", 4, 6));
        ir.add_class(ClassEntity::new("Model", 7, 9));
        ir.add_class(ClassEntity::new("Msg", 10, 12));

        let (graph, info) = build(&ir);
        assert_eq!(info.functions.len(), 2);
        assert_eq!(info.classes.len(), 2);
        // file + 2 functions + 2 types.
        assert_eq!(graph.node_count(), 5);

        let neighbors = graph
            .get_neighbors(info.file_id, Direction::Outgoing)
            .unwrap();
        for id in info.functions.iter().chain(info.classes.iter()) {
            assert!(neighbors.contains(id));
        }
    }

    #[test]
    fn module_without_doc_omits_doc_prop() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Main.elm"));
        let mut module = ModuleEntity::new("Main", "src/Main.elm", "elm");
        module.line_count = 10;
        ir.set_module(module);

        let (graph, info) = build(&ir);
        assert_eq!(prop(&graph, info.file_id, "doc"), None);
    }
}
