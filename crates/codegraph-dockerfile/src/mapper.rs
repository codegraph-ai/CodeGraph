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
    use codegraph_parser_api::FunctionEntity;
    use std::path::PathBuf;

    #[test]
    fn test_ir_to_graph_empty() {
        let ir = CodeIR::new(PathBuf::from("Dockerfile"));
        let mut graph = CodeGraph::in_memory().unwrap();
        let info = ir_to_graph(&ir, &mut graph, PathBuf::from("Dockerfile").as_path()).unwrap();
        assert_eq!(info.functions.len(), 0);
    }

    #[test]
    fn test_ir_to_graph_with_directives() {
        let mut ir = CodeIR::new(PathBuf::from("Dockerfile"));
        let mut from_dir = FunctionEntity::new("FROM", 1, 1);
        from_dir.body_prefix = Some("FROM python:3.11".to_string());
        ir.add_function(from_dir);

        let mut user_dir = FunctionEntity::new("USER", 2, 2);
        user_dir.body_prefix = Some("USER root".to_string());
        ir.add_function(user_dir);

        let mut graph = CodeGraph::in_memory().unwrap();
        let info = ir_to_graph(&ir, &mut graph, PathBuf::from("Dockerfile").as_path()).unwrap();
        assert_eq!(info.functions.len(), 2);
    }
}
