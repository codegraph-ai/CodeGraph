// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Mapper for converting CodeIR to CodeGraph nodes and edges

use codegraph::{CodeGraph, EdgeType, NodeId, NodeType, PropertyMap};
use codegraph_parser_api::{CodeIR, FileInfo, ParserError};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

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
        let name = file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();
        let props = PropertyMap::new()
            .with("name", name.clone())
            .with("path", file_path.display().to_string())
            .with("language", "go");

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
            .with("is_static", func.is_static);

        if let Some(ref doc) = func.doc_comment {
            props = props.with("doc", doc.clone());
        }
        if let Some(ref return_type) = func.return_type {
            props = props.with("return_type", return_type.clone());
        }
        if let Some(ref body) = func.body_prefix {
            props = props.with("body_prefix", body.clone());
        }
        // Detect HTTP handler by signature pattern
        if detect_go_http_handler(&func.signature) {
            props = props
                .with("http_method", "ANY")
                .with("route", "/")
                .with("is_entry_point", true);
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

        // Link function to file
        graph
            .add_edge(file_id, func_id, EdgeType::Contains, PropertyMap::new())
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
    }

    // Add classes/structs
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
        if let Some(ref body) = class.body_prefix {
            props = props.with("body_prefix", body.clone());
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

        // Add methods
        for method in &class.methods {
            let method_name = format!("{}.{}", class.name, method.name);
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
            // Detect HTTP handler by signature pattern
            if detect_go_http_handler(&method.signature) {
                method_props = method_props
                    .with("http_method", "ANY")
                    .with("route", "/")
                    .with("is_entry_point", true);
            }

            let method_id = graph
                .add_node(NodeType::Function, method_props)
                .map_err(|e| ParserError::GraphError(e.to_string()))?;

            node_map.insert(method_name, method_id);
            function_ids.push(method_id);

            // Link method to class
            graph
                .add_edge(class_id, method_id, EdgeType::Contains, PropertyMap::new())
                .map_err(|e| ParserError::GraphError(e.to_string()))?;
        }
    }

    // Add interfaces
    for interface in &ir.traits {
        let mut props = PropertyMap::new()
            .with("name", interface.name.clone())
            .with("path", file_path.display().to_string())
            .with("visibility", interface.visibility.clone())
            .with("line_start", interface.line_start as i64)
            .with("line_end", interface.line_end as i64);

        if let Some(ref doc) = interface.doc_comment {
            props = props.with("doc", doc.clone());
        }

        let trait_id = graph
            .add_node(NodeType::Interface, props)
            .map_err(|e| ParserError::GraphError(e.to_string()))?;

        node_map.insert(interface.name.clone(), trait_id);
        trait_ids.push(trait_id);

        // Link interface to file
        graph
            .add_edge(file_id, trait_id, EdgeType::Contains, PropertyMap::new())
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
    }

    // Add import nodes and relationships
    for import in &ir.imports {
        let imported_module = &import.imported;

        // Create or get import node
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

        // Create import edge from file to imported module
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
                let edge_props = PropertyMap::new()
                    .with("call_site_line", call.call_site_line as i64)
                    .with("is_direct", call.is_direct);

                graph
                    .add_edge(caller_id, callee_id, EdgeType::Calls, edge_props)
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

    // Add implementation relationships (struct implements interface)
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

    // Count source lines
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

/// Detect Go HTTP handler functions by signature pattern.
///
/// Recognizes:
/// - Standard library: `func(w http.ResponseWriter, r *http.Request)`
/// - Gin: `func(c *gin.Context)`
/// - Echo: `func(c echo.Context)`
/// - Fiber: `func(c *fiber.Ctx)`
fn detect_go_http_handler(signature: &str) -> bool {
    let lower = signature.to_lowercase();
    (lower.contains("responsewriter") && lower.contains("request"))
        || (lower.contains("http.responsewriter") && lower.contains("http.request"))
        || lower.contains("gin.context")
        || lower.contains("echo.context")
        || lower.contains("fiber.ctx")
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph_parser_api::{ClassEntity, FunctionEntity, ImportRelation, TraitEntity};
    use std::path::PathBuf;

    #[test]
    fn test_ir_to_graph_empty() {
        let ir = CodeIR::new(PathBuf::from("test.go"));
        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, PathBuf::from("test.go").as_path());

        assert!(result.is_ok());
        let file_info = result.unwrap();
        assert_eq!(file_info.functions.len(), 0);
        assert_eq!(file_info.classes.len(), 0);
        assert_eq!(file_info.traits.len(), 0);
    }

    #[test]
    fn test_ir_to_graph_with_function() {
        let mut ir = CodeIR::new(PathBuf::from("test.go"));
        ir.add_function(FunctionEntity::new("testFunc", 1, 5));

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, PathBuf::from("test.go").as_path());

        assert!(result.is_ok());
        let file_info = result.unwrap();
        assert_eq!(file_info.functions.len(), 1);
    }

    #[test]
    fn test_ir_to_graph_with_struct() {
        let mut ir = CodeIR::new(PathBuf::from("test.go"));
        ir.add_class(ClassEntity::new("Person", 1, 10));

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, PathBuf::from("test.go").as_path());

        assert!(result.is_ok());
        let file_info = result.unwrap();
        assert_eq!(file_info.classes.len(), 1);
    }

    #[test]
    fn test_ir_to_graph_with_interface() {
        let mut ir = CodeIR::new(PathBuf::from("test.go"));
        ir.add_trait(TraitEntity::new("Reader", 1, 5));

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, PathBuf::from("test.go").as_path());

        assert!(result.is_ok());
        let file_info = result.unwrap();
        assert_eq!(file_info.traits.len(), 1);
    }

    #[test]
    fn test_ir_to_graph_with_module() {
        let mut ir = CodeIR::new(PathBuf::from("test.go"));
        ir.set_module(codegraph_parser_api::ModuleEntity::new(
            "main", "test.go", "go",
        ));

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, PathBuf::from("test.go").as_path());

        assert!(result.is_ok());
        let file_info = result.unwrap();
        // File node should be created - just verify we got a valid NodeId
        graph.get_node(file_info.file_id).unwrap();
    }

    #[test]
    fn test_ir_to_graph_with_imports() {
        let mut ir = CodeIR::new(PathBuf::from("test.go"));
        ir.add_import(ImportRelation::new("main", "fmt"));
        ir.add_import(ImportRelation::new("main", "os"));

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, PathBuf::from("test.go").as_path());

        assert!(result.is_ok());
        let file_info = result.unwrap();
        assert_eq!(file_info.imports.len(), 2);
    }

    #[test]
    fn test_ir_to_graph_with_methods() {
        let mut ir = CodeIR::new(PathBuf::from("test.go"));

        let mut class = ClassEntity::new("Calculator", 1, 10);
        class.methods.push(FunctionEntity::new("Add", 2, 4));
        class.methods.push(FunctionEntity::new("Subtract", 5, 7));
        ir.add_class(class);

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, PathBuf::from("test.go").as_path());

        assert!(result.is_ok());
        let file_info = result.unwrap();
        assert_eq!(file_info.classes.len(), 1);
        // Methods are added as function nodes linked to the class
        assert_eq!(file_info.functions.len(), 2);
    }

    #[test]
    fn test_ir_to_graph_function_properties() {
        let mut ir = CodeIR::new(PathBuf::from("test.go"));
        let func = FunctionEntity::new("publicFunc", 1, 5)
            .with_visibility("public")
            .with_signature("func publicFunc() error");
        ir.add_function(func);

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, PathBuf::from("test.go").as_path());

        assert!(result.is_ok());
        let file_info = result.unwrap();
        assert_eq!(file_info.functions.len(), 1);

        // Verify function properties are set
        let func_node = graph.get_node(file_info.functions[0]).unwrap();
        assert_eq!(
            func_node.properties.get("name"),
            Some(&codegraph::PropertyValue::String("publicFunc".to_string()))
        );
        assert_eq!(
            func_node.properties.get("visibility"),
            Some(&codegraph::PropertyValue::String("public".to_string()))
        );
    }

    #[test]
    fn test_ir_to_graph_with_implementation() {
        use codegraph::EdgeType;
        use codegraph_parser_api::ImplementationRelation;

        let mut ir = CodeIR::new(PathBuf::from("test.go"));
        ir.add_class(ClassEntity::new("FileReader", 1, 20));
        ir.add_trait(TraitEntity::new("Reader", 22, 30));
        ir.add_implementation(ImplementationRelation::new("FileReader", "Reader"));

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, PathBuf::from("test.go").as_path());

        assert!(result.is_ok());
        let file_info = result.unwrap();
        assert_eq!(file_info.classes.len(), 1);
        assert_eq!(file_info.traits.len(), 1);

        // Find struct and interface node IDs
        let struct_id = file_info.classes[0];
        let interface_id = file_info.traits[0];

        // Verify implements edge was created
        let edges = graph.get_edges_between(struct_id, interface_id).unwrap();
        assert!(
            !edges.is_empty(),
            "Should have implements edge between struct and interface"
        );

        let edge = graph.get_edge(edges[0]).unwrap();
        assert_eq!(
            edge.edge_type,
            EdgeType::Implements,
            "Edge should be of type Implements"
        );
    }

    #[test]
    fn test_property_types() {
        use codegraph::PropertyValue;
        use codegraph_parser_api::{FunctionEntity, ModuleEntity};

        let mut ir = CodeIR::default();
        ir.set_module(ModuleEntity::new("test", "test.go", "go").with_line_count(100));
        let func = FunctionEntity::new("test_fn", 10, 20).async_fn();
        ir.add_function(func);

        let mut graph = CodeGraph::in_memory().unwrap();
        let file_info = ir_to_graph(&ir, &mut graph, std::path::Path::new("test.go")).unwrap();

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
