// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Mapper for converting CodeIR to CodeGraph nodes and edges
//!
//! This module handles the conversion of the intermediate representation (IR)
//! into actual graph nodes and edges in the CodeGraph database.

use codegraph::{CodeGraph, EdgeType, NodeId, NodeType, PropertyMap};
use codegraph_parser_api::{CodeIR, FileInfo, ParserError};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

/// Convert CodeIR to graph nodes and edges, returning FileInfo
pub fn ir_to_graph(
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
        // Create a default file node
        let file_name = file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();
        let props = PropertyMap::new()
            .with("name", file_name.clone())
            .with("path", file_path.display().to_string())
            .with("language", "rust");

        let id = graph
            .add_node(NodeType::CodeFile, props)
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
        node_map.insert(file_name, id);
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

        if let Some(ref doc) = func.doc_comment {
            props = props.with("doc", doc.clone());
        }
        if let Some(ref return_type) = func.return_type {
            props = props.with("return_type", return_type.clone());
        }
        if let Some(ref body) = func.body_prefix {
            props = props.with("body_prefix", body.clone());
        }

        let func_id = graph
            .add_node(NodeType::Function, props)
            .map_err(|e| ParserError::GraphError(e.to_string()))?;

        node_map.insert(func.name.clone(), func_id);
        function_ids.push(func_id);

        // Link function to file or parent class
        if let Some(ref parent_class) = func.parent_class {
            // This is a method - link to class if it exists
            if let Some(&class_id) = node_map.get(parent_class) {
                graph
                    .add_edge(class_id, func_id, EdgeType::Contains, PropertyMap::new())
                    .map_err(|e| ParserError::GraphError(e.to_string()))?;
            } else {
                // Parent class not yet in map, link to file for now
                graph
                    .add_edge(file_id, func_id, EdgeType::Contains, PropertyMap::new())
                    .map_err(|e| ParserError::GraphError(e.to_string()))?;
            }
        } else {
            // Top-level function - link to file
            graph
                .add_edge(file_id, func_id, EdgeType::Contains, PropertyMap::new())
                .map_err(|e| ParserError::GraphError(e.to_string()))?;
        }
    }

    // Add classes (structs/enums)
    for class in &ir.classes {
        let mut props = PropertyMap::new()
            .with("name", class.name.clone())
            .with("path", file_path.display().to_string())
            .with("visibility", class.visibility.clone())
            .with("line_start", class.line_start as i64)
            .with("line_end", class.line_end as i64)
            .with("is_abstract", class.is_abstract)
            .with("is_interface", class.is_interface);

        if let Some(ref doc) = class.doc_comment {
            props = props.with("doc", doc.clone());
        }
        if let Some(ref body) = class.body_prefix {
            props = props.with("body_prefix", body.clone());
        }

        if !class.type_parameters.is_empty() {
            props = props.with("type_parameters", class.type_parameters.join(", "));
        }

        let class_id = graph
            .add_node(NodeType::Class, props)
            .map_err(|e| ParserError::GraphError(e.to_string()))?;

        node_map.insert(class.name.clone(), class_id);
        class_ids.push(class_id);

        // Link class to file
        graph
            .add_edge(file_id, class_id, EdgeType::Contains, PropertyMap::new())
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
    }

    // Add traits
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

        // Link trait to file
        graph
            .add_edge(file_id, trait_id, EdgeType::Contains, PropertyMap::new())
            .map_err(|e| ParserError::GraphError(e.to_string()))?;

        // Add parent trait relationships
        for parent in &trait_entity.parent_traits {
            if let Some(&parent_id) = node_map.get(parent) {
                graph
                    .add_edge(trait_id, parent_id, EdgeType::Extends, PropertyMap::new())
                    .map_err(|e| ParserError::GraphError(e.to_string()))?;
            }
        }
    }

    // Add import nodes and relationships
    for import in &ir.imports {
        let imported_module = &import.imported;
        let is_mod = import.importer == "mod_declaration";

        // Create or get module node
        let import_id = if let Some(&existing_id) = node_map.get(imported_module) {
            existing_id
        } else {
            // Determine if this is an external or internal module
            // mod declarations always reference local files
            let is_external = if is_mod {
                false
            } else {
                !imported_module.starts_with("super::")
                    && !imported_module.starts_with("crate::")
                    && !imported_module.starts_with("self::")
            };

            let props = PropertyMap::new()
                .with("name", imported_module.clone())
                .with("is_external", is_external)
                .with("is_mod_declaration", is_mod);

            let id = graph
                .add_node(NodeType::Module, props)
                .map_err(|e| ParserError::GraphError(e.to_string()))?;
            node_map.insert(imported_module.clone(), id);
            id
        };

        import_ids.push(import_id);

        // Create import edge from file to module
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
    // Track unresolved calls per caller for cross-file resolution
    let mut unresolved_calls: HashMap<String, Vec<String>> = HashMap::new();

    for call in &ir.calls {
        if let Some(&caller_id) = node_map.get(&call.caller) {
            if let Some(&callee_id) = node_map.get(&call.callee) {
                // Both caller and callee are in this file - create direct edge
                graph
                    .add_edge(caller_id, callee_id, EdgeType::Calls, PropertyMap::new())
                    .map_err(|e| ParserError::GraphError(e.to_string()))?;
            } else {
                // Callee not found in this file - store for cross-file resolution
                unresolved_calls
                    .entry(call.caller.clone())
                    .or_default()
                    .push(call.callee.clone());
            }
        }
    }

    // Store unresolved calls on caller nodes for post-processing
    for (caller_name, callees) in unresolved_calls {
        if let Some(&caller_id) = node_map.get(&caller_name) {
            if let Ok(node) = graph.get_node(caller_id) {
                let mut all_callees: Vec<String> = node
                    .properties
                    .get_string_list_compat("unresolved_calls")
                    .unwrap_or_default();
                for callee in &callees {
                    if !all_callees.iter().any(|c| c == callee) {
                        all_callees.push(callee.clone());
                    }
                }
                let new_props = node
                    .properties
                    .clone()
                    .with("unresolved_calls", all_callees);
                let _ = graph.update_node_properties(caller_id, new_props);
            }
        }
    }

    // Add type reference relationships (creates References edges)
    let mut unresolved_type_refs: HashMap<String, Vec<String>> = HashMap::new();

    for type_ref in &ir.type_references {
        if let Some(&referrer_id) = node_map.get(&type_ref.referrer) {
            if let Some(&type_id) = node_map.get(&type_ref.type_name) {
                let _ = graph.add_edge(
                    referrer_id,
                    type_id,
                    EdgeType::References,
                    PropertyMap::new(),
                );
            } else {
                unresolved_type_refs
                    .entry(type_ref.referrer.clone())
                    .or_default()
                    .push(type_ref.type_name.clone());
            }
        }
    }

    for (referrer_name, types) in unresolved_type_refs {
        if let Some(&referrer_id) = node_map.get(&referrer_name) {
            if let Ok(node) = graph.get_node(referrer_id) {
                let mut all: Vec<String> = node
                    .properties
                    .get_string_list_compat("unresolved_type_refs")
                    .unwrap_or_default();
                for t in &types {
                    if !all.iter().any(|existing| existing == t) {
                        all.push(t.clone());
                    }
                }
                let new_props = node.properties.clone().with("unresolved_type_refs", all);
                let _ = graph.update_node_properties(referrer_id, new_props);
            }
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

    // Add inheritance relationships
    for inheritance in &ir.inheritance {
        if let (Some(&child_id), Some(&parent_id)) = (
            node_map.get(&inheritance.child),
            node_map.get(&inheritance.parent),
        ) {
            graph
                .add_edge(child_id, parent_id, EdgeType::Extends, PropertyMap::new())
                .map_err(|e| ParserError::GraphError(e.to_string()))?;
        }
    }

    Ok(FileInfo {
        file_path: file_path.to_path_buf(),
        file_id,
        functions: function_ids,
        classes: class_ids,
        traits: trait_ids,
        imports: import_ids,
        parse_time: Duration::ZERO, // Will be set by caller
        line_count: 0,              // Will be set by caller
        byte_count: 0,              // Will be set by caller
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph_parser_api::{FunctionEntity, ModuleEntity};

    #[test]
    fn test_ir_to_graph_basic() {
        let mut graph = CodeGraph::in_memory().unwrap();
        let mut ir = CodeIR::new(std::path::PathBuf::from("test.rs"));

        ir.module = Some(ModuleEntity {
            name: "test".to_string(),
            path: "test.rs".to_string(),
            language: "rust".to_string(),
            line_count: 10,
            doc_comment: None,
            attributes: Vec::new(),
        });

        ir.functions.push(FunctionEntity {
            name: "hello".to_string(),
            signature: "fn hello()".to_string(),
            visibility: "public".to_string(),
            line_start: 1,
            line_end: 3,
            is_async: false,
            is_test: false,
            is_static: false,
            is_abstract: false,
            parameters: Vec::new(),
            return_type: None,
            doc_comment: None,
            attributes: Vec::new(),
            parent_class: None,
            complexity: None,
            body_prefix: None,
        });

        let result = ir_to_graph(&ir, &mut graph, Path::new("test.rs"));
        assert!(result.is_ok());

        let info = result.unwrap();
        assert_eq!(info.functions.len(), 1);
    }

    #[test]
    fn test_ir_to_graph_empty() {
        let ir = CodeIR::new(std::path::PathBuf::from("test.rs"));
        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, Path::new("test.rs"));

        assert!(result.is_ok());
        let file_info = result.unwrap();
        assert_eq!(file_info.functions.len(), 0);
        assert_eq!(file_info.classes.len(), 0);
        assert_eq!(file_info.traits.len(), 0);
    }

    #[test]
    fn test_ir_to_graph_with_function() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("test.rs"));
        ir.add_function(FunctionEntity::new("test_fn", 1, 5));

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, Path::new("test.rs"));

        assert!(result.is_ok());
        let file_info = result.unwrap();
        assert_eq!(file_info.functions.len(), 1);
    }

    #[test]
    fn test_ir_to_graph_with_struct() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("test.rs"));
        ir.add_class(codegraph_parser_api::ClassEntity::new("MyStruct", 1, 10));

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, Path::new("test.rs"));

        assert!(result.is_ok());
        let file_info = result.unwrap();
        assert_eq!(file_info.classes.len(), 1);
    }

    #[test]
    fn test_ir_to_graph_with_trait() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("test.rs"));
        ir.add_trait(codegraph_parser_api::TraitEntity::new("MyTrait", 1, 5));

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, Path::new("test.rs"));

        assert!(result.is_ok());
        let file_info = result.unwrap();
        assert_eq!(file_info.traits.len(), 1);
    }

    #[test]
    fn test_ir_to_graph_with_module() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("test.rs"));
        ir.set_module(ModuleEntity::new("test", "test.rs", "rust"));

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, Path::new("test.rs"));

        assert!(result.is_ok());
        let file_info = result.unwrap();
        // File node should be created - verify we got a valid NodeId
        graph.get_node(file_info.file_id).unwrap();
    }

    #[test]
    fn test_ir_to_graph_with_imports() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("test.rs"));
        ir.add_import(codegraph_parser_api::ImportRelation::new(
            "test",
            "std::collections",
        ));
        ir.add_import(codegraph_parser_api::ImportRelation::new("test", "std::io"));

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, Path::new("test.rs"));

        assert!(result.is_ok());
        // Note: Import processing not yet implemented in mapper
        // When implemented, this should be: assert_eq!(file_info.imports.len(), 2);
    }

    #[test]
    fn test_ir_to_graph_with_methods() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("test.rs"));

        let mut class = codegraph_parser_api::ClassEntity::new("MyStruct", 1, 15);
        class.methods.push(FunctionEntity::new("new", 2, 5));
        class.methods.push(FunctionEntity::new("method", 7, 10));
        ir.add_class(class);

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, Path::new("test.rs"));

        assert!(result.is_ok());
        let file_info = result.unwrap();
        assert_eq!(file_info.classes.len(), 1);
        // Note: Method processing not yet implemented in mapper
        // When implemented, methods should be added as function nodes linked to the class
    }

    #[test]
    fn test_ir_to_graph_async_function() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("test.rs"));
        let func = FunctionEntity::new("async_fn", 1, 5).async_fn();
        ir.add_function(func);

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, Path::new("test.rs"));

        assert!(result.is_ok());
        let file_info = result.unwrap();
        assert_eq!(file_info.functions.len(), 1);

        // Verify the function has is_async property
        let func_node = graph.get_node(file_info.functions[0]).unwrap();
        assert_eq!(
            func_node.properties.get("is_async"),
            Some(&codegraph::PropertyValue::Bool(true))
        );
    }

    #[test]
    fn test_ir_to_graph_function_properties() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("test.rs"));
        let func = FunctionEntity::new("public_fn", 1, 5)
            .with_visibility("public")
            .with_signature("pub fn public_fn() -> i32");
        ir.add_function(func);

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, Path::new("test.rs"));

        assert!(result.is_ok());
        let file_info = result.unwrap();
        assert_eq!(file_info.functions.len(), 1);

        // Verify function properties are set
        let func_node = graph.get_node(file_info.functions[0]).unwrap();
        assert_eq!(
            func_node.properties.get("name"),
            Some(&codegraph::PropertyValue::String("public_fn".to_string()))
        );
        assert_eq!(
            func_node.properties.get("visibility"),
            Some(&codegraph::PropertyValue::String("public".to_string()))
        );
    }

    #[test]
    fn test_ir_to_graph_trait_implementation() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("test.rs"));

        ir.add_trait(codegraph_parser_api::TraitEntity::new("Display", 1, 3));
        let mut class = codegraph_parser_api::ClassEntity::new("Item", 5, 10);
        class.implemented_traits.push("Display".to_string());
        ir.add_class(class);
        ir.add_implementation(codegraph_parser_api::ImplementationRelation::new(
            "Item", "Display",
        ));

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, Path::new("test.rs"));

        assert!(result.is_ok());
        let file_info = result.unwrap();
        assert_eq!(file_info.traits.len(), 1);
        assert_eq!(file_info.classes.len(), 1);
    }

    #[test]
    fn test_type_refs_resolved_creates_references_edge() {
        use codegraph::EdgeType;
        use codegraph_parser_api::TypeReference;

        let mut ir = CodeIR::new(std::path::PathBuf::from("test.rs"));
        ir.add_function(FunctionEntity::new("process", 1, 5));
        ir.add_class(codegraph_parser_api::ClassEntity::new("Config", 6, 10));
        ir.add_type_reference(TypeReference::new("process", "Config", 2));

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, Path::new("test.rs"));
        assert!(result.is_ok());

        let file_info = result.unwrap();
        let process_id = file_info.functions[0];
        let config_id = file_info.classes[0];

        // Verify References edge exists from process -> Config
        let edge_ids = graph.get_edges_between(process_id, config_id).unwrap();
        assert!(
            edge_ids.iter().any(|&eid| {
                graph
                    .get_edge(eid)
                    .map(|e| e.edge_type == EdgeType::References)
                    .unwrap_or(false)
            }),
            "Expected References edge from process to Config"
        );
    }

    #[test]
    fn test_type_refs_unresolved_stored_as_property() {
        use codegraph_parser_api::TypeReference;

        let mut ir = CodeIR::new(std::path::PathBuf::from("test.rs"));
        ir.add_function(FunctionEntity::new("handler", 1, 5));
        // ExternalType is not in this IR (it's from another file)
        ir.add_type_reference(TypeReference::new("handler", "ExternalType", 2));

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, Path::new("test.rs"));
        assert!(result.is_ok());

        let file_info = result.unwrap();
        let handler_id = file_info.functions[0];

        let node = graph.get_node(handler_id).unwrap();
        let unresolved = node
            .properties
            .get_string_list_compat("unresolved_type_refs")
            .unwrap_or_default();
        assert!(
            unresolved.contains(&"ExternalType".to_string()),
            "ExternalType should be stored as unresolved type ref, got: {:?}",
            unresolved
        );
    }

    #[test]
    fn test_type_refs_dedup_unresolved() {
        use codegraph_parser_api::TypeReference;

        let mut ir = CodeIR::new(std::path::PathBuf::from("test.rs"));
        ir.add_function(FunctionEntity::new("func", 1, 5));
        // Same type referenced twice (params + return)
        ir.add_type_reference(TypeReference::new("func", "Token", 2));
        ir.add_type_reference(TypeReference::new("func", "Token", 3));

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, Path::new("test.rs"));
        assert!(result.is_ok());

        let file_info = result.unwrap();
        let func_id = file_info.functions[0];
        let node = graph.get_node(func_id).unwrap();
        let unresolved = node
            .properties
            .get_string_list_compat("unresolved_type_refs")
            .unwrap_or_default();

        assert_eq!(
            unresolved.iter().filter(|t| t.as_str() == "Token").count(),
            1,
            "Token should appear only once after dedup, got: {:?}",
            unresolved
        );
    }

    #[test]
    fn test_property_types() {
        use codegraph::PropertyValue;
        let mut ir = CodeIR::new(std::path::PathBuf::from("test.rs"));
        ir.module = Some(
            codegraph_parser_api::ModuleEntity::new("test", "test.rs", "rust").with_line_count(100),
        );
        let func = codegraph_parser_api::FunctionEntity::new("test_fn", 10, 20)
            .with_signature("fn test_fn()")
            .with_visibility("public")
            .async_fn();
        ir.functions.push(func);

        let mut graph = CodeGraph::in_memory().unwrap();
        let file_info = ir_to_graph(&ir, &mut graph, std::path::Path::new("test.rs")).unwrap();

        // Verify file node line_count is Int
        let file_node = graph.get_node(file_info.file_id).unwrap();
        assert!(
            matches!(
                file_node.properties.get("line_count"),
                Some(PropertyValue::Int(100))
            ),
            "line_count should be Int, got {:?}",
            file_node.properties.get("line_count")
        );

        // Verify function properties are correct types
        let func_node = graph.get_node(file_info.functions[0]).unwrap();
        assert!(
            matches!(
                func_node.properties.get("line_start"),
                Some(PropertyValue::Int(10))
            ),
            "line_start should be Int(10), got {:?}",
            func_node.properties.get("line_start")
        );
        assert!(
            matches!(
                func_node.properties.get("line_end"),
                Some(PropertyValue::Int(20))
            ),
            "line_end should be Int(20), got {:?}",
            func_node.properties.get("line_end")
        );
        assert!(
            matches!(
                func_node.properties.get("is_async"),
                Some(PropertyValue::Bool(true))
            ),
            "is_async should be Bool(true), got {:?}",
            func_node.properties.get("is_async")
        );
    }
}
