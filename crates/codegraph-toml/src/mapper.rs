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
    use codegraph::PropertyValue;
    use codegraph_parser_api::{ClassEntity, FunctionEntity, ModuleEntity};
    use std::path::PathBuf;

    fn map(ir: &CodeIR, path: &str) -> (CodeGraph, FileInfo) {
        let mut graph = CodeGraph::in_memory().unwrap();
        let info = ir_to_graph(ir, &mut graph, &PathBuf::from(path)).unwrap();
        (graph, info)
    }

    /// Return the single edge between two nodes (fails if not exactly one).
    fn edge_between(graph: &CodeGraph, src: NodeId, dst: NodeId) -> &codegraph::Edge {
        let ids = graph.get_edges_between(src, dst).unwrap();
        assert_eq!(ids.len(), 1, "expected exactly one edge {src}->{dst}");
        graph.get_edge(ids[0]).unwrap()
    }

    #[test]
    fn test_ir_to_graph_empty_uses_path_stem() {
        let ir = CodeIR::new(PathBuf::from("test.toml"));
        let (graph, info) = map(&ir, "test.toml");
        assert_eq!(info.classes.len(), 0);
        assert_eq!(info.functions.len(), 0);
        assert_eq!(info.traits.len(), 0);
        assert_eq!(info.imports.len(), 0);
        assert_eq!(graph.node_count(), 1);
        assert_eq!(graph.edge_count(), 0);

        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(file.node_type, NodeType::CodeFile);
        assert_eq!(file.properties.get_string("name"), Some("test"));
        assert_eq!(file.properties.get_string("path"), Some("test.toml"));
        assert_eq!(file.properties.get_string("language"), Some("toml"));
    }

    #[test]
    fn test_empty_path_stem_falls_back_to_unknown() {
        let ir = CodeIR::new(PathBuf::from(".."));
        let (graph, info) = map(&ir, "..");
        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(file.properties.get_string("name"), Some("unknown"));
    }

    #[test]
    fn test_module_drives_file_metadata() {
        let mut ir = CodeIR::new(PathBuf::from("Cargo.toml"));
        ir.set_module(ModuleEntity::new("Cargo", "/proj/Cargo.toml", "toml").with_line_count(42));
        let (graph, info) = map(&ir, "Cargo.toml");

        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(file.properties.get_string("name"), Some("Cargo"));
        assert_eq!(file.properties.get_string("path"), Some("/proj/Cargo.toml"));
        assert_eq!(file.properties.get_string("language"), Some("toml"));
        assert!(matches!(
            file.properties.get("line_count"),
            Some(PropertyValue::Int(42))
        ));
        assert_eq!(info.line_count, 42);
    }

    #[test]
    fn test_class_node_props_and_contains_edge() {
        let mut ir = CodeIR::new(PathBuf::from("Cargo.toml"));
        ir.add_class(ClassEntity::new("package", 1, 5).with_visibility("public"));
        let (graph, info) = map(&ir, "Cargo.toml");

        assert_eq!(info.classes.len(), 1);
        let class = graph.get_node(info.classes[0]).unwrap();
        assert_eq!(class.node_type, NodeType::Class);
        assert_eq!(class.properties.get_string("name"), Some("package"));
        assert_eq!(class.properties.get_string("path"), Some("Cargo.toml"));
        assert_eq!(class.properties.get_string("visibility"), Some("public"));
        assert_eq!(class.properties.get_string("language"), Some("toml"));
        assert!(matches!(
            class.properties.get("line_start"),
            Some(PropertyValue::Int(1))
        ));
        assert!(matches!(
            class.properties.get("line_end"),
            Some(PropertyValue::Int(5))
        ));

        let edge = edge_between(&graph, info.file_id, info.classes[0]);
        assert_eq!(edge.edge_type, EdgeType::Contains);
    }

    #[test]
    fn test_keypair_function_props_and_flags() {
        let mut ir = CodeIR::new(PathBuf::from("config.toml"));
        let mut f = FunctionEntity::new("name", 2, 2);
        f.signature = r#"name = "codegraph""#.to_string();
        f.visibility = "public".to_string();
        ir.add_function(f);
        let (graph, info) = map(&ir, "config.toml");

        assert_eq!(info.functions.len(), 1);
        let func = graph.get_node(info.functions[0]).unwrap();
        assert_eq!(func.node_type, NodeType::Function);
        assert_eq!(func.properties.get_string("name"), Some("name"));
        assert_eq!(
            func.properties.get_string("signature"),
            Some(r#"name = "codegraph""#)
        );
        assert_eq!(func.properties.get_string("visibility"), Some("public"));
        assert_eq!(func.properties.get_string("language"), Some("toml"));
        assert_eq!(func.properties.get_bool("is_async"), Some(false));
        assert_eq!(func.properties.get_bool("is_static"), Some(false));
        assert_eq!(func.properties.get_bool("is_abstract"), Some(false));
        assert_eq!(func.properties.get_bool("is_test"), Some(false));
        assert!(matches!(
            func.properties.get("line_start"),
            Some(PropertyValue::Int(2))
        ));
        // No parent_class -> file Contains edge and no parent_class prop.
        assert!(func.properties.get("parent_class").is_none());
        let edge = edge_between(&graph, info.file_id, info.functions[0]);
        assert_eq!(edge.edge_type, EdgeType::Contains);
    }

    #[test]
    fn test_function_contained_by_known_section() {
        let mut ir = CodeIR::new(PathBuf::from("config.toml"));
        ir.add_class(ClassEntity::new("package", 1, 3));
        let mut f = FunctionEntity::new("package.name", 2, 2);
        f.parent_class = Some("package".to_string());
        ir.add_function(f);
        let (graph, info) = map(&ir, "config.toml");

        let section_id = info.classes[0];
        let func_id = info.functions[0];
        let func = graph.get_node(func_id).unwrap();
        assert_eq!(func.properties.get_string("parent_class"), Some("package"));

        // Contained by its section, not the file.
        let edge = edge_between(&graph, section_id, func_id);
        assert_eq!(edge.edge_type, EdgeType::Contains);
        assert!(graph
            .get_edges_between(info.file_id, func_id)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn test_function_unknown_parent_falls_back_to_file() {
        let mut ir = CodeIR::new(PathBuf::from("config.toml"));
        let mut f = FunctionEntity::new("dangling.key", 2, 2);
        f.parent_class = Some("no_such_section".to_string());
        ir.add_function(f);
        let (graph, info) = map(&ir, "config.toml");

        let func_id = info.functions[0];
        // parent_class prop is still recorded even though the section is absent.
        let func = graph.get_node(func_id).unwrap();
        assert_eq!(
            func.properties.get_string("parent_class"),
            Some("no_such_section")
        );
        // Fallback: contained directly by the file.
        let edge = edge_between(&graph, info.file_id, func_id);
        assert_eq!(edge.edge_type, EdgeType::Contains);
    }

    #[test]
    fn test_multiple_functions_each_contained() {
        let mut ir = CodeIR::new(PathBuf::from("config.toml"));
        ir.add_function(FunctionEntity::new("a", 1, 1));
        ir.add_function(FunctionEntity::new("b", 2, 2));
        ir.add_function(FunctionEntity::new("c", 3, 3));
        let (graph, info) = map(&ir, "config.toml");

        assert_eq!(info.functions.len(), 3);
        // file node + 3 function nodes.
        assert_eq!(graph.node_count(), 4);
        // one Contains edge per function.
        assert_eq!(graph.edge_count(), 3);
        for &func_id in &info.functions {
            let edge = edge_between(&graph, info.file_id, func_id);
            assert_eq!(edge.edge_type, EdgeType::Contains);
        }
    }

    #[test]
    fn test_section_with_keypair_full_shape() {
        let mut ir = CodeIR::new(PathBuf::from("config.toml"));
        ir.add_class(ClassEntity::new("package", 1, 3));
        let mut f = FunctionEntity::new("package.name", 2, 2);
        f.parent_class = Some("package".to_string());
        ir.add_function(f);
        let (graph, info) = map(&ir, "config.toml");

        assert_eq!(info.classes.len(), 1);
        assert_eq!(info.functions.len(), 1);
        // file + section + keypair.
        assert_eq!(graph.node_count(), 3);
        // file->section Contains + section->keypair Contains.
        assert_eq!(graph.edge_count(), 2);
    }

    #[test]
    fn test_traits_and_imports_always_empty() {
        let mut ir = CodeIR::new(PathBuf::from("config.toml"));
        ir.add_class(ClassEntity::new("package", 1, 3));
        ir.add_function(FunctionEntity::new("edition", 2, 2));
        let (_graph, info) = map(&ir, "config.toml");

        assert!(info.traits.is_empty());
        assert!(info.imports.is_empty());
    }

    #[test]
    fn test_no_module_line_count_defaults_zero() {
        let ir = CodeIR::new(PathBuf::from("config.toml"));
        let (_graph, info) = map(&ir, "config.toml");
        assert_eq!(info.line_count, 0);
    }
}
