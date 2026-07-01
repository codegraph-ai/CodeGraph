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
            .with("language", "perl");

        let id = graph
            .add_node(NodeType::CodeFile, props)
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
        node_map.insert(name, id);
        id
    };

    // Add packages as classes
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

        let class_id = graph
            .add_node(NodeType::Class, props)
            .map_err(|e| ParserError::GraphError(e.to_string()))?;

        node_map.insert(class.name.clone(), class_id);
        class_ids.push(class_id);

        graph
            .add_edge(file_id, class_id, EdgeType::Contains, PropertyMap::new())
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
    }

    // Add functions/subroutines
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
            .with("is_abstract", func.is_abstract)
            .with("is_test", func.is_test);

        if let Some(ref doc) = func.doc_comment {
            props = props.with("doc", doc.clone());
        }
        if let Some(ref parent) = func.parent_class {
            props = props.with("parent_class", parent.clone());
        }
        if !func.parameters.is_empty() {
            let param_names: Vec<String> = func.parameters.iter().map(|p| p.name.clone()).collect();
            props = props.with("parameters", param_names);
        }
        if let Some(ref body) = func.body_prefix {
            props = props.with("body_prefix", body.clone());
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

        // Link to parent class (package) if present, otherwise to file
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

    // Add call relationships
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
    use codegraph::PropertyValue;
    use codegraph_parser_api::{
        CallRelation, ClassEntity, ComplexityMetrics, FunctionEntity, ImportRelation, ModuleEntity,
    };
    use std::path::PathBuf;

    fn map(ir: &CodeIR) -> (CodeGraph, FileInfo) {
        let mut graph = CodeGraph::in_memory().unwrap();
        let info = ir_to_graph(ir, &mut graph, Path::new("test.pl")).unwrap();
        (graph, info)
    }

    /// Return the single edge between two nodes (fails if not exactly one).
    fn edge_between(graph: &CodeGraph, src: NodeId, dst: NodeId) -> &codegraph::Edge {
        let ids = graph.get_edges_between(src, dst).unwrap();
        assert_eq!(ids.len(), 1, "expected exactly one edge {src}->{dst}");
        graph.get_edge(ids[0]).unwrap()
    }

    #[test]
    fn empty_ir_builds_file_node_from_path_stem() {
        // No module set: name derives from the file stem, language is hard-coded
        // "perl", the graph holds only the file node, and line_count is 0.
        let ir = CodeIR::new(PathBuf::from("test.pl"));
        let (graph, info) = map(&ir);

        assert_eq!(graph.node_count(), 1);
        assert_eq!(graph.edge_count(), 0);
        assert_eq!(info.functions.len(), 0);
        assert_eq!(info.classes.len(), 0);
        assert_eq!(info.line_count, 0);

        let file = graph.get_node(info.file_id).unwrap();
        assert!(matches!(file.node_type, NodeType::CodeFile));
        assert_eq!(file.properties.get_string("name"), Some("test"));
        assert_eq!(file.properties.get_string("language"), Some("perl"));
    }

    #[test]
    fn module_drives_file_metadata() {
        // When a module is set, the file node takes its name/path/language and
        // line_count, and FileInfo.line_count mirrors the module value.
        let mut ir = CodeIR::new(PathBuf::from("test.pl"));
        ir.set_module(ModuleEntity::new("MyApp", "lib/MyApp.pm", "perl").with_line_count(42));
        let (graph, info) = map(&ir);

        assert_eq!(info.line_count, 42);
        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(file.properties.get_string("name"), Some("MyApp"));
        assert_eq!(file.properties.get_string("path"), Some("lib/MyApp.pm"));
        assert!(matches!(
            file.properties.get("line_count"),
            Some(PropertyValue::Int(42))
        ));
    }

    #[test]
    fn free_function_gets_file_contains_edge() {
        // A subroutine with no parent_class is wired file -> function via
        // Contains, keeps its bare name, and carries the boolean flags.
        let mut ir = CodeIR::new(PathBuf::from("test.pl"));
        ir.add_function(FunctionEntity::new("greet", 1, 5).with_visibility("public"));
        let (graph, info) = map(&ir);

        assert_eq!(info.functions.len(), 1);
        let func_id = info.functions[0];
        let edge = edge_between(&graph, info.file_id, func_id);
        assert!(matches!(edge.edge_type, EdgeType::Contains));

        let func = graph.get_node(func_id).unwrap();
        assert_eq!(func.properties.get_string("name"), Some("greet"));
        assert!(matches!(func.node_type, NodeType::Function));
        assert!(matches!(
            func.properties.get("is_async"),
            Some(PropertyValue::Bool(false))
        ));
        assert!(matches!(
            func.properties.get("is_static"),
            Some(PropertyValue::Bool(false))
        ));
    }

    #[test]
    fn function_complexity_props_recorded() {
        // A function carrying ComplexityMetrics gets the full complexity prop set.
        let metrics = ComplexityMetrics::new()
            .with_branches(3)
            .with_loops(2)
            .finalize();
        let grade = metrics.grade();
        let mut ir = CodeIR::new(PathBuf::from("test.pl"));
        ir.add_function(FunctionEntity::new("busy", 1, 30).with_complexity(metrics));
        let (graph, info) = map(&ir);

        let func = graph.get_node(info.functions[0]).unwrap();
        assert!(matches!(
            func.properties.get("complexity_branches"),
            Some(PropertyValue::Int(3))
        ));
        assert!(matches!(
            func.properties.get("complexity_loops"),
            Some(PropertyValue::Int(2))
        ));
        assert_eq!(
            func.properties.get_string("complexity_grade"),
            Some(grade.to_string().as_str())
        );
    }

    #[test]
    fn package_emits_class_node_with_file_contains() {
        // A package maps to a Class node wired file -> class via Contains and
        // carries the is_abstract flag.
        let mut ir = CodeIR::new(PathBuf::from("User.pm"));
        ir.add_class(ClassEntity::new("MyApp::User", 1, 20));
        let (graph, info) = map(&ir);

        assert_eq!(info.classes.len(), 1);
        let class_id = info.classes[0];
        let class = graph.get_node(class_id).unwrap();
        assert!(matches!(class.node_type, NodeType::Class));
        assert_eq!(class.properties.get_string("name"), Some("MyApp::User"));
        assert!(matches!(
            class.properties.get("is_abstract"),
            Some(PropertyValue::Bool(false))
        ));
        let edge = edge_between(&graph, info.file_id, class_id);
        assert!(matches!(edge.edge_type, EdgeType::Contains));
    }

    #[test]
    fn method_with_known_parent_links_to_class_not_file() {
        // Classes are mapped before functions, so a sub whose parent_class matches
        // a package is contained by the class - with no file -> function edge.
        let mut ir = CodeIR::new(PathBuf::from("User.pm"));
        ir.add_class(ClassEntity::new("MyApp::User", 1, 20));
        ir.add_function(FunctionEntity::new("login", 5, 8).with_parent_class("MyApp::User"));
        let (graph, info) = map(&ir);

        let class_id = info.classes[0];
        let func_id = info.functions[0];
        let edge = edge_between(&graph, class_id, func_id);
        assert!(matches!(edge.edge_type, EdgeType::Contains));
        // No direct file -> method containment edge.
        assert!(graph
            .get_edges_between(info.file_id, func_id)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn method_with_unknown_parent_falls_back_to_file() {
        // A parent_class not present as a mapped package falls back to a
        // file -> function Contains edge.
        let mut ir = CodeIR::new(PathBuf::from("test.pl"));
        ir.add_function(FunctionEntity::new("orphan", 1, 3).with_parent_class("Missing::Pkg"));
        let (graph, info) = map(&ir);

        let func_id = info.functions[0];
        let edge = edge_between(&graph, info.file_id, func_id);
        assert!(matches!(edge.edge_type, EdgeType::Contains));
    }

    #[test]
    fn import_creates_external_module_with_bare_edge() {
        // An import creates an external Module node and a bare Imports edge
        // (the perl mapper records no symbols/alias props on the edge).
        let mut ir = CodeIR::new(PathBuf::from("test.pl"));
        ir.add_import(ImportRelation::new("main", "POSIX").with_symbols(vec!["floor".to_string()]));
        let (graph, info) = map(&ir);

        assert_eq!(info.imports.len(), 1);
        let module_id = info.imports[0];
        let module = graph.get_node(module_id).unwrap();
        assert!(matches!(module.node_type, NodeType::Module));
        assert_eq!(module.properties.get_string("name"), Some("POSIX"));
        assert_eq!(module.properties.get_string("is_external"), Some("true"));

        let edge = edge_between(&graph, info.file_id, module_id);
        assert!(matches!(edge.edge_type, EdgeType::Imports));
        assert!(edge.properties.get("symbols").is_none());
        assert!(edge.properties.get("alias").is_none());
    }

    #[test]
    fn duplicate_import_reuses_single_module_node() {
        // Two imports of the same target share one Module node but yield two edges.
        let mut ir = CodeIR::new(PathBuf::from("test.pl"));
        ir.add_import(ImportRelation::new("main", "POSIX"));
        ir.add_import(ImportRelation::new("main", "POSIX"));
        let (graph, info) = map(&ir);

        assert_eq!(info.imports.len(), 2);
        assert_eq!(info.imports[0], info.imports[1]);
        let ids = graph
            .get_edges_between(info.file_id, info.imports[0])
            .unwrap();
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn resolved_call_creates_calls_edge() {
        // A call between two known subroutines yields a Calls edge carrying the
        // call site line and is_direct flag.
        let mut ir = CodeIR::new(PathBuf::from("test.pl"));
        ir.add_function(FunctionEntity::new("caller_sub", 1, 5));
        ir.add_function(FunctionEntity::new("callee_sub", 6, 10));
        ir.add_call(CallRelation::new("caller_sub", "callee_sub", 3));
        let (graph, info) = map(&ir);

        let caller_id = info.functions[0];
        let callee_id = info.functions[1];
        let edge = edge_between(&graph, caller_id, callee_id);
        assert!(matches!(edge.edge_type, EdgeType::Calls));
        assert!(matches!(
            edge.properties.get("call_site_line"),
            Some(PropertyValue::Int(3))
        ));
        assert!(matches!(
            edge.properties.get("is_direct"),
            Some(PropertyValue::Bool(true))
        ));
    }

    #[test]
    fn unresolved_call_creates_no_edge() {
        // A call whose callee is not a mapped node produces no Calls edge.
        let mut ir = CodeIR::new(PathBuf::from("test.pl"));
        ir.add_function(FunctionEntity::new("caller_sub", 1, 5));
        ir.add_call(CallRelation::new("caller_sub", "nonexistent", 3));
        let (graph, info) = map(&ir);

        let caller_id = info.functions[0];
        // Only the file -> function Contains edge exists; no Calls edge added.
        assert_eq!(graph.edge_count(), 1);
        assert!(graph
            .get_edges_between(caller_id, info.file_id)
            .unwrap()
            .is_empty());
    }
}
