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
    let trait_ids = Vec::new();
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
            .with("language", "c");

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

        // Link function to file
        graph
            .add_edge(file_id, func_id, EdgeType::Contains, PropertyMap::new())
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
    }

    // Add classes/structs/unions/enums
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

        // Add type attribute (struct, union, enum)
        if !class.attributes.is_empty() {
            props = props.with("type_kind", class.attributes[0].clone());
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

        // Add fields as properties or child nodes (simplified: store as metadata)
        for (idx, field) in class.fields.iter().enumerate() {
            let field_props = PropertyMap::new()
                .with("name", field.name.clone())
                .with(
                    "type_annotation",
                    field.type_annotation.clone().unwrap_or_default(),
                )
                .with("visibility", field.visibility.clone())
                .with("is_static", field.is_static)
                .with("is_constant", field.is_constant)
                .with("field_index", idx as i64);

            let field_id = graph
                .add_node(NodeType::Variable, field_props)
                .map_err(|e| ParserError::GraphError(e.to_string()))?;

            // Link field to class
            graph
                .add_edge(class_id, field_id, EdgeType::Contains, PropertyMap::new())
                .map_err(|e| ParserError::GraphError(e.to_string()))?;
        }
    }

    // Add import nodes and relationships
    for import in &ir.imports {
        let imported_module = &import.imported;

        // Create or get import node
        let import_id = if let Some(&existing_id) = node_map.get(imported_module) {
            existing_id
        } else {
            let mut props = PropertyMap::new()
                .with("name", imported_module.clone())
                .with("is_external", "true");

            // Mark system includes
            if import.alias.as_deref() == Some("system") {
                props = props.with("is_system", "true");
            }

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

        graph
            .add_edge(file_id, import_id, EdgeType::Imports, edge_props)
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
    }

    // Add call relationships
    // Track unresolved calls per caller for cross-file resolution
    let mut unresolved_calls: HashMap<String, Vec<String>> = HashMap::new();

    for call in &ir.calls {
        // Resolve caller: function node, or file node for vtable assignments
        let caller_id = node_map.get(&call.caller).copied().unwrap_or_else(|| {
            // Vtable/struct initializer calls have synthetic caller names
            // (e.g. "vtable_readlink") — use file node as the caller
            if call.caller.starts_with("vtable_") {
                file_id
            } else {
                // Unknown caller — skip by returning a sentinel
                u64::MAX
            }
        });

        if caller_id == u64::MAX {
            continue;
        }

        if let Some(&callee_id) = node_map.get(&call.callee) {
            // Both caller and callee are in this file - create direct edge
            let mut edge_props = PropertyMap::new()
                .with("call_site_line", call.call_site_line as i64)
                .with("is_direct", call.is_direct);

            // Add ops struct metadata if this is a vtable assignment
            if let Some(ref st) = call.struct_type {
                edge_props = edge_props.with("struct_type", st.clone());
            }
            if let Some(ref fn_name) = call.field_name {
                edge_props = edge_props.with("field_name", fn_name.clone());
            }

            graph
                .add_edge(caller_id, callee_id, EdgeType::Calls, edge_props)
                .map_err(|e| ParserError::GraphError(e.to_string()))?;
        } else {
            // Callee not found in this file - store for cross-file resolution
            // For vtable calls, store on the file node so cross-file resolution
            // can find the target function
            let store_on = if call.caller.starts_with("vtable_") {
                "file".to_string()
            } else {
                call.caller.clone()
            };
            unresolved_calls
                .entry(store_on)
                .or_default()
                .push(call.callee.clone());
        }
    }

    // Store unresolved calls on caller nodes for post-processing
    for (caller_name, callees) in unresolved_calls {
        // "file" is a special key for vtable calls — store on the file node
        let caller_id_opt = if caller_name == "file" {
            Some(file_id)
        } else {
            node_map.get(&caller_name).copied()
        };
        if let Some(caller_id) = caller_id_opt {
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

/// Apply kernel macro metadata to function nodes.
/// Sets `is_entry_point` on module_init/exit targets and
/// `is_exported` on EXPORT_SYMBOL targets.
pub fn apply_kernel_macros(
    graph: &mut CodeGraph,
    entry_points: &[String],
    exported_symbols: &[String],
) {
    if entry_points.is_empty() && exported_symbols.is_empty() {
        return;
    }

    // Build name → NodeId map for functions
    let func_map: HashMap<String, NodeId> = graph
        .iter_nodes()
        .filter(|(_, n)| n.node_type == NodeType::Function)
        .filter_map(|(id, n)| {
            n.properties
                .get_string("name")
                .map(|name| (name.to_string(), id))
        })
        .collect();

    for name in entry_points {
        if let Some(&node_id) = func_map.get(name) {
            if let Ok(node) = graph.get_node(node_id) {
                let props = node
                    .properties
                    .clone()
                    .with("is_entry_point", true)
                    .with("entry_type", "module_init");
                let _ = graph.update_node_properties(node_id, props);
            }
        }
    }

    for name in exported_symbols {
        if let Some(&node_id) = func_map.get(name) {
            if let Ok(node) = graph.get_node(node_id) {
                let props = node
                    .properties
                    .clone()
                    .with("is_exported", true)
                    .with("visibility", "public");
                let _ = graph.update_node_properties(node_id, props);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph_parser_api::{ClassEntity, FunctionEntity, ImportRelation, ModuleEntity};
    use std::path::PathBuf;

    #[test]
    fn test_ir_to_graph_empty() {
        let ir = CodeIR::new(PathBuf::from("test.c"));
        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, PathBuf::from("test.c").as_path());

        assert!(result.is_ok());
        let file_info = result.unwrap();
        assert_eq!(file_info.functions.len(), 0);
        assert_eq!(file_info.classes.len(), 0);
        assert_eq!(file_info.traits.len(), 0);
    }

    #[test]
    fn test_ir_to_graph_with_function() {
        let mut ir = CodeIR::new(PathBuf::from("test.c"));
        ir.add_function(FunctionEntity::new("main", 1, 5));

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, PathBuf::from("test.c").as_path());

        assert!(result.is_ok());
        let file_info = result.unwrap();
        assert_eq!(file_info.functions.len(), 1);
    }

    #[test]
    fn test_ir_to_graph_with_struct() {
        let mut ir = CodeIR::new(PathBuf::from("test.c"));
        ir.add_class(ClassEntity::new("Point", 1, 10));

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, PathBuf::from("test.c").as_path());

        assert!(result.is_ok());
        let file_info = result.unwrap();
        assert_eq!(file_info.classes.len(), 1);
    }

    #[test]
    fn test_ir_to_graph_with_module() {
        let mut ir = CodeIR::new(PathBuf::from("test.c"));
        ir.set_module(codegraph_parser_api::ModuleEntity::new(
            "main", "test.c", "c",
        ));

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, PathBuf::from("test.c").as_path());

        assert!(result.is_ok());
        let file_info = result.unwrap();
        // File node should be created
        graph.get_node(file_info.file_id).unwrap();
    }

    #[test]
    fn test_ir_to_graph_with_imports() {
        let mut ir = CodeIR::new(PathBuf::from("test.c"));
        ir.add_import(ImportRelation::new("main", "stdio.h"));
        ir.add_import(ImportRelation::new("main", "stdlib.h"));

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, PathBuf::from("test.c").as_path());

        assert!(result.is_ok());
        let file_info = result.unwrap();
        assert_eq!(file_info.imports.len(), 2);
    }

    #[test]
    fn test_ir_to_graph_with_fields() {
        let mut ir = CodeIR::new(PathBuf::from("test.c"));

        let mut class = ClassEntity::new("Point", 1, 10);
        class.fields.push(codegraph_parser_api::Field {
            name: "x".to_string(),
            type_annotation: Some("int".to_string()),
            visibility: "public".to_string(),
            is_static: false,
            is_constant: false,
            default_value: None,
        });
        class.fields.push(codegraph_parser_api::Field {
            name: "y".to_string(),
            type_annotation: Some("int".to_string()),
            visibility: "public".to_string(),
            is_static: false,
            is_constant: false,
            default_value: None,
        });
        ir.add_class(class);

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, PathBuf::from("test.c").as_path());

        assert!(result.is_ok());
        let file_info = result.unwrap();
        assert_eq!(file_info.classes.len(), 1);
    }

    #[test]
    fn test_ir_to_graph_function_properties() {
        let mut ir = CodeIR::new(PathBuf::from("test.c"));
        let func = FunctionEntity::new("helper", 1, 5)
            .with_visibility("private")
            .with_signature("static int helper(void)");
        ir.add_function(func);

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, PathBuf::from("test.c").as_path());

        assert!(result.is_ok());
        let file_info = result.unwrap();
        assert_eq!(file_info.functions.len(), 1);

        // Verify function properties
        let func_node = graph.get_node(file_info.functions[0]).unwrap();
        assert_eq!(
            func_node.properties.get("name"),
            Some(&codegraph::PropertyValue::String("helper".to_string()))
        );
        assert_eq!(
            func_node.properties.get("visibility"),
            Some(&codegraph::PropertyValue::String("private".to_string()))
        );
    }

    #[test]
    fn test_ir_to_graph_with_complexity() {
        let mut ir = CodeIR::new(PathBuf::from("test.c"));

        let mut func = FunctionEntity::new("complex", 1, 20);
        func.complexity = Some(codegraph_parser_api::ComplexityMetrics {
            cyclomatic_complexity: 10,
            branches: 5,
            loops: 3,
            logical_operators: 2,
            max_nesting_depth: 4,
            exception_handlers: 0,
            early_returns: 1,
        });
        ir.add_function(func);

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, PathBuf::from("test.c").as_path());

        assert!(result.is_ok());
        let file_info = result.unwrap();

        // Verify complexity properties
        let func_node = graph.get_node(file_info.functions[0]).unwrap();
        assert_eq!(
            func_node.properties.get("complexity"),
            Some(&codegraph::PropertyValue::Int(10))
        );
        assert_eq!(
            func_node.properties.get("complexity_grade"),
            Some(&codegraph::PropertyValue::String("B".to_string()))
        );
    }

    #[test]
    fn test_property_types() {
        use codegraph::PropertyValue;
        let mut ir = CodeIR::new(std::path::PathBuf::from("test.c"));
        ir.module = Some(ModuleEntity::new("test", "test.c", "c").with_line_count(100));
        let func = FunctionEntity::new("test_fn", 10, 20)
            .with_signature("fn test_fn()")
            .with_visibility("public");
        ir.functions.push(func);

        let mut graph = CodeGraph::in_memory().unwrap();
        let file_info = ir_to_graph(&ir, &mut graph, std::path::Path::new("test.c")).unwrap();

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
    }
}
