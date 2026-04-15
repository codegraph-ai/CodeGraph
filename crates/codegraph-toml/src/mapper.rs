// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Mapper for converting TOML CodeIR to CodeGraph nodes and edges

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

    // Create module/file node
    let file_id = if let Some(ref module) = ir.module {
        let props = PropertyMap::new()
            .with("name", module.name.clone())
            .with("path", module.path.clone())
            .with("language", module.language.clone())
            .with("line_count", module.line_count as i64);

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
            .with("language", "toml");

        let id = graph
            .add_node(NodeType::CodeFile, props)
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
        node_map.insert(name, id);
        id
    };

    // Add table sections as Class nodes
    for class in &ir.classes {
        let props = PropertyMap::new()
            .with("name", class.name.clone())
            .with("path", file_path.display().to_string())
            .with("visibility", class.visibility.clone())
            .with("line_start", class.line_start as i64)
            .with("line_end", class.line_end as i64)
            .with("language", "toml");

        let class_id = graph
            .add_node(NodeType::Class, props)
            .map_err(|e| ParserError::GraphError(e.to_string()))?;

        node_map.insert(class.name.clone(), class_id);
        class_ids.push(class_id);

        // Link section to file
        graph
            .add_edge(file_id, class_id, EdgeType::Contains, PropertyMap::new())
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
    }

    // Add key-value pairs as Function nodes (property proxy)
    for func in &ir.functions {
        let mut props = PropertyMap::new()
            .with("name", func.name.clone())
            .with("path", file_path.display().to_string())
            .with("signature", func.signature.clone())
            .with("visibility", func.visibility.clone())
            .with("line_start", func.line_start as i64)
            .with("line_end", func.line_end as i64)
            .with("language", "toml")
            .with("is_async", false)
            .with("is_static", false)
            .with("is_abstract", false)
            .with("is_test", false);

        if let Some(ref parent) = func.parent_class {
            props = props.with("parent_class", parent.clone());
        }

        let func_id = graph
            .add_node(NodeType::Function, props)
            .map_err(|e| ParserError::GraphError(e.to_string()))?;

        node_map.insert(func.name.clone(), func_id);
        function_ids.push(func_id);

        // Link to parent section or file
        if let Some(ref parent_name) = func.parent_class {
            if let Some(&section_id) = node_map.get(parent_name) {
                graph
                    .add_edge(section_id, func_id, EdgeType::Contains, PropertyMap::new())
                    .map_err(|e| ParserError::GraphError(e.to_string()))?;
            } else {
                // Section not yet seen — link to file
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

    let line_count = ir.module.as_ref().map(|m| m.line_count).unwrap_or(0);

    Ok(FileInfo {
        file_path: file_path.to_path_buf(),
        file_id,
        functions: function_ids,
        classes: class_ids,
        traits: Vec::new(),
        imports: Vec::new(),
        parse_time: Duration::ZERO,
        line_count,
        byte_count: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph_parser_api::{ClassEntity, FunctionEntity};
    use std::path::PathBuf;

    #[test]
    fn test_ir_to_graph_empty() {
        let ir = CodeIR::new(PathBuf::from("test.toml"));
        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, &PathBuf::from("test.toml"));
        assert!(result.is_ok());
        let info = result.unwrap();
        assert_eq!(info.classes.len(), 0);
        assert_eq!(info.functions.len(), 0);
    }

    #[test]
    fn test_ir_to_graph_with_section() {
        let mut ir = CodeIR::new(PathBuf::from("Cargo.toml"));
        ir.add_class(ClassEntity::new("package", 1, 5));

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, &PathBuf::from("Cargo.toml"));
        assert!(result.is_ok());
        let info = result.unwrap();
        assert_eq!(info.classes.len(), 1);
    }

    #[test]
    fn test_ir_to_graph_with_keypair() {
        let mut ir = CodeIR::new(PathBuf::from("config.toml"));
        ir.add_class(ClassEntity::new("package", 1, 3));
        let mut f = FunctionEntity::new("package.name", 2, 2);
        f.parent_class = Some("package".to_string());
        f.signature = r#"name = "codegraph""#.to_string();
        ir.add_function(f);

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, &PathBuf::from("config.toml"));
        assert!(result.is_ok());
        let info = result.unwrap();
        assert_eq!(info.classes.len(), 1);
        assert_eq!(info.functions.len(), 1);
    }
}
