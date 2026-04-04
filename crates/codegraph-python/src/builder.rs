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
    use codegraph_parser_api::{CallRelation, ClassEntity, FunctionEntity, ImportRelation};

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
}
