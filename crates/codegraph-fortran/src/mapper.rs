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

    // Create file node
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
            .with("language", "fortran");

        let id = graph
            .add_node(NodeType::CodeFile, props)
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
        node_map.insert(name, id);
        id
    };

    // Add program units (program, module, submodule, block_data) as class nodes
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

        graph
            .add_edge(file_id, class_id, EdgeType::Contains, PropertyMap::new())
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
    }

    // Add functions and subroutines
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
        if let Some(ref body) = func.body_prefix {
            props = props.with("body_prefix", body.clone());
        }
        if let Some(ref parent) = func.parent_class {
            props = props.with("parent_class", parent.clone());
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

        // Link to parent program unit or file
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

    // Add USE module imports
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

        let mut edge_props = PropertyMap::new();
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

    // Add CALL relationships
    let mut unresolved_calls: HashMap<String, Vec<String>> = HashMap::new();

    for call in &ir.calls {
        if let Some(&caller_id) = node_map.get(&call.caller) {
            if let Some(&callee_id) = node_map.get(&call.callee) {
                let edge_props = PropertyMap::new()
                    .with("call_site_line", call.call_site_line as i64)
                    .with("is_direct", call.is_direct);

                graph
                    .add_edge(caller_id, callee_id, EdgeType::Calls, edge_props)
                    .map_err(|e| ParserError::GraphError(e.to_string()))?;
            } else {
                unresolved_calls
                    .entry(call.caller.clone())
                    .or_default()
                    .push(call.callee.clone());
            }
        }
    }

    // Store unresolved calls on caller nodes for cross-file resolution
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
    use codegraph::PropertyValue;
    use codegraph_parser_api::{
        CallRelation, ClassEntity, ComplexityMetrics, FunctionEntity, ImportRelation, ModuleEntity,
    };
    use std::path::PathBuf;

    fn map(ir: &CodeIR) -> (CodeGraph, FileInfo) {
        let mut graph = CodeGraph::in_memory().unwrap();
        let info = ir_to_graph(ir, &mut graph, Path::new("test.f90")).unwrap();
        (graph, info)
    }

    /// Assert exactly one edge between src and dst and return its id.
    fn edge_between(graph: &CodeGraph, src: NodeId, dst: NodeId) -> codegraph::EdgeId {
        let edges = graph.get_edges_between(src, dst).unwrap();
        assert_eq!(edges.len(), 1, "expected exactly one edge {src}->{dst}");
        edges[0]
    }

    #[test]
    fn test_ir_to_graph_empty() {
        let ir = CodeIR::new(PathBuf::from("test.f90"));
        let (graph, info) = map(&ir);

        assert_eq!(info.functions.len(), 0);
        assert_eq!(info.classes.len(), 0);
        assert_eq!(info.imports.len(), 0);
        assert_eq!(graph.node_count(), 1);

        // File node name comes from the path stem, language is fortran.
        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(file.properties.get_string("name"), Some("test"));
        assert_eq!(file.properties.get_string("language"), Some("fortran"));
        assert_eq!(info.line_count, 0);
    }

    #[test]
    fn test_unknown_name_fallback() {
        let ir = CodeIR::new(PathBuf::from(".."));
        let mut graph = CodeGraph::in_memory().unwrap();
        let info = ir_to_graph(&ir, &mut graph, Path::new("..")).unwrap();

        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(file.properties.get_string("name"), Some("unknown"));
    }

    #[test]
    fn test_module_drives_file_metadata() {
        let mut ir = CodeIR::new(PathBuf::from("test.f90"));
        ir.set_module(
            ModuleEntity::new("mymod", "test.f90", "fortran")
                .with_line_count(42)
                .with_doc("module doc"),
        );

        let (graph, info) = map(&ir);
        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(file.properties.get_string("name"), Some("mymod"));
        assert_eq!(file.properties.get_int("line_count"), Some(42));
        assert_eq!(file.properties.get_string("doc"), Some("module doc"));
        assert_eq!(info.line_count, 42);
    }

    #[test]
    fn test_program_unit_class_node_and_contains() {
        let mut ir = CodeIR::new(PathBuf::from("test.f90"));
        ir.add_class(ClassEntity::new("hello", 1, 5).with_visibility("public"));

        let (graph, info) = map(&ir);
        assert_eq!(info.classes.len(), 1);
        assert_eq!(graph.node_count(), 2);

        let class = graph.get_node(info.classes[0]).unwrap();
        assert_eq!(class.node_type, NodeType::Class);
        assert_eq!(class.properties.get_string("name"), Some("hello"));
        assert_eq!(class.properties.get_int("line_start"), Some(1));
        assert_eq!(class.properties.get_int("line_end"), Some(5));
        assert_eq!(class.properties.get_bool("is_abstract"), Some(false));

        // File contains the class.
        let edge_id = edge_between(&graph, info.file_id, info.classes[0]);
        let edge = graph.get_edge(edge_id).unwrap();
        assert_eq!(edge.edge_type, EdgeType::Contains);
    }

    #[test]
    fn test_free_function_file_contains_and_flags() {
        let mut ir = CodeIR::new(PathBuf::from("test.f90"));
        ir.add_function(FunctionEntity::new("add", 2, 6));

        let (graph, info) = map(&ir);
        assert_eq!(info.functions.len(), 1);

        let func = graph.get_node(info.functions[0]).unwrap();
        assert_eq!(func.node_type, NodeType::Function);
        assert_eq!(func.properties.get_string("name"), Some("add"));
        assert_eq!(func.properties.get_bool("is_async"), Some(false));
        assert_eq!(func.properties.get_bool("is_static"), Some(false));
        // No parent_class prop for a free function.
        assert_eq!(func.properties.get_string("parent_class"), None);

        let edge_id = edge_between(&graph, info.file_id, info.functions[0]);
        assert_eq!(
            graph.get_edge(edge_id).unwrap().edge_type,
            EdgeType::Contains
        );
    }

    #[test]
    fn test_function_complexity_props() {
        let mut ir = CodeIR::new(PathBuf::from("test.f90"));
        let complexity = ComplexityMetrics::new()
            .with_branches(3)
            .with_loops(2)
            .finalize();
        ir.add_function(FunctionEntity::new("compute", 1, 20).with_complexity(complexity));

        let (graph, info) = map(&ir);
        let func = graph.get_node(info.functions[0]).unwrap();
        assert_eq!(func.properties.get_int("complexity_branches"), Some(3));
        assert_eq!(func.properties.get_int("complexity_loops"), Some(2));
        assert!(func.properties.get_string("complexity_grade").is_some());
    }

    #[test]
    fn test_function_contained_by_known_parent() {
        let mut ir = CodeIR::new(PathBuf::from("test.f90"));
        ir.add_class(ClassEntity::new("hello", 1, 10));
        ir.add_function(FunctionEntity::new("inner", 2, 4).with_parent_class("hello"));

        let (graph, info) = map(&ir);
        let class_id = info.classes[0];
        let func_id = info.functions[0];

        // Contained by the class, not the file (classes map before functions).
        let edge_id = edge_between(&graph, class_id, func_id);
        assert_eq!(
            graph.get_edge(edge_id).unwrap().edge_type,
            EdgeType::Contains
        );
        assert!(graph
            .get_edges_between(info.file_id, func_id)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn test_function_unknown_parent_falls_back_to_file() {
        let mut ir = CodeIR::new(PathBuf::from("test.f90"));
        ir.add_function(FunctionEntity::new("orphan", 2, 4).with_parent_class("missing"));

        let (graph, info) = map(&ir);
        let func = graph.get_node(info.functions[0]).unwrap();
        // parent_class prop is still recorded even though the parent is absent.
        assert_eq!(func.properties.get_string("parent_class"), Some("missing"));

        let edge_id = edge_between(&graph, info.file_id, info.functions[0]);
        assert_eq!(
            graph.get_edge(edge_id).unwrap().edge_type,
            EdgeType::Contains
        );
    }

    #[test]
    fn test_import_external_module_with_symbols_and_wildcard() {
        let mut ir = CodeIR::new(PathBuf::from("test.f90"));
        ir.add_import(
            ImportRelation::new("file", "iso_fortran_env")
                .with_symbols(vec!["real64".to_string(), "int32".to_string()])
                .wildcard(),
        );

        let (graph, info) = map(&ir);
        assert_eq!(info.imports.len(), 1);

        let module = graph.get_node(info.imports[0]).unwrap();
        assert_eq!(module.node_type, NodeType::Module);
        assert_eq!(
            module.properties.get_string("name"),
            Some("iso_fortran_env")
        );
        assert_eq!(module.properties.get_string("is_external"), Some("true"));

        // Fortran records symbols (StringList) and is_wildcard on the Imports edge.
        let edge_id = edge_between(&graph, info.file_id, info.imports[0]);
        let edge = graph.get_edge(edge_id).unwrap();
        assert_eq!(edge.edge_type, EdgeType::Imports);
        assert_eq!(edge.properties.get_string("is_wildcard"), Some("true"));
        assert_eq!(
            edge.properties.get_string_list_compat("symbols"),
            Some(vec!["real64".to_string(), "int32".to_string()])
        );
    }

    #[test]
    fn test_duplicate_import_dedup() {
        let mut ir = CodeIR::new(PathBuf::from("test.f90"));
        ir.add_import(ImportRelation::new("file", "mylib"));
        ir.add_import(ImportRelation::new("file", "mylib"));

        let (graph, info) = map(&ir);
        assert_eq!(info.imports.len(), 2);
        // Both imports resolve to a single Module node (file + one module = 2 nodes).
        assert_eq!(graph.node_count(), 2);
        assert_eq!(info.imports[0], info.imports[1]);
        // Two Imports edges from the file to the same module.
        assert_eq!(
            graph
                .get_edges_between(info.file_id, info.imports[0])
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn test_resolved_and_unresolved_calls() {
        let mut ir = CodeIR::new(PathBuf::from("test.f90"));
        ir.add_function(FunctionEntity::new("caller", 1, 5));
        ir.add_function(FunctionEntity::new("callee", 6, 10));
        // Resolved: both endpoints known.
        ir.add_call(CallRelation::new("caller", "callee", 3));
        // Unresolved: callee not in node_map.
        ir.add_call(CallRelation::new("caller", "external_sub", 4));

        let (graph, info) = map(&ir);
        let caller_id = info.functions[0];
        let callee_id = info.functions[1];

        // Resolved call becomes a Calls edge with call_site_line + is_direct.
        let edge_id = edge_between(&graph, caller_id, callee_id);
        let edge = graph.get_edge(edge_id).unwrap();
        assert_eq!(edge.edge_type, EdgeType::Calls);
        assert_eq!(edge.properties.get_int("call_site_line"), Some(3));
        assert_eq!(edge.properties.get_bool("is_direct"), Some(true));

        // Unresolved call stored as a list prop on the caller, no edge created.
        let caller = graph.get_node(caller_id).unwrap();
        assert_eq!(
            caller.properties.get_string_list_compat("unresolved_calls"),
            Some(vec!["external_sub".to_string()])
        );
    }

    #[test]
    fn test_property_types() {
        let mut ir = CodeIR::new(PathBuf::from("test.f90"));
        ir.set_module(ModuleEntity::new("test", "test.f90", "fortran").with_line_count(50));
        ir.add_function(FunctionEntity::new("compute", 5, 15));

        let (graph, info) = map(&ir);

        let file_node = graph.get_node(info.file_id).unwrap();
        assert!(matches!(
            file_node.properties.get("line_count"),
            Some(PropertyValue::Int(50))
        ));

        let func_node = graph.get_node(info.functions[0]).unwrap();
        assert!(matches!(
            func_node.properties.get("line_start"),
            Some(PropertyValue::Int(5))
        ));
    }
}
