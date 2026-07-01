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
    use codegraph_parser_api::{
        CallRelation, ClassEntity, ComplexityMetrics, FunctionEntity, ImportRelation, TraitEntity,
    };
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

    // ---- detect_go_http_handler (pure) ----

    #[test]
    fn test_detect_go_http_handler_stdlib() {
        assert!(detect_go_http_handler(
            "func h(w http.ResponseWriter, r *http.Request)"
        ));
    }

    #[test]
    fn test_detect_go_http_handler_frameworks() {
        assert!(detect_go_http_handler("func h(c *gin.Context)"));
        assert!(detect_go_http_handler("func h(c echo.Context)"));
        assert!(detect_go_http_handler("func h(c *fiber.Ctx)"));
    }

    #[test]
    fn test_detect_go_http_handler_case_insensitive() {
        // Lowercasing means an all-caps signature still matches.
        assert!(detect_go_http_handler(
            "FUNC H(W HTTP.RESPONSEWRITER, R *HTTP.REQUEST)"
        ));
    }

    #[test]
    fn test_detect_go_http_handler_negative() {
        assert!(!detect_go_http_handler("func add(a int, b int) int"));
        // ResponseWriter alone without Request is not enough.
        assert!(!detect_go_http_handler("func h(w http.ResponseWriter)"));
    }

    #[test]
    fn test_function_http_handler_props_stamped() {
        let mut ir = CodeIR::new(PathBuf::from("h.go"));
        ir.add_function(
            FunctionEntity::new("Serve", 1, 5)
                .with_signature("func Serve(w http.ResponseWriter, r *http.Request)"),
        );

        let mut graph = CodeGraph::in_memory().unwrap();
        let fi = ir_to_graph(&ir, &mut graph, Path::new("h.go")).unwrap();

        let node = graph.get_node(fi.functions[0]).unwrap();
        assert_eq!(
            node.properties.get("http_method"),
            Some(&codegraph::PropertyValue::String("ANY".to_string()))
        );
        assert_eq!(
            node.properties.get("route"),
            Some(&codegraph::PropertyValue::String("/".to_string()))
        );
        assert_eq!(
            node.properties.get("is_entry_point"),
            Some(&codegraph::PropertyValue::Bool(true))
        );
    }

    // ---- file node fallback / module doc ----

    #[test]
    fn test_file_stem_fallback_no_module() {
        let ir = CodeIR::new(PathBuf::from("handlers.go"));
        let mut graph = CodeGraph::in_memory().unwrap();
        let fi = ir_to_graph(&ir, &mut graph, Path::new("pkg/handlers.go")).unwrap();

        let node = graph.get_node(fi.file_id).unwrap();
        assert_eq!(
            node.properties.get("name"),
            Some(&codegraph::PropertyValue::String("handlers".to_string()))
        );
        assert_eq!(
            node.properties.get("language"),
            Some(&codegraph::PropertyValue::String("go".to_string()))
        );
    }

    #[test]
    fn test_module_doc_prop() {
        let mut ir = CodeIR::new(PathBuf::from("test.go"));
        // The mapper stamps a module's doc_comment onto the file node's `doc` prop.
        ir.set_module(
            codegraph_parser_api::ModuleEntity::new("main", "test.go", "go")
                .with_doc("package docs"),
        );

        let mut graph = CodeGraph::in_memory().unwrap();
        let fi = ir_to_graph(&ir, &mut graph, Path::new("test.go")).unwrap();

        let node = graph.get_node(fi.file_id).unwrap();
        assert_eq!(
            node.properties.get("doc"),
            Some(&codegraph::PropertyValue::String(
                "package docs".to_string()
            ))
        );
    }

    // ---- function optional / complexity props ----

    #[test]
    fn test_function_optional_props_present() {
        let mut ir = CodeIR::new(PathBuf::from("test.go"));
        ir.add_function(
            FunctionEntity::new("f", 1, 5)
                .with_doc("does f")
                .with_return_type("error")
                .with_body_prefix("return nil"),
        );

        let mut graph = CodeGraph::in_memory().unwrap();
        let fi = ir_to_graph(&ir, &mut graph, Path::new("test.go")).unwrap();

        let node = graph.get_node(fi.functions[0]).unwrap();
        assert_eq!(
            node.properties.get("doc"),
            Some(&codegraph::PropertyValue::String("does f".to_string()))
        );
        assert_eq!(
            node.properties.get("return_type"),
            Some(&codegraph::PropertyValue::String("error".to_string()))
        );
        assert_eq!(
            node.properties.get("body_prefix"),
            Some(&codegraph::PropertyValue::String("return nil".to_string()))
        );
    }

    #[test]
    fn test_function_optional_props_absent() {
        let mut ir = CodeIR::new(PathBuf::from("test.go"));
        ir.add_function(FunctionEntity::new("f", 1, 5));

        let mut graph = CodeGraph::in_memory().unwrap();
        let fi = ir_to_graph(&ir, &mut graph, Path::new("test.go")).unwrap();

        let node = graph.get_node(fi.functions[0]).unwrap();
        assert!(node.properties.get("doc").is_none());
        assert!(node.properties.get("return_type").is_none());
        assert!(node.properties.get("body_prefix").is_none());
        assert!(node.properties.get("complexity").is_none());
    }

    #[test]
    fn test_function_complexity_all_props() {
        let mut ir = CodeIR::new(PathBuf::from("test.go"));
        let metrics = ComplexityMetrics {
            cyclomatic_complexity: 12,
            branches: 3,
            loops: 2,
            logical_operators: 4,
            max_nesting_depth: 5,
            exception_handlers: 1,
            early_returns: 2,
        };
        ir.add_function(FunctionEntity::new("f", 1, 20).with_complexity(metrics));

        let mut graph = CodeGraph::in_memory().unwrap();
        let fi = ir_to_graph(&ir, &mut graph, Path::new("test.go")).unwrap();

        let p = &graph.get_node(fi.functions[0]).unwrap().properties;
        use codegraph::PropertyValue::{Int, String as S};
        assert_eq!(p.get("complexity"), Some(&Int(12)));
        assert_eq!(p.get("complexity_grade"), Some(&S("C".to_string())));
        assert_eq!(p.get("complexity_branches"), Some(&Int(3)));
        assert_eq!(p.get("complexity_loops"), Some(&Int(2)));
        assert_eq!(p.get("complexity_logical_ops"), Some(&Int(4)));
        assert_eq!(p.get("complexity_nesting"), Some(&Int(5)));
        assert_eq!(p.get("complexity_exceptions"), Some(&Int(1)));
        assert_eq!(p.get("complexity_early_returns"), Some(&Int(2)));
    }

    // ---- class / method / interface props ----

    #[test]
    fn test_class_optional_props() {
        let mut ir = CodeIR::new(PathBuf::from("test.go"));
        ir.add_class(
            ClassEntity::new("Base", 1, 10)
                .with_visibility("public")
                .abstract_class()
                .with_doc("base struct")
                .with_body_prefix("type Base struct {"),
        );

        let mut graph = CodeGraph::in_memory().unwrap();
        let fi = ir_to_graph(&ir, &mut graph, Path::new("test.go")).unwrap();

        let p = &graph.get_node(fi.classes[0]).unwrap().properties;
        use codegraph::PropertyValue::{Bool, String as S};
        assert_eq!(p.get("visibility"), Some(&S("public".to_string())));
        assert_eq!(p.get("is_abstract"), Some(&Bool(true)));
        assert_eq!(p.get("doc"), Some(&S("base struct".to_string())));
        assert_eq!(
            p.get("body_prefix"),
            Some(&S("type Base struct {".to_string()))
        );
    }

    #[test]
    fn test_method_qualified_name_and_props() {
        let mut ir = CodeIR::new(PathBuf::from("test.go"));
        let mut class = ClassEntity::new("Calc", 1, 10);
        class.methods.push(
            FunctionEntity::new("Add", 2, 4)
                .with_doc("adds")
                .with_body_prefix("return a + b"),
        );
        ir.add_class(class);

        let mut graph = CodeGraph::in_memory().unwrap();
        let fi = ir_to_graph(&ir, &mut graph, Path::new("test.go")).unwrap();

        let p = &graph.get_node(fi.functions[0]).unwrap().properties;
        use codegraph::PropertyValue::String as S;
        assert_eq!(p.get("name"), Some(&S("Calc.Add".to_string())));
        assert_eq!(p.get("is_method"), Some(&S("true".to_string())));
        assert_eq!(p.get("parent_class"), Some(&S("Calc".to_string())));
        assert_eq!(p.get("doc"), Some(&S("adds".to_string())));
        assert_eq!(p.get("body_prefix"), Some(&S("return a + b".to_string())));

        // Method is contained by the class node, not the file node.
        let edges = graph
            .get_edges_between(fi.classes[0], fi.functions[0])
            .unwrap();
        assert!(!edges.is_empty());
        assert_eq!(
            graph.get_edge(edges[0]).unwrap().edge_type,
            EdgeType::Contains
        );
    }

    #[test]
    fn test_method_http_handler_detected() {
        let mut ir = CodeIR::new(PathBuf::from("test.go"));
        let mut class = ClassEntity::new("Server", 1, 10);
        class.methods.push(
            FunctionEntity::new("Handle", 2, 4)
                .with_signature("func (s *Server) Handle(c *gin.Context)"),
        );
        ir.add_class(class);

        let mut graph = CodeGraph::in_memory().unwrap();
        let fi = ir_to_graph(&ir, &mut graph, Path::new("test.go")).unwrap();

        let p = &graph.get_node(fi.functions[0]).unwrap().properties;
        assert_eq!(
            p.get("is_entry_point"),
            Some(&codegraph::PropertyValue::Bool(true))
        );
    }

    #[test]
    fn test_interface_props_and_containment() {
        let mut ir = CodeIR::new(PathBuf::from("test.go"));
        ir.add_trait(
            TraitEntity::new("Reader", 3, 8)
                .with_visibility("public")
                .with_doc("reads bytes"),
        );

        let mut graph = CodeGraph::in_memory().unwrap();
        let fi = ir_to_graph(&ir, &mut graph, Path::new("test.go")).unwrap();

        let p = &graph.get_node(fi.traits[0]).unwrap().properties;
        use codegraph::PropertyValue::{Int, String as S};
        assert_eq!(p.get("visibility"), Some(&S("public".to_string())));
        assert_eq!(p.get("doc"), Some(&S("reads bytes".to_string())));
        assert_eq!(p.get("line_start"), Some(&Int(3)));
        assert_eq!(p.get("line_end"), Some(&Int(8)));

        let edges = graph.get_edges_between(fi.file_id, fi.traits[0]).unwrap();
        assert_eq!(
            graph.get_edge(edges[0]).unwrap().edge_type,
            EdgeType::Contains
        );
    }

    // ---- import edge props / reuse ----

    #[test]
    fn test_import_edge_props() {
        let mut ir = CodeIR::new(PathBuf::from("test.go"));
        ir.add_import(
            ImportRelation::new("main", "encoding/json")
                .with_alias("j")
                .with_symbols(vec!["Marshal".to_string(), "Unmarshal".to_string()])
                .wildcard(),
        );

        let mut graph = CodeGraph::in_memory().unwrap();
        let fi = ir_to_graph(&ir, &mut graph, Path::new("test.go")).unwrap();

        let edges = graph.get_edges_between(fi.file_id, fi.imports[0]).unwrap();
        let edge = graph.get_edge(edges[0]).unwrap();
        assert_eq!(edge.edge_type, EdgeType::Imports);
        use codegraph::PropertyValue::{String as S, StringList};
        assert_eq!(edge.properties.get("alias"), Some(&S("j".to_string())));
        assert_eq!(
            edge.properties.get("is_wildcard"),
            Some(&S("true".to_string()))
        );
        assert_eq!(
            edge.properties.get("symbols"),
            Some(&StringList(vec![
                "Marshal".to_string(),
                "Unmarshal".to_string()
            ]))
        );

        // External module node is marked is_external.
        assert_eq!(
            graph
                .get_node(fi.imports[0])
                .unwrap()
                .properties
                .get("is_external"),
            Some(&S("true".to_string()))
        );
    }

    #[test]
    fn test_import_reuses_in_file_node() {
        let mut ir = CodeIR::new(PathBuf::from("test.go"));
        // A function named "helper" then an import of the same name reuses the node.
        ir.add_function(FunctionEntity::new("helper", 1, 3));
        ir.add_import(ImportRelation::new("main", "helper"));

        let mut graph = CodeGraph::in_memory().unwrap();
        let fi = ir_to_graph(&ir, &mut graph, Path::new("test.go")).unwrap();

        // Import id equals the existing function node id (reused, not a new Module).
        assert_eq!(fi.imports[0], fi.functions[0]);
        let node = graph.get_node(fi.imports[0]).unwrap();
        // Reused Function node is NOT stamped is_external.
        assert!(node.properties.get("is_external").is_none());
        assert_eq!(node.node_type, NodeType::Function);
    }

    // ---- calls: direct + unresolved storage ----

    #[test]
    fn test_call_direct_edge() {
        let mut ir = CodeIR::new(PathBuf::from("test.go"));
        ir.add_function(FunctionEntity::new("caller", 1, 3));
        ir.add_function(FunctionEntity::new("callee", 5, 7));
        ir.add_call(CallRelation::new("caller", "callee", 2));

        let mut graph = CodeGraph::in_memory().unwrap();
        let fi = ir_to_graph(&ir, &mut graph, Path::new("test.go")).unwrap();

        let edges = graph
            .get_edges_between(fi.functions[0], fi.functions[1])
            .unwrap();
        let edge = graph.get_edge(edges[0]).unwrap();
        assert_eq!(edge.edge_type, EdgeType::Calls);
        assert_eq!(
            edge.properties.get("call_site_line"),
            Some(&codegraph::PropertyValue::Int(2))
        );
        assert_eq!(
            edge.properties.get("is_direct"),
            Some(&codegraph::PropertyValue::Bool(true))
        );
    }

    #[test]
    fn test_unresolved_calls_stored_and_deduped() {
        let mut ir = CodeIR::new(PathBuf::from("test.go"));
        ir.add_function(FunctionEntity::new("caller", 1, 3));
        // callee is not defined in this file -> stored as unresolved (twice -> deduped).
        ir.add_call(CallRelation::new("caller", "external", 2));
        ir.add_call(CallRelation::new("caller", "external", 4));

        let mut graph = CodeGraph::in_memory().unwrap();
        let fi = ir_to_graph(&ir, &mut graph, Path::new("test.go")).unwrap();

        let node = graph.get_node(fi.functions[0]).unwrap();
        let unresolved = node
            .properties
            .get_string_list_compat("unresolved_calls")
            .unwrap();
        assert_eq!(unresolved, vec!["external".to_string()]);
    }

    // ---- type references + inheritance ----

    #[test]
    fn test_type_reference_edge() {
        use codegraph_parser_api::TypeReference;
        let mut ir = CodeIR::new(PathBuf::from("test.go"));
        ir.add_function(FunctionEntity::new("useit", 1, 3));
        ir.add_class(ClassEntity::new("Config", 5, 10));
        ir.add_type_reference(TypeReference::new("useit", "Config", 2));

        let mut graph = CodeGraph::in_memory().unwrap();
        let fi = ir_to_graph(&ir, &mut graph, Path::new("test.go")).unwrap();

        let edges = graph
            .get_edges_between(fi.functions[0], fi.classes[0])
            .unwrap();
        assert_eq!(
            graph.get_edge(edges[0]).unwrap().edge_type,
            EdgeType::References
        );
    }

    #[test]
    fn test_unresolved_type_refs_stored() {
        use codegraph_parser_api::TypeReference;
        let mut ir = CodeIR::new(PathBuf::from("test.go"));
        ir.add_function(FunctionEntity::new("useit", 1, 3));
        ir.add_type_reference(TypeReference::new("useit", "ExternalType", 2));

        let mut graph = CodeGraph::in_memory().unwrap();
        let fi = ir_to_graph(&ir, &mut graph, Path::new("test.go")).unwrap();

        let node = graph.get_node(fi.functions[0]).unwrap();
        let refs = node
            .properties
            .get_string_list_compat("unresolved_type_refs")
            .unwrap();
        assert_eq!(refs, vec!["ExternalType".to_string()]);
    }

    #[test]
    fn test_inheritance_extends_edge() {
        use codegraph_parser_api::InheritanceRelation;
        let mut ir = CodeIR::new(PathBuf::from("test.go"));
        ir.add_class(ClassEntity::new("Derived", 1, 5));
        ir.add_class(ClassEntity::new("Base", 7, 10));
        ir.add_inheritance(InheritanceRelation::new("Derived", "Base").with_order(0));

        let mut graph = CodeGraph::in_memory().unwrap();
        let fi = ir_to_graph(&ir, &mut graph, Path::new("test.go")).unwrap();

        let edges = graph
            .get_edges_between(fi.classes[0], fi.classes[1])
            .unwrap();
        let edge = graph.get_edge(edges[0]).unwrap();
        assert_eq!(edge.edge_type, EdgeType::Extends);
        assert_eq!(
            edge.properties.get("order"),
            Some(&codegraph::PropertyValue::Int(0))
        );
    }
}
