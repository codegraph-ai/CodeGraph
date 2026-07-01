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
            .with("language", "cobol");

        let id = graph
            .add_node(NodeType::CodeFile, props)
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
        node_map.insert(name, id);
        id
    };

    // Add COBOL programs as class nodes
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

    // Add COBOL paragraphs as function nodes
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

        // Link paragraph to its parent program or file
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

    // Add import relationships (COPY statements)
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

    // Add call relationships (CALL statements)
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
        let info = ir_to_graph(ir, &mut graph, Path::new("test.cob")).unwrap();
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
        // No module set: name derives from the file stem, language is the
        // hard-coded "cobol", the graph holds only the file node, line_count is 0.
        let ir = CodeIR::new(PathBuf::from("test.cob"));
        let (graph, info) = map(&ir);

        assert_eq!(graph.node_count(), 1);
        assert_eq!(graph.edge_count(), 0);
        assert_eq!(info.functions.len(), 0);
        assert_eq!(info.classes.len(), 0);
        assert_eq!(info.line_count, 0);

        let file = graph.get_node(info.file_id).unwrap();
        assert!(matches!(file.node_type, NodeType::CodeFile));
        assert_eq!(file.properties.get_string("name"), Some("test"));
        assert_eq!(file.properties.get_string("language"), Some("cobol"));
    }

    #[test]
    fn module_drives_file_metadata() {
        // When a module is set, the file node takes its name/path/language and
        // line_count, and FileInfo.line_count mirrors the module value.
        let mut ir = CodeIR::new(PathBuf::from("test.cob"));
        ir.set_module(ModuleEntity::new("PAYROLL", "src/PAYROLL.cob", "cobol").with_line_count(80));
        let (graph, info) = map(&ir);

        assert_eq!(info.line_count, 80);
        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(file.properties.get_string("name"), Some("PAYROLL"));
        assert_eq!(file.properties.get_string("path"), Some("src/PAYROLL.cob"));
        assert!(matches!(
            file.properties.get("line_count"),
            Some(PropertyValue::Int(80))
        ));
    }

    #[test]
    fn module_doc_comment_is_emitted_on_file_node() {
        // A module carrying a doc_comment stamps the optional `doc` prop on the
        // file node (mapper.rs:31-33), an arm module_drives_file_metadata leaves
        // unset.
        let mut ir = CodeIR::new(PathBuf::from("test.cob"));
        ir.set_module(
            ModuleEntity::new("PAYROLL", "src/PAYROLL.cob", "cobol").with_doc("payroll batch"),
        );
        let (graph, info) = map(&ir);

        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(file.properties.get_string("doc"), Some("payroll batch"));
    }

    #[test]
    fn program_doc_and_body_prefix_are_emitted_on_class_node() {
        // A program carrying doc_comment and body_prefix stamps both optional
        // props on the Class node (mapper.rs:68-73); program_becomes_class_node
        // leaves both unset.
        let mut ir = CodeIR::new(PathBuf::from("test.cob"));
        ir.add_class(
            ClassEntity::new("MYPROG", 1, 20)
                .with_doc("main program")
                .with_body_prefix("IDENTIFICATION DIVISION."),
        );
        let (graph, info) = map(&ir);

        let class = graph.get_node(info.classes[0]).unwrap();
        assert_eq!(class.properties.get_string("doc"), Some("main program"));
        assert_eq!(
            class.properties.get_string("body_prefix"),
            Some("IDENTIFICATION DIVISION.")
        );
    }

    #[test]
    fn paragraph_doc_and_body_prefix_are_emitted_on_function_node() {
        // A paragraph carrying doc_comment and body_prefix stamps both optional
        // props on the Function node (mapper.rs:99-104), arms no prior function
        // test populated.
        let mut ir = CodeIR::new(PathBuf::from("test.cob"));
        ir.add_function(
            FunctionEntity::new("INIT-PARA", 5, 10)
                .with_doc("initializes state")
                .with_body_prefix("MOVE 0 TO WS-COUNT."),
        );
        let (graph, info) = map(&ir);

        let func = graph.get_node(info.functions[0]).unwrap();
        assert_eq!(func.properties.get_string("doc"), Some("initializes state"));
        assert_eq!(
            func.properties.get_string("body_prefix"),
            Some("MOVE 0 TO WS-COUNT.")
        );
    }

    #[test]
    fn no_module_path_without_stem_falls_back_to_unknown() {
        // With no module and a path that has no file_stem, the file node name
        // falls back to "unknown" (mapper.rs:41-44); the map() helper always uses
        // test.cob so this arm was never reached.
        let ir = CodeIR::new(PathBuf::from(".."));
        let mut graph = CodeGraph::in_memory().unwrap();
        let info = ir_to_graph(&ir, &mut graph, Path::new("..")).unwrap();

        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(file.properties.get_string("name"), Some("unknown"));
    }

    #[test]
    fn program_becomes_class_node_with_file_contains_edge() {
        // A COBOL program maps to a Class node carrying name/visibility/line
        // bounds/is_abstract, wired file -> class via Contains.
        let mut ir = CodeIR::new(PathBuf::from("test.cob"));
        ir.add_class(ClassEntity::new("MYPROG", 1, 20));
        let (graph, info) = map(&ir);

        assert_eq!(info.classes.len(), 1);
        let class_id = info.classes[0];
        let edge = edge_between(&graph, info.file_id, class_id);
        assert!(matches!(edge.edge_type, EdgeType::Contains));

        let class = graph.get_node(class_id).unwrap();
        assert!(matches!(class.node_type, NodeType::Class));
        assert_eq!(class.properties.get_string("name"), Some("MYPROG"));
        assert_eq!(class.properties.get_string("visibility"), Some("public"));
        assert!(matches!(
            class.properties.get("line_start"),
            Some(PropertyValue::Int(1))
        ));
        assert!(matches!(
            class.properties.get("line_end"),
            Some(PropertyValue::Int(20))
        ));
        assert!(matches!(
            class.properties.get("is_abstract"),
            Some(PropertyValue::Bool(false))
        ));
    }

    #[test]
    fn free_paragraph_gets_file_contains_edge_with_flags() {
        // A paragraph with no parent_class is wired file -> function via
        // Contains, keeps its bare name, and carries the boolean flags.
        let mut ir = CodeIR::new(PathBuf::from("test.cob"));
        ir.add_function(FunctionEntity::new("MAIN-PARA", 5, 15).with_visibility("public"));
        let (graph, info) = map(&ir);

        assert_eq!(info.functions.len(), 1);
        let func_id = info.functions[0];
        let edge = edge_between(&graph, info.file_id, func_id);
        assert!(matches!(edge.edge_type, EdgeType::Contains));

        let func = graph.get_node(func_id).unwrap();
        assert!(matches!(func.node_type, NodeType::Function));
        assert_eq!(func.properties.get_string("name"), Some("MAIN-PARA"));
        assert!(matches!(
            func.properties.get("is_async"),
            Some(PropertyValue::Bool(false))
        ));
        assert!(matches!(
            func.properties.get("is_static"),
            Some(PropertyValue::Bool(false))
        ));
        // No parent_class was set, so the prop is absent.
        assert!(func.properties.get("parent_class").is_none());
    }

    #[test]
    fn paragraph_with_known_parent_is_contained_by_program() {
        // Classes are mapped before functions, so a paragraph whose parent_class
        // matches a program is contained by the Class node (class -> func), not
        // the file.
        let mut ir = CodeIR::new(PathBuf::from("test.cob"));
        ir.add_class(ClassEntity::new("MYPROG", 1, 40));
        ir.add_function(FunctionEntity::new("INIT-PARA", 5, 10).with_parent_class("MYPROG"));
        let (graph, info) = map(&ir);

        let class_id = info.classes[0];
        let func_id = info.functions[0];

        // class -> func Contains edge exists.
        let edge = edge_between(&graph, class_id, func_id);
        assert!(matches!(edge.edge_type, EdgeType::Contains));

        // No file -> func edge (only file -> class).
        assert!(graph
            .get_edges_between(info.file_id, func_id)
            .unwrap()
            .is_empty());

        let func = graph.get_node(func_id).unwrap();
        assert_eq!(func.properties.get_string("parent_class"), Some("MYPROG"));
    }

    #[test]
    fn paragraph_with_unknown_parent_falls_back_to_file() {
        // A parent_class not present in the graph records the prop but the
        // containment edge falls back to the file.
        let mut ir = CodeIR::new(PathBuf::from("test.cob"));
        ir.add_function(FunctionEntity::new("ORPHAN-PARA", 5, 10).with_parent_class("GHOST"));
        let (graph, info) = map(&ir);

        let func_id = info.functions[0];
        let edge = edge_between(&graph, info.file_id, func_id);
        assert!(matches!(edge.edge_type, EdgeType::Contains));

        let func = graph.get_node(func_id).unwrap();
        assert_eq!(func.properties.get_string("parent_class"), Some("GHOST"));
    }

    #[test]
    fn paragraph_records_complexity_props() {
        // A function carrying ComplexityMetrics stamps the complexity family of
        // props (value, grade, branches, loops) onto the Function node.
        let metrics = ComplexityMetrics::new()
            .with_branches(3)
            .with_loops(2)
            .finalize();
        let mut ir = CodeIR::new(PathBuf::from("test.cob"));
        ir.add_function(FunctionEntity::new("BUSY-PARA", 1, 30).with_complexity(metrics.clone()));
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
            Some(metrics.grade().to_string()).as_deref()
        );
    }

    #[test]
    fn copy_creates_external_module_with_bare_import_edge() {
        // A COPY statement creates an external Module node (is_external=true) and
        // a bare Imports edge with no symbol/alias props.
        let mut ir = CodeIR::new(PathBuf::from("test.cob"));
        ir.add_import(ImportRelation::new("MYPROG", "MYBOOK").with_alias("BK"));
        let (graph, info) = map(&ir);

        assert_eq!(info.imports.len(), 1);
        let module_id = info.imports[0];
        let module = graph.get_node(module_id).unwrap();
        assert!(matches!(module.node_type, NodeType::Module));
        assert_eq!(module.properties.get_string("name"), Some("MYBOOK"));
        assert_eq!(module.properties.get_string("is_external"), Some("true"));

        let edge = edge_between(&graph, info.file_id, module_id);
        assert!(matches!(edge.edge_type, EdgeType::Imports));
        // Import edge carries no symbol/alias metadata.
        assert!(edge.properties.get("symbols").is_none());
        assert!(edge.properties.get("alias").is_none());
    }

    #[test]
    fn duplicate_copy_targets_dedup_to_one_module() {
        // Two COPY statements naming the same book reuse one Module node while
        // each still contributes an Imports edge.
        let mut ir = CodeIR::new(PathBuf::from("test.cob"));
        ir.add_import(ImportRelation::new("MYPROG", "SHARED"));
        ir.add_import(ImportRelation::new("MYPROG", "SHARED"));
        let (graph, info) = map(&ir);

        // file node + one deduped Module node.
        assert_eq!(graph.node_count(), 2);
        assert_eq!(info.imports.len(), 2);
        assert_eq!(info.imports[0], info.imports[1]);

        let ids = graph
            .get_edges_between(info.file_id, info.imports[0])
            .unwrap();
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn resolved_call_creates_calls_edge() {
        // A CALL between two known paragraphs creates a Calls edge carrying the
        // call_site_line and is_direct props.
        let mut ir = CodeIR::new(PathBuf::from("test.cob"));
        ir.add_function(FunctionEntity::new("CALLER", 1, 10));
        ir.add_function(FunctionEntity::new("CALLEE", 11, 20));
        ir.add_call(CallRelation::new("CALLER", "CALLEE", 5));
        let (graph, info) = map(&ir);

        let caller_id = info.functions[0];
        let callee_id = info.functions[1];
        let edge = edge_between(&graph, caller_id, callee_id);
        assert!(matches!(edge.edge_type, EdgeType::Calls));
        assert!(matches!(
            edge.properties.get("call_site_line"),
            Some(PropertyValue::Int(5))
        ));
        assert!(matches!(
            edge.properties.get("is_direct"),
            Some(PropertyValue::Bool(true))
        ));
    }

    #[test]
    fn unresolved_call_stored_on_caller_without_edge() {
        // A CALL whose callee is not in the graph produces no Calls edge; the
        // callee name is stored on the caller's unresolved_calls list.
        let mut ir = CodeIR::new(PathBuf::from("test.cob"));
        ir.add_function(FunctionEntity::new("CALLER", 1, 10));
        ir.add_call(CallRelation::new("CALLER", "EXTERNAL-PROG", 5));
        let (graph, info) = map(&ir);

        // Only file + caller nodes, and the single file -> caller Contains edge.
        assert_eq!(graph.node_count(), 2);
        assert_eq!(graph.edge_count(), 1);

        let caller = graph.get_node(info.functions[0]).unwrap();
        let unresolved = caller
            .properties
            .get_string_list_compat("unresolved_calls")
            .unwrap();
        assert_eq!(unresolved, vec!["EXTERNAL-PROG".to_string()]);
    }
}
