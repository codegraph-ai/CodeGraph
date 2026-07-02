// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use codegraph::{helpers, CodeGraph, EdgeType, NodeId, NodeType, PropertyMap};
use codegraph_parser_api::CodeIR;
use std::collections::HashMap;

use crate::error::Result;

/// Build a graph from the intermediate representation.
///
/// Takes a `CodeIR` structure and adds all entities and relationships to the given graph.
///
/// # Arguments
///
/// * `graph` - Mutable reference to the code graph
/// * `ir` - The intermediate representation containing entities and relationships
/// * `file_path` - Path to the source file being processed
///
/// # Returns
///
/// The `NodeId` of the file node created, or an error if building fails.
pub fn build_graph(graph: &mut CodeGraph, ir: &CodeIR, file_path: &str) -> Result<NodeId> {
    // Add the file/module node
    let file_id = helpers::add_file(graph, file_path, "python")
        .map_err(|e| crate::error::ParseError::GraphError(e.to_string()))?;

    // Track entity name -> NodeId mappings for relationship building
    let mut entity_map: HashMap<String, NodeId> = HashMap::new();

    // Add all functions
    for func in &ir.functions {
        let mut props = PropertyMap::new()
            .with("name", func.name.clone())
            .with("signature", func.signature.clone())
            .with("line_start", func.line_start as i64)
            .with("line_end", func.line_end as i64)
            .with("visibility", func.visibility.clone())
            .with("is_async", func.is_async)
            .with("is_static", func.is_static)
            .with("is_test", func.is_test)
            .with("attributes", func.attributes.clone());

        if let Some(ref body) = func.body_prefix {
            props = props.with("body_prefix", body.clone());
        }

        // Add complexity metrics if available
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
            .map_err(|e| crate::error::ParseError::GraphError(e.to_string()))?;

        // Add Contains edge from file to function
        graph
            .add_edge(file_id, func_id, EdgeType::Contains, PropertyMap::new())
            .map_err(|e| crate::error::ParseError::GraphError(e.to_string()))?;

        entity_map.insert(func.name.clone(), func_id);
    }

    // Add all classes
    for class in &ir.classes {
        let class_id = helpers::add_class(
            graph,
            file_id,
            &class.name,
            class.line_start as i64,
            class.line_end as i64,
        )
        .map_err(|e| crate::error::ParseError::GraphError(e.to_string()))?;

        if let Some(ref body) = class.body_prefix {
            if let Ok(node) = graph.get_node(class_id) {
                let new_props = node.properties.clone().with("body_prefix", body.clone());
                let _ = graph.update_node_properties(class_id, new_props);
            }
        }

        entity_map.insert(class.name.clone(), class_id);

        // Add methods as functions linked to the class
        for method in &class.methods {
            let method_id = helpers::add_method(
                graph,
                class_id,
                &method.name,
                method.line_start as i64,
                method.line_end as i64,
            )
            .map_err(|e| crate::error::ParseError::GraphError(e.to_string()))?;

            // Track methods with qualified name for call relationships
            let qualified = format!("{}.{}", class.name, method.name);
            entity_map.insert(qualified, method_id);
        }
    }

    // Add call relationships
    for call in &ir.calls {
        if let (Some(&caller_id), Some(&callee_id)) =
            (entity_map.get(&call.caller), entity_map.get(&call.callee))
        {
            helpers::add_call(graph, caller_id, callee_id, call.call_site_line as i64)
                .map_err(|e| crate::error::ParseError::GraphError(e.to_string()))?;
        }
    }

    // Add import relationships
    for import in &ir.imports {
        let imported_module = &import.imported;

        let import_id = if let Some(&existing_id) = entity_map.get(imported_module) {
            existing_id
        } else {
            let is_external = !imported_module.starts_with('.');
            let props = PropertyMap::new()
                .with("name", imported_module.clone())
                .with("is_external", is_external.to_string());

            let id = graph
                .add_node(NodeType::Module, props)
                .map_err(|e| crate::error::ParseError::GraphError(e.to_string()))?;
            entity_map.insert(imported_module.clone(), id);
            id
        };

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
            .map_err(|e| crate::error::ParseError::GraphError(e.to_string()))?;
    }

    Ok(file_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph::EdgeType;
    use codegraph_parser_api::{
        CallRelation, ClassEntity, ComplexityMetrics, FunctionEntity, ImportRelation,
    };

    /// Find the first node of the given type whose `name` property matches.
    fn find_node(graph: &CodeGraph, nt: NodeType, name: &str) -> Option<NodeId> {
        graph.iter_nodes().find_map(|(id, node)| {
            if node.node_type == nt && node.properties.get_string("name") == Some(name) {
                Some(id)
            } else {
                None
            }
        })
    }

    #[test]
    fn test_build_empty_module() {
        let mut graph = CodeGraph::in_memory().unwrap();
        let ir = CodeIR::new(std::path::PathBuf::from("test.py"));

        let result = build_graph(&mut graph, &ir, "test.py");
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_with_function() {
        let mut graph = CodeGraph::in_memory().unwrap();
        let mut ir = CodeIR::new(std::path::PathBuf::from("test.py"));

        ir.add_function(FunctionEntity::new("test_func", 1, 3));

        let result = build_graph(&mut graph, &ir, "test.py");
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_with_class() {
        let mut graph = CodeGraph::in_memory().unwrap();
        let mut ir = CodeIR::new(std::path::PathBuf::from("test.py"));

        ir.add_class(ClassEntity::new("MyClass", 1, 4));

        let result = build_graph(&mut graph, &ir, "test.py");
        assert!(result.is_ok());
    }

    #[test]
    fn test_build_with_relationships() {
        let mut graph = CodeGraph::in_memory().unwrap();
        let mut ir = CodeIR::new(std::path::PathBuf::from("test.py"));

        // Add two functions
        ir.add_function(FunctionEntity::new("caller", 1, 3));
        ir.add_function(FunctionEntity::new("callee", 5, 7));

        // Add call relationship using parser-API constructor
        ir.add_call(CallRelation::new("caller", "callee", 2));

        // Add import using parser-API constructor
        ir.add_import(ImportRelation::new("test", "os"));

        let result = build_graph(&mut graph, &ir, "test.py");
        assert!(result.is_ok());
    }

    #[test]
    fn function_node_carries_all_scalar_props_and_contains_edge() {
        let mut graph = CodeGraph::in_memory().unwrap();
        let mut ir = CodeIR::new(std::path::PathBuf::from("m.py"));
        let func = FunctionEntity::new("worker", 3, 9)
            .with_signature("def worker(x)")
            .with_visibility("private")
            .with_attributes(vec!["deco".to_string()]);
        ir.add_function(func);

        let file_id = build_graph(&mut graph, &ir, "m.py").unwrap();
        let fid = find_node(&graph, NodeType::Function, "worker").expect("function node");
        let props = &graph.get_node(fid).unwrap().properties;
        assert_eq!(props.get_string("signature"), Some("def worker(x)"));
        assert_eq!(props.get_int("line_start"), Some(3));
        assert_eq!(props.get_int("line_end"), Some(9));
        assert_eq!(props.get_string("visibility"), Some("private"));
        assert_eq!(props.get_bool("is_async"), Some(false));
        assert_eq!(props.get_bool("is_static"), Some(false));
        assert_eq!(props.get_bool("is_test"), Some(false));
        assert_eq!(
            props.get_string_list("attributes"),
            Some(&["deco".to_string()][..])
        );
        // body_prefix/complexity absent when unset
        assert!(!props.contains_key("body_prefix"));
        assert!(!props.contains_key("complexity"));
        // Contains edge file -> function exists
        let edges = graph.get_edges_between(file_id, fid).unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(
            graph.get_edge(edges[0]).unwrap().edge_type,
            EdgeType::Contains
        );
    }

    #[test]
    fn function_body_prefix_and_complexity_metrics_expand() {
        let mut graph = CodeGraph::in_memory().unwrap();
        let mut ir = CodeIR::new(std::path::PathBuf::from("m.py"));
        let metrics = ComplexityMetrics::new()
            .with_branches(4)
            .with_loops(2)
            .with_logical_operators(3)
            .with_nesting_depth(5)
            .with_exception_handlers(1)
            .with_early_returns(2)
            .finalize();
        let func = FunctionEntity::new("heavy", 1, 40)
            .with_body_prefix("def heavy():")
            .with_complexity(metrics.clone());
        ir.add_function(func);

        build_graph(&mut graph, &ir, "m.py").unwrap();
        let fid = find_node(&graph, NodeType::Function, "heavy").unwrap();
        let props = &graph.get_node(fid).unwrap().properties;
        assert_eq!(props.get_string("body_prefix"), Some("def heavy():"));
        assert_eq!(
            props.get_int("complexity"),
            Some(metrics.cyclomatic_complexity as i64)
        );
        assert_eq!(
            props.get_string("complexity_grade"),
            Some(metrics.grade().to_string().as_str())
        );
        assert_eq!(props.get_int("complexity_branches"), Some(4));
        assert_eq!(props.get_int("complexity_loops"), Some(2));
        assert_eq!(props.get_int("complexity_logical_ops"), Some(3));
        assert_eq!(props.get_int("complexity_nesting"), Some(5));
        assert_eq!(props.get_int("complexity_exceptions"), Some(1));
        assert_eq!(props.get_int("complexity_early_returns"), Some(2));
    }

    #[test]
    fn class_methods_and_body_prefix_are_wired() {
        let mut graph = CodeGraph::in_memory().unwrap();
        let mut ir = CodeIR::new(std::path::PathBuf::from("m.py"));
        let class = ClassEntity::new("Widget", 1, 20)
            .with_body_prefix("class Widget:")
            .with_methods(vec![FunctionEntity::new("render", 2, 5)]);
        ir.add_class(class);

        build_graph(&mut graph, &ir, "m.py").unwrap();
        let cid = find_node(&graph, NodeType::Class, "Widget").expect("class node");
        assert_eq!(
            graph
                .get_node(cid)
                .unwrap()
                .properties
                .get_string("body_prefix"),
            Some("class Widget:")
        );
        // Method exists as a Function node linked to the class via Contains
        let mid = find_node(&graph, NodeType::Function, "render").expect("method node");
        let edges = graph.get_edges_between(cid, mid).unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(
            graph.get_edge(edges[0]).unwrap().edge_type,
            EdgeType::Contains
        );
    }

    #[test]
    fn call_edge_created_only_when_both_endpoints_resolve() {
        let mut graph = CodeGraph::in_memory().unwrap();
        let mut ir = CodeIR::new(std::path::PathBuf::from("m.py"));
        ir.add_function(FunctionEntity::new("caller", 1, 3));
        ir.add_function(FunctionEntity::new("callee", 5, 7));
        // Resolvable call, plus one whose callee is unknown (must be skipped).
        ir.add_call(CallRelation::new("caller", "callee", 2));
        ir.add_call(CallRelation::new("caller", "ghost", 4));

        build_graph(&mut graph, &ir, "m.py").unwrap();
        let caller = find_node(&graph, NodeType::Function, "caller").unwrap();
        let callee = find_node(&graph, NodeType::Function, "callee").unwrap();
        let edges = graph.get_edges_between(caller, callee).unwrap();
        assert_eq!(edges.len(), 1);
        let edge = graph.get_edge(edges[0]).unwrap();
        assert_eq!(edge.edge_type, EdgeType::Calls);
        assert_eq!(edge.properties.get_int("line"), Some(2));
        // The unresolved "ghost" callee never became a node.
        assert!(find_node(&graph, NodeType::Function, "ghost").is_none());
        // Exactly one Calls edge in the whole graph.
        assert_eq!(
            graph
                .iter_edges()
                .filter(|(_, e)| e.edge_type == EdgeType::Calls)
                .count(),
            1
        );
    }

    #[test]
    fn external_import_creates_module_with_edge_props() {
        let mut graph = CodeGraph::in_memory().unwrap();
        let mut ir = CodeIR::new(std::path::PathBuf::from("m.py"));
        ir.add_import(
            ImportRelation::new("m", "numpy")
                .with_alias("np")
                .with_symbols(vec!["array".to_string()]),
        );

        let file_id = build_graph(&mut graph, &ir, "m.py").unwrap();
        let mid = find_node(&graph, NodeType::Module, "numpy").expect("module node");
        // is_external is stored as a String, not a bool.
        assert_eq!(
            graph
                .get_node(mid)
                .unwrap()
                .properties
                .get_string("is_external"),
            Some("true")
        );
        let edges = graph.get_edges_between(file_id, mid).unwrap();
        assert_eq!(edges.len(), 1);
        let edge = graph.get_edge(edges[0]).unwrap();
        assert_eq!(edge.edge_type, EdgeType::Imports);
        assert_eq!(edge.properties.get_string("alias"), Some("np"));
        assert_eq!(
            edge.properties.get_string_list("symbols"),
            Some(&["array".to_string()][..])
        );
        assert!(!edge.properties.contains_key("is_wildcard"));
    }

    #[test]
    fn relative_wildcard_import_marks_internal_and_wildcard() {
        let mut graph = CodeGraph::in_memory().unwrap();
        let mut ir = CodeIR::new(std::path::PathBuf::from("m.py"));
        ir.add_import(ImportRelation::new("m", ".sibling").wildcard());

        build_graph(&mut graph, &ir, "m.py").unwrap();
        let mid = find_node(&graph, NodeType::Module, ".sibling").expect("module node");
        // A leading dot marks the module as internal.
        assert_eq!(
            graph
                .get_node(mid)
                .unwrap()
                .properties
                .get_string("is_external"),
            Some("false")
        );
        let wildcard_edge = graph
            .iter_edges()
            .find(|(_, e)| e.edge_type == EdgeType::Imports)
            .and_then(|(_, e)| e.properties.get_string("is_wildcard").map(str::to_string));
        assert_eq!(wildcard_edge, Some("true".to_string()));
    }

    #[test]
    fn import_reuses_existing_entity_instead_of_new_module() {
        let mut graph = CodeGraph::in_memory().unwrap();
        let mut ir = CodeIR::new(std::path::PathBuf::from("m.py"));
        // A function named "helper" already lives in entity_map...
        ir.add_function(FunctionEntity::new("helper", 1, 2));
        // ...so importing "helper" must reuse that node, not add a Module.
        ir.add_import(ImportRelation::new("m", "helper"));

        let file_id = build_graph(&mut graph, &ir, "m.py").unwrap();
        assert!(find_node(&graph, NodeType::Module, "helper").is_none());
        let helper = find_node(&graph, NodeType::Function, "helper").unwrap();
        // The Imports edge targets the existing function node.
        let import_edges: Vec<_> = graph
            .get_edges_between(file_id, helper)
            .unwrap()
            .into_iter()
            .filter(|id| graph.get_edge(*id).unwrap().edge_type == EdgeType::Imports)
            .collect();
        assert_eq!(import_edges.len(), 1);
    }
}
