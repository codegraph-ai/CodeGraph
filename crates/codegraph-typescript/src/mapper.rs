// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Mapper for converting CodeIR to CodeGraph nodes and edges

use codegraph::{CodeGraph, EdgeType, NodeId, NodeType, PropertyMap};
use codegraph_parser_api::{CodeIR, FileInfo, ParserError};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Resolve a relative import path to an absolute path
///
/// Given a file path like `/src/extension.ts` and an import like `./toolManager`,
/// returns `/src/toolManager` (without extension, as TypeScript imports don't include it)
fn resolve_import_path(importing_file: &Path, import_path: &str) -> Option<PathBuf> {
    // Skip external/package imports (don't start with . or /)
    if !import_path.starts_with('.') && !import_path.starts_with('/') {
        return None;
    }

    // Get the directory containing the importing file
    let parent_dir = importing_file.parent()?;

    // Resolve the relative path
    let resolved = if let Some(stripped) = import_path.strip_prefix("./") {
        parent_dir.join(stripped)
    } else if import_path.starts_with("../") {
        // Handle parent directory references
        let mut current = parent_dir.to_path_buf();
        let mut remaining = import_path;
        while let Some(rest) = remaining.strip_prefix("../") {
            current = current.parent()?.to_path_buf();
            remaining = rest;
        }
        current.join(remaining)
    } else if let Some(stripped) = import_path.strip_prefix('/') {
        // Absolute path (rare in TypeScript)
        PathBuf::from(format!("/{stripped}"))
    } else {
        return None;
    };

    Some(resolved)
}

/// Normalize a path for matching (remove extension, convert to canonical form)
fn normalize_path_for_matching(path: &Path) -> String {
    let path_str = path.to_string_lossy();
    // Remove common TypeScript/JavaScript extensions
    // Order matters: .d.ts must be checked before .ts
    let without_ext = if path_str.ends_with(".d.ts") {
        path_str.trim_end_matches(".d.ts")
    } else {
        path_str
            .trim_end_matches(".ts")
            .trim_end_matches(".tsx")
            .trim_end_matches(".js")
            .trim_end_matches(".jsx")
    };
    without_ext.to_string()
}

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
        let file_name = file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();
        let props = PropertyMap::new()
            .with("name", file_name.clone())
            .with("path", file_path.display().to_string())
            .with("language", "typescript");

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
            .with("line_start", func.line_start as i64)
            .with("line_end", func.line_end as i64)
            .with("is_async", func.is_async)
            .with("visibility", func.visibility.clone());

        if let Some(ref body) = func.body_prefix {
            props = props.with("body_prefix", body.clone());
        }

        if !func.attributes.is_empty() {
            props = props.with("attributes", func.attributes.clone());
        }

        // HTTP handler detection: NestJS decorators (@Get, @Post, etc.)
        if let Some((method, route)) = detect_ts_http_decorator(&func.attributes) {
            props = props
                .with("http_method", method)
                .with("route", route)
                .with("is_entry_point", true);
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
            .map_err(|e| ParserError::GraphError(e.to_string()))?;

        node_map.insert(func.name.clone(), func_id);
        function_ids.push(func_id);

        // Link to file or parent class
        if let Some(ref parent_class) = func.parent_class {
            if let Some(&class_id) = node_map.get(parent_class) {
                graph
                    .add_edge(class_id, func_id, EdgeType::Contains, PropertyMap::new())
                    .map_err(|e| ParserError::GraphError(e.to_string()))?;
            } else {
                graph
                    .add_edge(file_id, func_id, EdgeType::Contains, PropertyMap::new())
                    .map_err(|e| ParserError::GraphError(e.to_string()))?;
            }
        } else {
            graph
                .add_edge(file_id, func_id, EdgeType::Contains, PropertyMap::new())
                .map_err(|e| ParserError::GraphError(e.to_string()))?;
        }
    }

    // Add classes
    for class in &ir.classes {
        let mut props = PropertyMap::new()
            .with("name", class.name.clone())
            .with("path", file_path.display().to_string())
            .with("line_start", class.line_start as i64)
            .with("line_end", class.line_end as i64)
            .with("visibility", class.visibility.clone());

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

    // Add interfaces
    for interface in &ir.traits {
        let props = PropertyMap::new()
            .with("name", interface.name.clone())
            .with("path", file_path.display().to_string())
            .with("line_start", interface.line_start as i64)
            .with("line_end", interface.line_end as i64)
            .with("visibility", interface.visibility.clone());

        let trait_id = graph
            .add_node(NodeType::Interface, props)
            .map_err(|e| ParserError::GraphError(e.to_string()))?;

        node_map.insert(interface.name.clone(), trait_id);
        trait_ids.push(trait_id);

        graph
            .add_edge(file_id, trait_id, EdgeType::Contains, PropertyMap::new())
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
    }

    // Add import nodes and relationships
    for import in &ir.imports {
        let imported_module = &import.imported;

        // Try to resolve relative imports to actual file paths
        let resolved_path = resolve_import_path(file_path, imported_module);
        let normalized_resolved = resolved_path
            .as_ref()
            .map(|p| normalize_path_for_matching(p));

        // Check if we can find the target in our node_map by resolved path
        // This handles the case where we've already parsed the target file
        let mut target_node_id: Option<NodeId> = None;

        if let Some(ref resolved) = normalized_resolved {
            // Try to find a file node matching the resolved import path
            // Check node_map for matching file stem
            let import_file_stem = PathBuf::from(resolved)
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string());

            if let Some(ref stem) = import_file_stem {
                target_node_id = node_map.get(stem).copied();
            }
        }

        // Create edge properties
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

        // Store resolved path for cross-file resolution
        if let Some(ref resolved) = normalized_resolved {
            edge_props = edge_props.with("resolved_path", resolved.clone());
        }

        // If we have specific imported symbols, create edges to those symbols directly
        // This enables proper usage tracking for classes, functions, etc.
        let mut symbols_linked = false;
        if !import.symbols.is_empty() {
            for symbol in &import.symbols {
                // Check if this symbol exists in our node_map
                if let Some(&symbol_id) = node_map.get(symbol) {
                    // Create an import edge directly to the symbol
                    let symbol_edge_props = edge_props
                        .clone()
                        .with("imported_symbol", symbol.clone())
                        .with("source_module", imported_module.clone());

                    graph
                        .add_edge(file_id, symbol_id, EdgeType::Imports, symbol_edge_props)
                        .map_err(|e| ParserError::GraphError(e.to_string()))?;
                    symbols_linked = true;
                }
            }
        }

        // Also create edge to the module/file node (for module-level tracking)
        let import_id = if let Some(id) = target_node_id {
            id
        } else if let Some(&existing_id) = node_map.get(imported_module) {
            existing_id
        } else {
            // Create a placeholder module node for unresolved/external imports
            let is_external = resolved_path.is_none();
            let props = PropertyMap::new()
                .with("name", imported_module.clone())
                .with("is_external", is_external);

            let id = graph
                .add_node(NodeType::Module, props)
                .map_err(|e| ParserError::GraphError(e.to_string()))?;
            node_map.insert(imported_module.clone(), id);
            id
        };

        import_ids.push(import_id);

        // Create import edge from file to imported module
        // (only if we didn't already link to individual symbols, or if no symbols specified)
        if !symbols_linked || import.symbols.is_empty() {
            graph
                .add_edge(file_id, import_id, EdgeType::Imports, edge_props)
                .map_err(|e| ParserError::GraphError(e.to_string()))?;
        }
    }

    // Add call relationships
    // Track unresolved calls per caller for cross-file resolution
    let mut unresolved_calls: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();

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

    // Add type reference relationships (creates References edges)
    // Track unresolved type refs per referrer for cross-file resolution
    let mut unresolved_type_refs: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();

    for type_ref in &ir.type_references {
        if let Some(&referrer_id) = node_map.get(&type_ref.referrer) {
            if let Some(&type_id) = node_map.get(&type_ref.type_name) {
                // Both referrer and type are in this file — create direct edge
                let _ = graph.add_edge(
                    referrer_id,
                    type_id,
                    EdgeType::References,
                    PropertyMap::new(),
                );
            } else {
                // Type not in this file — store for cross-file resolution
                unresolved_type_refs
                    .entry(type_ref.referrer.clone())
                    .or_default()
                    .push(type_ref.type_name.clone());
            }
        }
    }

    // Store unresolved type refs on referrer nodes for post-processing
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

    Ok(FileInfo {
        file_path: file_path.to_path_buf(),
        file_id,
        functions: function_ids,
        classes: class_ids,
        traits: trait_ids,
        imports: import_ids,
        parse_time: Duration::ZERO,
        line_count: 0,
        byte_count: 0,
    })
}

/// Detect HTTP handler decorators for TypeScript/JavaScript.
///
/// NestJS: @Get(), @Post('/path'), @Put(':id'), @Delete(), @Patch()
/// Also: @HttpCode, @Header are hints but not route-defining
fn detect_ts_http_decorator(attributes: &[String]) -> Option<(String, String)> {
    const HTTP_METHODS: &[&str] = &["get", "post", "put", "delete", "patch", "head", "options"];

    for attr in attributes {
        let lower = attr.to_lowercase();

        // NestJS: Get(), Get('/path'), Post(), etc.
        for method in HTTP_METHODS {
            if lower.starts_with(method) && (lower.len() == method.len() || lower.as_bytes().get(method.len()) == Some(&b'(')) {
                let route = extract_first_string(attr).unwrap_or_else(|| "/".to_string());
                return Some((method.to_uppercase(), route));
            }
        }

        // Express-style: app.get, router.get — but these are call patterns, not decorators
        // Handled separately if needed
    }

    None
}

/// Extract the first quoted string from a decorator argument like `Get('/users/:id')`
fn extract_first_string(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\'' || bytes[i] == b'"' {
            let quote = bytes[i];
            let start = i + 1;
            i += 1;
            while i < bytes.len() && bytes[i] != quote {
                i += 1;
            }
            if i < bytes.len() {
                return Some(String::from_utf8_lossy(&bytes[start..i]).to_string());
            }
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph_parser_api::{ClassEntity, FunctionEntity, ModuleEntity, TraitEntity};
    use std::path::PathBuf;

    #[test]
    fn test_ir_to_graph_empty() {
        let ir = CodeIR::new(PathBuf::from("test.ts"));
        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, PathBuf::from("test.ts").as_path());

        assert!(result.is_ok());
        let file_info = result.unwrap();
        assert_eq!(file_info.functions.len(), 0);
        assert_eq!(file_info.classes.len(), 0);
        assert_eq!(file_info.traits.len(), 0);
    }

    #[test]
    fn test_ir_to_graph_with_function() {
        let mut ir = CodeIR::new(PathBuf::from("test.ts"));
        ir.add_function(FunctionEntity::new("testFunc", 1, 5));

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, PathBuf::from("test.ts").as_path());

        assert!(result.is_ok());
        let file_info = result.unwrap();
        assert_eq!(file_info.functions.len(), 1);
    }

    #[test]
    fn test_ir_to_graph_with_class() {
        let mut ir = CodeIR::new(PathBuf::from("test.ts"));
        ir.add_class(ClassEntity::new("TestClass", 1, 10));

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, PathBuf::from("test.ts").as_path());

        assert!(result.is_ok());
        let file_info = result.unwrap();
        assert_eq!(file_info.classes.len(), 1);
    }

    #[test]
    fn test_ir_to_graph_with_interface() {
        let mut ir = CodeIR::new(PathBuf::from("test.ts"));
        ir.add_trait(TraitEntity::new("ITest", 1, 5));

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, PathBuf::from("test.ts").as_path());

        assert!(result.is_ok());
        let file_info = result.unwrap();
        assert_eq!(file_info.traits.len(), 1);
    }

    #[test]
    fn test_ir_to_graph_with_module() {
        let mut ir = CodeIR::new(PathBuf::from("test.ts"));
        ir.set_module(codegraph_parser_api::ModuleEntity::new(
            "test",
            "test.ts",
            "typescript",
        ));

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, PathBuf::from("test.ts").as_path());

        assert!(result.is_ok());
        let file_info = result.unwrap();
        // File node should be created - just verify we got a valid NodeId
        graph.get_node(file_info.file_id).unwrap();
    }

    #[test]
    fn test_ir_to_graph_async_function() {
        let mut ir = CodeIR::new(PathBuf::from("test.ts"));
        let func = FunctionEntity::new("asyncFunc", 1, 5).async_fn();
        ir.add_function(func);

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, PathBuf::from("test.ts").as_path());

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
    fn test_ir_to_graph_with_imports() {
        use codegraph::{Direction, EdgeType};
        use codegraph_parser_api::ImportRelation;

        let mut ir = CodeIR::new(PathBuf::from("test.ts"));
        ir.add_import(ImportRelation::new("test.ts", "react"));
        ir.add_import(
            ImportRelation::new("test.ts", "lodash")
                .with_symbols(vec!["map".to_string(), "filter".to_string()]),
        );

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, PathBuf::from("test.ts").as_path());

        assert!(result.is_ok());
        let file_info = result.unwrap();

        // Verify import nodes were created and returned
        assert_eq!(file_info.imports.len(), 2, "Should have 2 import nodes");

        // Verify import edges were created (file -> module with EdgeType::Imports)
        let neighbors = graph
            .get_neighbors(file_info.file_id, Direction::Outgoing)
            .unwrap();

        // Check that import modules are in the neighbors
        let mut import_edges_count = 0;
        for neighbor_id in &neighbors {
            let edges = graph
                .get_edges_between(file_info.file_id, *neighbor_id)
                .unwrap();
            for edge_id in edges {
                let edge = graph.get_edge(edge_id).unwrap();
                if edge.edge_type == EdgeType::Imports {
                    import_edges_count += 1;
                }
            }
        }
        assert_eq!(import_edges_count, 2, "Should have 2 import edges");
    }

    #[test]
    fn test_ir_to_graph_with_calls() {
        use codegraph::EdgeType;
        use codegraph_parser_api::CallRelation;

        let mut ir = CodeIR::new(PathBuf::from("test.ts"));
        ir.add_function(FunctionEntity::new("caller", 1, 10));
        ir.add_function(FunctionEntity::new("callee", 12, 20));
        ir.add_call(CallRelation::new("caller", "callee", 5));

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, PathBuf::from("test.ts").as_path());

        assert!(result.is_ok());
        let file_info = result.unwrap();
        assert_eq!(file_info.functions.len(), 2);

        // Find caller and callee node IDs
        let caller_id = file_info.functions[0];
        let callee_id = file_info.functions[1];

        // Verify call edge was created
        let edges = graph.get_edges_between(caller_id, callee_id).unwrap();
        assert!(
            !edges.is_empty(),
            "Should have call edge between caller and callee"
        );

        let edge = graph.get_edge(edges[0]).unwrap();
        assert_eq!(
            edge.edge_type,
            EdgeType::Calls,
            "Edge should be of type Calls"
        );
    }

    #[test]
    fn test_ir_to_graph_with_inheritance() {
        use codegraph::EdgeType;
        use codegraph_parser_api::InheritanceRelation;

        let mut ir = CodeIR::new(PathBuf::from("test.ts"));
        ir.add_class(ClassEntity::new("ChildClass", 1, 20));
        ir.add_class(ClassEntity::new("ParentClass", 22, 40));
        ir.add_inheritance(InheritanceRelation::new("ChildClass", "ParentClass"));

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, PathBuf::from("test.ts").as_path());

        assert!(result.is_ok());
        let file_info = result.unwrap();
        assert_eq!(file_info.classes.len(), 2);

        // Find child and parent node IDs
        let child_id = file_info.classes[0];
        let parent_id = file_info.classes[1];

        // Verify extends edge was created
        let edges = graph.get_edges_between(child_id, parent_id).unwrap();
        assert!(
            !edges.is_empty(),
            "Should have extends edge between child and parent"
        );

        let edge = graph.get_edge(edges[0]).unwrap();
        assert_eq!(
            edge.edge_type,
            EdgeType::Extends,
            "Edge should be of type Extends"
        );
    }

    #[test]
    fn test_ir_to_graph_with_implementation() {
        use codegraph::EdgeType;
        use codegraph_parser_api::ImplementationRelation;

        let mut ir = CodeIR::new(PathBuf::from("test.ts"));
        ir.add_class(ClassEntity::new("MyClass", 1, 20));
        ir.add_trait(TraitEntity::new("IMyInterface", 22, 30));
        ir.add_implementation(ImplementationRelation::new("MyClass", "IMyInterface"));

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, PathBuf::from("test.ts").as_path());

        assert!(result.is_ok());
        let file_info = result.unwrap();
        assert_eq!(file_info.classes.len(), 1);
        assert_eq!(file_info.traits.len(), 1);

        // Find class and interface node IDs
        let class_id = file_info.classes[0];
        let interface_id = file_info.traits[0];

        // Verify implements edge was created
        let edges = graph.get_edges_between(class_id, interface_id).unwrap();
        assert!(
            !edges.is_empty(),
            "Should have implements edge between class and interface"
        );

        let edge = graph.get_edge(edges[0]).unwrap();
        assert_eq!(
            edge.edge_type,
            EdgeType::Implements,
            "Edge should be of type Implements"
        );
    }

    // ==========================================
    // Import Path Resolution Tests
    // ==========================================

    #[test]
    fn test_resolve_import_path_relative_same_dir() {
        let file_path = PathBuf::from("/src/extension.ts");
        let result = resolve_import_path(&file_path, "./toolManager");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), PathBuf::from("/src/toolManager"));
    }

    #[test]
    fn test_resolve_import_path_relative_subdir() {
        let file_path = PathBuf::from("/src/extension.ts");
        let result = resolve_import_path(&file_path, "./ai/toolManager");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), PathBuf::from("/src/ai/toolManager"));
    }

    #[test]
    fn test_resolve_import_path_parent_dir() {
        let file_path = PathBuf::from("/src/ai/toolManager.ts");
        let result = resolve_import_path(&file_path, "../extension");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), PathBuf::from("/src/extension"));
    }

    #[test]
    fn test_resolve_import_path_multiple_parent_dirs() {
        let file_path = PathBuf::from("/src/ai/tools/manager.ts");
        let result = resolve_import_path(&file_path, "../../extension");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), PathBuf::from("/src/extension"));
    }

    #[test]
    fn test_resolve_import_path_external_package() {
        let file_path = PathBuf::from("/src/extension.ts");
        let result = resolve_import_path(&file_path, "vscode");
        assert!(result.is_none(), "External packages should return None");
    }

    #[test]
    fn test_resolve_import_path_scoped_package() {
        let file_path = PathBuf::from("/src/extension.ts");
        let result = resolve_import_path(&file_path, "@types/node");
        assert!(result.is_none(), "Scoped packages should return None");
    }

    #[test]
    fn test_normalize_path_removes_ts_extension() {
        let path = PathBuf::from("/src/toolManager.ts");
        assert_eq!(normalize_path_for_matching(&path), "/src/toolManager");
    }

    #[test]
    fn test_normalize_path_removes_tsx_extension() {
        let path = PathBuf::from("/src/Component.tsx");
        assert_eq!(normalize_path_for_matching(&path), "/src/Component");
    }

    #[test]
    fn test_normalize_path_removes_dts_extension() {
        let path = PathBuf::from("/types/index.d.ts");
        assert_eq!(normalize_path_for_matching(&path), "/types/index");
    }

    #[test]
    fn test_normalize_path_no_extension() {
        let path = PathBuf::from("/src/toolManager");
        assert_eq!(normalize_path_for_matching(&path), "/src/toolManager");
    }

    // ==========================================
    // Import Edge to Symbol Tests
    // ==========================================

    #[test]
    fn test_import_creates_edge_to_symbol() {
        use codegraph::EdgeType;
        use codegraph_parser_api::ImportRelation;

        // Simulate: extension.ts imports { MyClass } from './module'
        // where MyClass is defined in the same file (for testing purposes)
        let mut ir = CodeIR::new(PathBuf::from("/src/extension.ts"));

        // Add the class that will be "imported"
        ir.add_class(ClassEntity::new("MyClass", 10, 20));

        // Add the import referencing that class
        ir.add_import(
            ImportRelation::new("/src/extension.ts", "./module")
                .with_symbols(vec!["MyClass".to_string()]),
        );

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(
            &ir,
            &mut graph,
            PathBuf::from("/src/extension.ts").as_path(),
        );

        assert!(result.is_ok());
        let file_info = result.unwrap();

        // Get the class node
        let class_id = file_info.classes[0];

        // Verify there's an import edge from file to the class
        let edges = graph
            .get_edges_between(file_info.file_id, class_id)
            .unwrap();

        let import_edges: Vec<_> = edges
            .iter()
            .filter_map(|e| graph.get_edge(*e).ok())
            .filter(|e| e.edge_type == EdgeType::Imports)
            .collect();

        assert!(
            !import_edges.is_empty(),
            "Should have import edge from file to the imported class"
        );

        // Verify edge has the imported_symbol property
        let edge = &import_edges[0];
        assert_eq!(
            edge.properties.get_string("imported_symbol"),
            Some("MyClass")
        );
    }

    #[test]
    fn test_import_creates_edge_to_function() {
        use codegraph::EdgeType;
        use codegraph_parser_api::ImportRelation;

        // Simulate: extension.ts imports { myFunction } from './utils'
        let mut ir = CodeIR::new(PathBuf::from("/src/extension.ts"));

        // Add the function that will be "imported"
        ir.add_function(FunctionEntity::new("myFunction", 5, 15));

        // Add the import referencing that function
        ir.add_import(
            ImportRelation::new("/src/extension.ts", "./utils")
                .with_symbols(vec!["myFunction".to_string()]),
        );

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(
            &ir,
            &mut graph,
            PathBuf::from("/src/extension.ts").as_path(),
        );

        assert!(result.is_ok());
        let file_info = result.unwrap();

        // Get the function node
        let func_id = file_info.functions[0];

        // Verify there's an import edge from file to the function
        let edges = graph.get_edges_between(file_info.file_id, func_id).unwrap();

        let import_edges: Vec<_> = edges
            .iter()
            .filter_map(|e| graph.get_edge(*e).ok())
            .filter(|e| e.edge_type == EdgeType::Imports)
            .collect();

        assert!(
            !import_edges.is_empty(),
            "Should have import edge from file to the imported function"
        );
    }

    #[test]
    fn test_import_multiple_symbols_creates_multiple_edges() {
        use codegraph::EdgeType;
        use codegraph_parser_api::ImportRelation;

        // Simulate: extension.ts imports { ClassA, ClassB } from './module'
        let mut ir = CodeIR::new(PathBuf::from("/src/extension.ts"));

        // Add both classes
        ir.add_class(ClassEntity::new("ClassA", 10, 20));
        ir.add_class(ClassEntity::new("ClassB", 25, 35));

        // Add the import with both symbols
        ir.add_import(
            ImportRelation::new("/src/extension.ts", "./module")
                .with_symbols(vec!["ClassA".to_string(), "ClassB".to_string()]),
        );

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(
            &ir,
            &mut graph,
            PathBuf::from("/src/extension.ts").as_path(),
        );

        assert!(result.is_ok());
        let file_info = result.unwrap();

        // Verify both classes have import edges
        for class_id in &file_info.classes {
            let edges = graph
                .get_edges_between(file_info.file_id, *class_id)
                .unwrap();

            let import_edges: Vec<_> = edges
                .iter()
                .filter_map(|e| graph.get_edge(*e).ok())
                .filter(|e| e.edge_type == EdgeType::Imports)
                .collect();

            assert!(
                !import_edges.is_empty(),
                "Each imported class should have an import edge"
            );
        }
    }

    #[test]
    fn test_external_import_creates_module_node() {
        use codegraph_parser_api::ImportRelation;

        // Simulate: extension.ts imports { useState } from 'react'
        let mut ir = CodeIR::new(PathBuf::from("/src/extension.ts"));

        ir.add_import(
            ImportRelation::new("/src/extension.ts", "react")
                .with_symbols(vec!["useState".to_string()]),
        );

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(
            &ir,
            &mut graph,
            PathBuf::from("/src/extension.ts").as_path(),
        );

        assert!(result.is_ok());
        let file_info = result.unwrap();

        // Verify a module node was created for 'react'
        assert_eq!(file_info.imports.len(), 1);

        let import_node = graph.get_node(file_info.imports[0]).unwrap();
        assert_eq!(import_node.node_type, NodeType::Module);
        assert_eq!(import_node.properties.get_string("name"), Some("react"));
        assert_eq!(import_node.properties.get_bool("is_external"), Some(true));
    }

    #[test]
    fn test_import_stores_resolved_path() {
        use codegraph::{Direction, EdgeType};
        use codegraph_parser_api::ImportRelation;

        // Simulate: extension.ts imports { Tool } from './ai/tools'
        let mut ir = CodeIR::new(PathBuf::from("/src/extension.ts"));

        ir.add_import(ImportRelation::new("/src/extension.ts", "./ai/tools"));

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(
            &ir,
            &mut graph,
            PathBuf::from("/src/extension.ts").as_path(),
        );

        assert!(result.is_ok());
        let file_info = result.unwrap();

        // Get the import edge and verify it has resolved_path
        let neighbors = graph
            .get_neighbors(file_info.file_id, Direction::Outgoing)
            .unwrap();

        let mut found_resolved_path = false;
        for neighbor_id in &neighbors {
            let edges = graph
                .get_edges_between(file_info.file_id, *neighbor_id)
                .unwrap();
            for edge_id in edges {
                let edge = graph.get_edge(edge_id).unwrap();
                if edge.edge_type == EdgeType::Imports {
                    if let Some(resolved) = edge.properties.get_string("resolved_path") {
                        assert!(resolved.contains("ai/tools"));
                        found_resolved_path = true;
                    }
                }
            }
        }

        assert!(
            found_resolved_path,
            "Import edge should have resolved_path property for relative imports"
        );
    }

    #[test]
    fn test_ir_to_graph_with_type_references() {
        use codegraph::EdgeType;
        use codegraph_parser_api::TypeReference;

        let mut ir = CodeIR::new(PathBuf::from("test.ts"));
        ir.add_function(FunctionEntity::new("process", 10, 15));
        ir.add_trait(TraitEntity::new("MyParams", 1, 4));
        ir.add_trait(TraitEntity::new("MyResponse", 6, 9));
        ir.type_references.push(TypeReference::new(
            "process".to_string(),
            "MyParams".to_string(),
            10,
        ));
        ir.type_references.push(TypeReference::new(
            "process".to_string(),
            "MyResponse".to_string(),
            10,
        ));
        ir.type_references.push(TypeReference::new(
            "MyResponse".to_string(),
            "MyParams".to_string(),
            7,
        ));

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, PathBuf::from("test.ts").as_path());

        assert!(result.is_ok());
        let file_info = result.unwrap();

        let func_id = file_info.functions[0];
        let params_id = file_info.traits[0];
        let response_id = file_info.traits[1];

        // process -> MyParams (References)
        let edges = graph.get_edges_between(func_id, params_id).unwrap();
        assert!(
            edges
                .iter()
                .any(|&e| graph.get_edge(e).unwrap().edge_type == EdgeType::References),
            "process should have References edge to MyParams"
        );

        // process -> MyResponse (References)
        let edges = graph.get_edges_between(func_id, response_id).unwrap();
        assert!(
            edges
                .iter()
                .any(|&e| graph.get_edge(e).unwrap().edge_type == EdgeType::References),
            "process should have References edge to MyResponse"
        );

        // MyResponse -> MyParams (References via field type)
        let edges = graph.get_edges_between(response_id, params_id).unwrap();
        assert!(
            edges
                .iter()
                .any(|&e| graph.get_edge(e).unwrap().edge_type == EdgeType::References),
            "MyResponse should have References edge to MyParams"
        );
    }

    #[test]
    fn test_property_types() {
        use codegraph::PropertyValue;
        let mut ir = CodeIR::new(std::path::PathBuf::from("test.ts"));
        ir.module = Some(ModuleEntity::new("test", "test.ts", "typescript").with_line_count(100));
        let func = FunctionEntity::new("test_fn", 10, 20)
            .with_signature("fn test_fn()")
            .with_visibility("public")
            .async_fn();
        ir.functions.push(func);

        let mut graph = CodeGraph::in_memory().unwrap();
        let file_info = ir_to_graph(&ir, &mut graph, std::path::Path::new("test.ts")).unwrap();

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
        assert_eq!(
            func_node.properties.get_string("visibility"),
            Some("public"),
            "visibility should be 'public'"
        );
    }
}
