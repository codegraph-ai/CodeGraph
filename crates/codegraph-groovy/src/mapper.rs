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
            .with("language", "groovy");

        let id = graph
            .add_node(NodeType::CodeFile, props)
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
        node_map.insert(name, id);
        id
    };

    // Add top-level functions (rare in Groovy scripts)
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

    // Add classes and their methods
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

        // Add methods inside class
        for method in &class.methods {
            let method_name = format!("{}.{}", class.name, method.name);
            let mut method_props = PropertyMap::new()
                .with("name", method_name.clone())
                .with("path", file_path.display().to_string())
                .with("signature", method.signature.clone())
                .with("visibility", method.visibility.clone())
                .with("line_start", method.line_start as i64)
                .with("line_end", method.line_end as i64)
                .with("is_async", method.is_async)
                .with("is_static", method.is_static)
                .with("is_abstract", method.is_abstract)
                .with("is_method", "true")
                .with("parent_class", class.name.clone());

            if let Some(ref doc) = method.doc_comment {
                method_props = method_props.with("doc", doc.clone());
            }
            if let Some(ref return_type) = method.return_type {
                method_props = method_props.with("return_type", return_type.clone());
            }
            if let Some(ref body) = method.body_prefix {
                method_props = method_props.with("body_prefix", body.clone());
            }
            if !method.parameters.is_empty() {
                let param_names: Vec<String> =
                    method.parameters.iter().map(|p| p.name.clone()).collect();
                method_props = method_props.with("parameters", param_names);
            }
            if let Some(ref complexity) = method.complexity {
                method_props = method_props
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
        let info = ir_to_graph(ir, &mut graph, Path::new("Service.groovy")).unwrap();
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
        let ir = CodeIR::new(std::path::PathBuf::from("Service.groovy"));
        let (graph, info) = build(&ir);

        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(
            file.properties.get("name"),
            Some(&PropertyValue::String("Service".to_string()))
        );
        assert_eq!(
            file.properties.get("language"),
            Some(&PropertyValue::String("groovy".to_string()))
        );
        assert!(info.functions.is_empty());
        assert!(info.classes.is_empty());
        assert!(info.traits.is_empty());
        assert!(info.imports.is_empty());
        assert_eq!(info.line_count, 0);
    }

    #[test]
    fn module_drives_file_node_metadata_and_line_count() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.groovy"));
        let mut module = ModuleEntity::new("my.service", "src/Service.groovy", "groovy");
        module.line_count = 90;
        module.doc_comment = Some("script docs".to_string());
        ir.set_module(module);

        let (graph, info) = build(&ir);
        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(
            file.properties.get("name"),
            Some(&PropertyValue::String("my.service".to_string()))
        );
        assert_eq!(
            file.properties.get("path"),
            Some(&PropertyValue::String("src/Service.groovy".to_string()))
        );
        assert_eq!(
            file.properties.get("line_count"),
            Some(&PropertyValue::Int(90))
        );
        assert_eq!(
            file.properties.get("doc"),
            Some(&PropertyValue::String("script docs".to_string()))
        );
        assert_eq!(info.line_count, 90);
    }

    #[test]
    fn class_with_method_links_via_contains_edges_with_dot_name() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.groovy"));
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

        // Groovy qualifies method names as Class.method (dot separator).
        let method_id = info.functions[0];
        assert_eq!(name_of(&graph, method_id), "Repo.save");
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
    fn traits_are_ignored_and_emit_no_interface_nodes() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.groovy"));
        let mut tr = TraitEntity::new("Ordered", 1, 8);
        tr.required_methods
            .push(FunctionEntity::new("compare", 2, 3));
        ir.add_trait(tr);

        let (graph, info) = build(&ir);
        // The groovy mapper does not process ir.traits at all.
        assert!(info.traits.is_empty());
        assert!(info.functions.is_empty());
        // Only the file node exists; no Interface node was created.
        assert_eq!(graph.node_count(), 1);
        assert!(graph
            .nodes_iter()
            .all(|(_, n)| n.node_type != NodeType::Interface));
    }

    #[test]
    fn free_function_is_contained_by_file_with_complexity_and_flag_props() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.groovy"));
        let metrics = ComplexityMetrics {
            cyclomatic_complexity: 12,
            branches: 6,
            loops: 2,
            ..Default::default()
        };
        let func = FunctionEntity::new("run", 1, 30)
            .with_signature("def run()")
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
    fn import_creates_external_module_with_empty_edge_props() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.groovy"));
        ir.add_import(
            ImportRelation::new("Service", "java.util.List").with_symbols(vec!["List".to_string()]),
        );

        let (graph, info) = build(&ir);
        assert_eq!(info.imports.len(), 1);

        let import_id = info.imports[0];
        let import_node = graph.get_node(import_id).unwrap();
        assert_eq!(import_node.node_type, NodeType::Module);
        assert_eq!(
            import_node.properties.get("name"),
            Some(&PropertyValue::String("java.util.List".to_string()))
        );
        assert_eq!(
            import_node.properties.get("is_external"),
            Some(&PropertyValue::String("true".to_string()))
        );

        let edge_ids = graph.get_edges_between(info.file_id, import_id).unwrap();
        assert_eq!(edge_ids.len(), 1);
        let edge = graph.get_edge(edge_ids[0]).unwrap();
        assert_eq!(edge.edge_type, EdgeType::Imports);
        // The groovy mapper records no properties on the import edge.
        assert!(edge.properties.get("symbols").is_none());
        assert!(edge.properties.get("alias").is_none());
        assert!(edge.properties.get("is_wildcard").is_none());
    }

    #[test]
    fn call_relation_wires_calls_edge_only_between_known_nodes() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.groovy"));
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
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.groovy"));
        ir.add_import(ImportRelation::new("Service", "java.util"));
        ir.add_import(ImportRelation::new("Service", "java.util"));

        let (graph, info) = build(&ir);
        assert_eq!(info.imports.len(), 2);
        assert_eq!(info.imports[0], info.imports[1]);
        let edges = graph
            .get_edges_between(info.file_id, info.imports[0])
            .unwrap();
        assert_eq!(edges.len(), 2);
    }

    #[test]
    fn free_function_optional_props_present() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.groovy"));
        let func = FunctionEntity::new("run", 1, 5)
            .with_doc("does things")
            .with_return_type("String")
            .with_body_prefix("def run() {")
            .with_parameters(vec![
                Parameter::new("a").with_type("int"),
                Parameter::new("b"),
            ]);
        ir.add_function(func);

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.functions[0]).unwrap();
        assert_eq!(
            node.properties.get("doc"),
            Some(&PropertyValue::String("does things".to_string()))
        );
        assert_eq!(
            node.properties.get("return_type"),
            Some(&PropertyValue::String("String".to_string()))
        );
        assert_eq!(
            node.properties.get("body_prefix"),
            Some(&PropertyValue::String("def run() {".to_string()))
        );
        assert_eq!(
            node.properties.get("parameters"),
            Some(&PropertyValue::StringList(vec![
                "a".to_string(),
                "b".to_string()
            ]))
        );
    }

    #[test]
    fn free_function_optional_props_absent() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.groovy"));
        ir.add_function(FunctionEntity::new("run", 1, 5));

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.functions[0]).unwrap();
        assert!(node.properties.get("doc").is_none());
        assert!(node.properties.get("return_type").is_none());
        assert!(node.properties.get("body_prefix").is_none());
        assert!(node.properties.get("parameters").is_none());
        assert!(node.properties.get("complexity").is_none());
    }

    #[test]
    fn free_function_records_all_eight_complexity_subprops() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.groovy"));
        let metrics = ComplexityMetrics {
            cyclomatic_complexity: 25,
            branches: 4,
            loops: 3,
            logical_operators: 5,
            max_nesting_depth: 6,
            exception_handlers: 2,
            early_returns: 1,
        };
        ir.add_function(FunctionEntity::new("run", 1, 30).with_complexity(metrics));

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.functions[0]).unwrap();
        let int = |k: &str| node.properties.get(k).cloned();
        assert_eq!(int("complexity"), Some(PropertyValue::Int(25)));
        // 25 falls in the D band.
        assert_eq!(
            int("complexity_grade"),
            Some(PropertyValue::String("D".to_string()))
        );
        assert_eq!(int("complexity_branches"), Some(PropertyValue::Int(4)));
        assert_eq!(int("complexity_loops"), Some(PropertyValue::Int(3)));
        assert_eq!(int("complexity_logical_ops"), Some(PropertyValue::Int(5)));
        assert_eq!(int("complexity_nesting"), Some(PropertyValue::Int(6)));
        assert_eq!(int("complexity_exceptions"), Some(PropertyValue::Int(2)));
        assert_eq!(int("complexity_early_returns"), Some(PropertyValue::Int(1)));
    }

    #[test]
    fn complexity_grade_bands_a_and_f() {
        let grade_for = |cc: u32| {
            let mut ir = CodeIR::new(std::path::PathBuf::from("Service.groovy"));
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
        assert_eq!(grade_for(80), "F");
    }

    #[test]
    fn free_function_is_static_flag() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.groovy"));
        ir.add_function(FunctionEntity::new("helper", 1, 3).static_fn());

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.functions[0]).unwrap();
        assert_eq!(
            node.properties.get("is_static"),
            Some(&PropertyValue::Bool(true))
        );
    }

    #[test]
    fn class_optional_props_present_and_absent() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.groovy"));
        let documented = ClassEntity::new("Repo", 1, 20)
            .with_doc("repo docs")
            .with_attributes(vec!["@Component".to_string()])
            .with_body_prefix("class Repo {");
        ir.add_class(documented);
        ir.add_class(ClassEntity::new("Bare", 21, 30));

        let (graph, info) = build(&ir);
        assert_eq!(info.classes.len(), 2);

        let documented_node = graph.get_node(info.classes[0]).unwrap();
        assert_eq!(
            documented_node.properties.get("doc"),
            Some(&PropertyValue::String("repo docs".to_string()))
        );
        assert_eq!(
            documented_node.properties.get("attributes"),
            Some(&PropertyValue::StringList(vec!["@Component".to_string()]))
        );
        assert_eq!(
            documented_node.properties.get("body_prefix"),
            Some(&PropertyValue::String("class Repo {".to_string()))
        );

        let bare_node = graph.get_node(info.classes[1]).unwrap();
        assert!(bare_node.properties.get("doc").is_none());
        assert!(bare_node.properties.get("attributes").is_none());
        assert!(bare_node.properties.get("body_prefix").is_none());
    }

    #[test]
    fn method_optional_props_and_complexity() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.groovy"));
        let metrics = ComplexityMetrics {
            cyclomatic_complexity: 7,
            branches: 2,
            ..Default::default()
        };
        let method = FunctionEntity::new("save", 2, 8)
            .with_doc("saves")
            .with_return_type("void")
            .with_body_prefix("void save(x) {")
            .with_parameters(vec![Parameter::new("x")])
            .with_complexity(metrics);
        let class = ClassEntity::new("Repo", 1, 20).with_methods(vec![method]);
        ir.add_class(class);

        let (graph, info) = build(&ir);
        let method_id = info.functions[0];
        let node = graph.get_node(method_id).unwrap();
        assert_eq!(name_of(&graph, method_id), "Repo.save");
        assert_eq!(
            node.properties.get("doc"),
            Some(&PropertyValue::String("saves".to_string()))
        );
        assert_eq!(
            node.properties.get("return_type"),
            Some(&PropertyValue::String("void".to_string()))
        );
        assert_eq!(
            node.properties.get("body_prefix"),
            Some(&PropertyValue::String("void save(x) {".to_string()))
        );
        assert_eq!(
            node.properties.get("parameters"),
            Some(&PropertyValue::StringList(vec!["x".to_string()]))
        );
        assert_eq!(
            node.properties.get("complexity"),
            Some(&PropertyValue::Int(7))
        );
        // 7 falls in the B band.
        assert_eq!(
            node.properties.get("complexity_grade"),
            Some(&PropertyValue::String("B".to_string()))
        );
        assert_eq!(
            node.properties.get("complexity_branches"),
            Some(&PropertyValue::Int(2))
        );
    }

    #[test]
    fn import_reuses_in_file_node_and_skips_is_external() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.groovy"));
        // A same-file class the import name resolves to.
        ir.add_class(ClassEntity::new("Repo", 1, 20));
        ir.add_import(ImportRelation::new("Service", "Repo"));

        let (graph, info) = build(&ir);
        assert_eq!(info.imports.len(), 1);
        // The import reused the class node rather than creating a Module.
        assert_eq!(info.imports[0], info.classes[0]);

        let node = graph.get_node(info.imports[0]).unwrap();
        assert_eq!(node.node_type, NodeType::Class);
        // Reused nodes are never stamped is_external.
        assert!(node.properties.get("is_external").is_none());

        // Both a Contains and an Imports edge now run file -> Repo.
        let edge_ids = graph
            .get_edges_between(info.file_id, info.imports[0])
            .unwrap();
        let kinds: Vec<_> = edge_ids
            .iter()
            .map(|&e| graph.get_edge(e).unwrap().edge_type)
            .collect();
        assert_eq!(kinds.len(), 2);
        assert!(kinds.contains(&EdgeType::Contains));
        assert!(kinds.contains(&EdgeType::Imports));
    }

    #[test]
    fn call_relation_records_is_direct_flag() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.groovy"));
        ir.add_function(FunctionEntity::new("caller", 1, 5));
        ir.add_function(FunctionEntity::new("direct", 6, 8));
        ir.add_function(FunctionEntity::new("dynamic", 9, 11));
        ir.add_call(CallRelation::new("caller", "direct", 2));
        ir.add_call(CallRelation::new("caller", "dynamic", 3).indirect());

        let (graph, info) = build(&ir);
        let id_of = |n: &str| {
            info.functions
                .iter()
                .copied()
                .find(|&id| name_of(&graph, id) == n)
                .unwrap()
        };
        let is_direct = |callee: &str| {
            let edges = graph
                .get_edges_between(id_of("caller"), id_of(callee))
                .unwrap();
            let edge = edges
                .iter()
                .map(|&e| graph.get_edge(e).unwrap())
                .find(|e| e.edge_type == EdgeType::Calls)
                .unwrap();
            edge.properties.get("is_direct").cloned()
        };
        assert_eq!(is_direct("direct"), Some(PropertyValue::Bool(true)));
        assert_eq!(is_direct("dynamic"), Some(PropertyValue::Bool(false)));
    }

    #[test]
    fn multiple_classes_and_functions_contained_by_file() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.groovy"));
        ir.add_function(FunctionEntity::new("top", 1, 2));
        ir.add_class(ClassEntity::new("A", 3, 5));
        ir.add_class(ClassEntity::new("B", 6, 8));

        let (graph, info) = build(&ir);
        assert_eq!(info.functions.len(), 1);
        assert_eq!(info.classes.len(), 2);

        let outgoing = graph
            .get_neighbors(info.file_id, Direction::Outgoing)
            .unwrap();
        assert!(outgoing.contains(&info.functions[0]));
        assert!(outgoing.contains(&info.classes[0]));
        assert!(outgoing.contains(&info.classes[1]));
    }

    #[test]
    fn free_function_line_signature_visibility_path_props() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.groovy"));
        ir.add_function(
            FunctionEntity::new("run", 4, 12)
                .with_signature("def run(int a)")
                .with_visibility("private"),
        );

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.functions[0]).unwrap();
        assert_eq!(
            node.properties.get("line_start"),
            Some(&PropertyValue::Int(4))
        );
        assert_eq!(
            node.properties.get("line_end"),
            Some(&PropertyValue::Int(12))
        );
        assert_eq!(
            node.properties.get("signature"),
            Some(&PropertyValue::String("def run(int a)".to_string()))
        );
        assert_eq!(
            node.properties.get("visibility"),
            Some(&PropertyValue::String("private".to_string()))
        );
        // The mapper stamps the file path on every function node.
        assert_eq!(
            node.properties.get("path"),
            Some(&PropertyValue::String("Service.groovy".to_string()))
        );
    }

    #[test]
    fn free_function_is_abstract_flag() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.groovy"));
        ir.add_function(FunctionEntity::new("shape", 1, 3).abstract_fn());

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.functions[0]).unwrap();
        assert_eq!(
            node.properties.get("is_abstract"),
            Some(&PropertyValue::Bool(true))
        );
        // async/static default to false when only is_abstract is set.
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
    fn method_writes_full_flag_and_complexity_prop_set() {
        // Unlike the scala/dart/solidity mappers, the groovy class-method loop
        // writes the SAME broad prop set as free functions: is_async, is_static,
        // is_abstract and the complexity subprops all survive for methods.
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.groovy"));
        let metrics = ComplexityMetrics {
            cyclomatic_complexity: 4,
            branches: 1,
            ..Default::default()
        };
        let method = FunctionEntity::new("load", 3, 9)
            .async_fn()
            .static_fn()
            .with_complexity(metrics);
        ir.add_class(ClassEntity::new("Repo", 1, 20).with_methods(vec![method]));

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.functions[0]).unwrap();
        assert_eq!(name_of(&graph, info.functions[0]), "Repo.load");
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
            Some(&PropertyValue::Bool(false))
        );
        assert_eq!(
            node.properties.get("complexity"),
            Some(&PropertyValue::Int(4))
        );
        assert_eq!(
            node.properties.get("complexity_branches"),
            Some(&PropertyValue::Int(1))
        );
    }

    #[test]
    fn method_line_path_visibility_bounds() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.groovy"));
        let method = FunctionEntity::new("save", 6, 14)
            .with_signature("void save()")
            .with_visibility("protected");
        ir.add_class(ClassEntity::new("Repo", 1, 20).with_methods(vec![method]));

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.functions[0]).unwrap();
        assert_eq!(
            node.properties.get("line_start"),
            Some(&PropertyValue::Int(6))
        );
        assert_eq!(
            node.properties.get("line_end"),
            Some(&PropertyValue::Int(14))
        );
        assert_eq!(
            node.properties.get("signature"),
            Some(&PropertyValue::String("void save()".to_string()))
        );
        assert_eq!(
            node.properties.get("visibility"),
            Some(&PropertyValue::String("protected".to_string()))
        );
        assert_eq!(
            node.properties.get("path"),
            Some(&PropertyValue::String("Service.groovy".to_string()))
        );
    }

    #[test]
    fn class_line_visibility_path_and_default_abstract_props() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.groovy"));
        ir.add_class(ClassEntity::new("Bare", 2, 18).with_visibility("public"));

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.classes[0]).unwrap();
        assert_eq!(
            node.properties.get("line_start"),
            Some(&PropertyValue::Int(2))
        );
        assert_eq!(
            node.properties.get("line_end"),
            Some(&PropertyValue::Int(18))
        );
        assert_eq!(
            node.properties.get("visibility"),
            Some(&PropertyValue::String("public".to_string()))
        );
        assert_eq!(
            node.properties.get("path"),
            Some(&PropertyValue::String("Service.groovy".to_string()))
        );
        // is_abstract defaults to false for a plain class.
        assert_eq!(
            node.properties.get("is_abstract"),
            Some(&PropertyValue::Bool(false))
        );
    }

    #[test]
    fn module_without_doc_omits_doc_prop() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.groovy"));
        // Module present but with no doc_comment -> the file node skips `doc`.
        ir.set_module(ModuleEntity::new(
            "my.service",
            "src/Service.groovy",
            "groovy",
        ));

        let (graph, info) = build(&ir);
        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(
            file.properties.get("name"),
            Some(&PropertyValue::String("my.service".to_string()))
        );
        assert!(file.properties.get("doc").is_none());
    }

    #[test]
    fn import_reuses_in_file_function_node() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.groovy"));
        ir.add_function(FunctionEntity::new("helper", 1, 5));
        ir.add_import(ImportRelation::new("Service", "helper"));

        let (graph, info) = build(&ir);
        assert_eq!(info.imports.len(), 1);
        // The import reused the function node rather than creating a Module.
        assert_eq!(info.imports[0], info.functions[0]);

        let node = graph.get_node(info.imports[0]).unwrap();
        assert_eq!(node.node_type, NodeType::Function);
        assert!(node.properties.get("is_external").is_none());
    }

    #[test]
    fn import_targeting_module_name_reuses_file_node_as_self_loop() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.groovy"));
        ir.set_module(ModuleEntity::new(
            "my.service",
            "src/Service.groovy",
            "groovy",
        ));
        // Import whose target equals the module name resolves to the file node.
        ir.add_import(ImportRelation::new("Service", "my.service"));

        let (graph, info) = build(&ir);
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0], info.file_id);

        let edge_ids = graph.get_edges_between(info.file_id, info.file_id).unwrap();
        assert_eq!(edge_ids.len(), 1);
        let edge = graph.get_edge(edge_ids[0]).unwrap();
        assert_eq!(edge.edge_type, EdgeType::Imports);
    }

    #[test]
    fn call_relation_wires_method_to_function() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.groovy"));
        ir.add_function(FunctionEntity::new("util", 1, 3));
        let method = FunctionEntity::new("save", 6, 10);
        ir.add_class(ClassEntity::new("Repo", 5, 20).with_methods(vec![method]));
        // Caller is the qualified method name, callee is the free function.
        ir.add_call(CallRelation::new("Repo.save", "util", 8));

        let (graph, info) = build(&ir);
        let method_id = info
            .functions
            .iter()
            .copied()
            .find(|&id| name_of(&graph, id) == "Repo.save")
            .unwrap();
        let util_id = info
            .functions
            .iter()
            .copied()
            .find(|&id| name_of(&graph, id) == "util")
            .unwrap();

        let call_edges: Vec<_> = graph
            .get_edges_between(method_id, util_id)
            .unwrap()
            .into_iter()
            .filter(|&e| graph.get_edge(e).unwrap().edge_type == EdgeType::Calls)
            .collect();
        assert_eq!(call_edges.len(), 1);
        assert_eq!(
            graph
                .get_edge(call_edges[0])
                .unwrap()
                .properties
                .get("call_site_line"),
            Some(&PropertyValue::Int(8))
        );
    }

    #[test]
    fn empty_path_stem_file_node_omits_doc_and_line_count_zero() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Service.groovy"));
        ir.add_class(ClassEntity::new("A", 1, 4));

        // No module -> file node comes from the path stem and never has a doc prop.
        let (graph, info) = build(&ir);
        let file = graph.get_node(info.file_id).unwrap();
        assert!(file.properties.get("doc").is_none());
        assert_eq!(
            file.properties.get("path"),
            Some(&PropertyValue::String("Service.groovy".to_string()))
        );
        assert_eq!(info.line_count, 0);
    }
}
