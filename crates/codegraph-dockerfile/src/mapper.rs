// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Mapper for converting CodeIR to CodeGraph nodes and edges.
//!
//! Each Dockerfile directive becomes a `Function` node attached to the file via
//! a `Contains` edge. The IaC security scanner queries these function nodes and
//! matches their `body_prefix` against rule patterns.

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
    let class_ids = Vec::new();
    let trait_ids = Vec::new();
    let import_ids = Vec::new();

    // Create file/module node
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
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("Dockerfile")
            .to_string();
        let props = PropertyMap::new()
            .with("name", name.clone())
            .with("path", file_path.display().to_string())
            .with("language", "dockerfile");

        let id = graph
            .add_node(NodeType::CodeFile, props)
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
        node_map.insert(name, id);
        id
    };

    // Add directives as function nodes
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
        if let Some(ref body) = func.body_prefix {
            props = props.with("body_prefix", body.clone());
        }

        let func_id = graph
            .add_node(NodeType::Function, props)
            .map_err(|e| ParserError::GraphError(e.to_string()))?;

        function_ids.push(func_id);

        graph
            .add_edge(file_id, func_id, EdgeType::Contains, PropertyMap::new())
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
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
    use codegraph_parser_api::{FunctionEntity, ModuleEntity};
    use std::path::PathBuf;

    fn map(ir: &CodeIR) -> (CodeGraph, FileInfo) {
        map_with(ir, Path::new("Dockerfile"))
    }

    fn map_with(ir: &CodeIR, path: &Path) -> (CodeGraph, FileInfo) {
        let mut graph = CodeGraph::in_memory().unwrap();
        let info = ir_to_graph(ir, &mut graph, path).unwrap();
        (graph, info)
    }

    /// Return the single edge between two nodes (fails if not exactly one).
    fn edge_between(graph: &CodeGraph, src: NodeId, dst: NodeId) -> &codegraph::Edge {
        let ids = graph.get_edges_between(src, dst).unwrap();
        assert_eq!(ids.len(), 1, "expected exactly one edge {src}->{dst}");
        graph.get_edge(ids[0]).unwrap()
    }

    #[test]
    fn empty_ir_yields_file_node_from_path_name() {
        let ir = CodeIR::new(PathBuf::from("Dockerfile"));
        let (graph, info) = map(&ir);

        assert_eq!(info.functions.len(), 0);
        assert_eq!(graph.node_count(), 1);
        assert_eq!(graph.edge_count(), 0);
        assert_eq!(info.line_count, 0);

        let file = graph.get_node(info.file_id).unwrap();
        assert!(matches!(file.node_type, NodeType::CodeFile));
        assert_eq!(file.properties.get_string("name"), Some("Dockerfile"));
        assert_eq!(file.properties.get_string("language"), Some("dockerfile"));
        assert_eq!(file.properties.get_string("path"), Some("Dockerfile"));
    }

    #[test]
    fn file_name_uses_full_filename_including_extension() {
        let ir = CodeIR::new(PathBuf::from("build/app.dockerfile"));
        let (graph, info) = map_with(&ir, Path::new("build/app.dockerfile"));

        let file = graph.get_node(info.file_id).unwrap();
        // Unlike source-language mappers that stem the name, the dockerfile
        // fallback keeps the full file_name (extension included).
        assert_eq!(file.properties.get_string("name"), Some("app.dockerfile"));
        assert_eq!(
            file.properties.get_string("path"),
            Some("build/app.dockerfile")
        );
    }

    #[test]
    fn missing_file_name_falls_back_to_dockerfile() {
        let ir = CodeIR::new(PathBuf::from(".."));
        // Path::new("..").file_name() is None, triggering the "Dockerfile" default.
        let (graph, info) = map_with(&ir, Path::new(".."));

        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(file.properties.get_string("name"), Some("Dockerfile"));
    }

    #[test]
    fn module_drives_file_metadata_and_line_count() {
        let mut ir = CodeIR::new(PathBuf::from("Dockerfile"));
        ir.set_module(
            ModuleEntity::new("Dockerfile", "docker/Dockerfile", "dockerfile")
                .with_line_count(17)
                .with_doc("build image"),
        );
        let (graph, info) = map(&ir);

        assert_eq!(info.line_count, 17);
        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(file.properties.get_string("name"), Some("Dockerfile"));
        assert_eq!(
            file.properties.get_string("path"),
            Some("docker/Dockerfile")
        );
        assert_eq!(file.properties.get_int("line_count"), Some(17));
        assert_eq!(file.properties.get_string("doc"), Some("build image"));
    }

    #[test]
    fn directive_becomes_function_with_contains_edge() {
        let mut ir = CodeIR::new(PathBuf::from("Dockerfile"));
        let mut from_dir = FunctionEntity::new("FROM", 1, 1);
        from_dir.body_prefix = Some("FROM python:3.11".to_string());
        ir.add_function(from_dir);

        let (graph, info) = map(&ir);
        assert_eq!(info.functions.len(), 1);
        assert_eq!(graph.node_count(), 2);

        let func_id = info.functions[0];
        let func = graph.get_node(func_id).unwrap();
        assert!(matches!(func.node_type, NodeType::Function));
        assert_eq!(func.properties.get_string("name"), Some("FROM"));
        assert_eq!(
            func.properties.get_string("body_prefix"),
            Some("FROM python:3.11")
        );
        assert_eq!(func.properties.get_string("path"), Some("Dockerfile"));

        let edge = edge_between(&graph, info.file_id, func_id);
        assert!(matches!(edge.edge_type, EdgeType::Contains));
    }

    #[test]
    fn function_flags_and_line_bounds_are_recorded() {
        let mut ir = CodeIR::new(PathBuf::from("Dockerfile"));
        ir.add_function(FunctionEntity::new("RUN", 3, 5).with_visibility("private"));

        let (graph, info) = map(&ir);
        let func = graph.get_node(info.functions[0]).unwrap();
        assert_eq!(func.properties.get_int("line_start"), Some(3));
        assert_eq!(func.properties.get_int("line_end"), Some(5));
        assert_eq!(func.properties.get_string("visibility"), Some("private"));
        assert_eq!(func.properties.get_bool("is_async"), Some(false));
        assert_eq!(func.properties.get_bool("is_static"), Some(false));
        assert_eq!(func.properties.get_bool("is_abstract"), Some(false));
        assert_eq!(func.properties.get_bool("is_test"), Some(false));
    }

    #[test]
    fn directive_doc_comment_maps_to_doc_prop() {
        let mut ir = CodeIR::new(PathBuf::from("Dockerfile"));
        ir.add_function(FunctionEntity::new("CMD", 8, 8).with_doc("entry command"));

        let (graph, info) = map(&ir);
        let func = graph.get_node(info.functions[0]).unwrap();
        assert_eq!(func.properties.get_string("doc"), Some("entry command"));
    }

    #[test]
    fn absent_body_prefix_and_doc_leave_props_unset() {
        let mut ir = CodeIR::new(PathBuf::from("Dockerfile"));
        ir.add_function(FunctionEntity::new("EXPOSE", 9, 9));

        let (graph, info) = map(&ir);
        let func = graph.get_node(info.functions[0]).unwrap();
        assert_eq!(func.properties.get_string("body_prefix"), None);
        assert_eq!(func.properties.get_string("doc"), None);
    }

    #[test]
    fn multiple_directives_each_get_contains_edge() {
        let mut ir = CodeIR::new(PathBuf::from("Dockerfile"));
        for (i, name) in ["FROM", "USER", "RUN"].iter().enumerate() {
            ir.add_function(FunctionEntity::new(*name, i + 1, i + 1));
        }

        let (graph, info) = map(&ir);
        assert_eq!(info.functions.len(), 3);
        // 1 file node + 3 directive function nodes.
        assert_eq!(graph.node_count(), 4);
        // One Contains edge per directive.
        assert_eq!(graph.edge_count(), 3);
        for func_id in &info.functions {
            let edge = edge_between(&graph, info.file_id, *func_id);
            assert!(matches!(edge.edge_type, EdgeType::Contains));
        }
    }

    #[test]
    fn classes_traits_imports_are_always_empty() {
        let mut ir = CodeIR::new(PathBuf::from("Dockerfile"));
        ir.add_function(FunctionEntity::new("FROM", 1, 1));

        let (_graph, info) = map(&ir);
        // The dockerfile mapper never emits class/trait/import nodes.
        assert!(info.classes.is_empty());
        assert!(info.traits.is_empty());
        assert!(info.imports.is_empty());
    }
}
