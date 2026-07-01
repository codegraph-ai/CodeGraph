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
    let class_ids = Vec::new();
    let trait_ids = Vec::new();
    let import_ids = Vec::new();

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
            .with("language", "yaml");

        let id = graph
            .add_node(NodeType::CodeFile, props)
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
        node_map.insert(name, id);
        id
    };

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
            .with("is_abstract", func.is_abstract);

        if let Some(ref doc) = func.doc_comment {
            props = props.with("doc", doc.clone());
        }
        if let Some(ref body) = func.body_prefix {
            props = props.with("body_prefix", body.clone());
        }

        let func_id = graph
            .add_node(NodeType::Function, props)
            .map_err(|e| ParserError::GraphError(e.to_string()))?;

        node_map.insert(func.name.clone(), func_id);
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
    use codegraph::{Direction, PropertyValue};
    use codegraph_parser_api::{
        ClassEntity, FunctionEntity, ImportRelation, ModuleEntity, TraitEntity,
    };

    fn build(ir: &CodeIR) -> (CodeGraph, FileInfo) {
        let mut graph = CodeGraph::in_memory().unwrap();
        let info = ir_to_graph(ir, &mut graph, Path::new("config.yaml")).unwrap();
        (graph, info)
    }

    fn name_of(graph: &CodeGraph, id: NodeId) -> String {
        match graph.get_node(id).unwrap().properties.get("name") {
            Some(PropertyValue::String(s)) => s.clone(),
            _ => String::new(),
        }
    }

    #[test]
    fn empty_ir_creates_file_node_from_path_stem() {
        let ir = CodeIR::new(std::path::PathBuf::from("config.yaml"));
        let (graph, info) = build(&ir);

        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(
            file.properties.get("name"),
            Some(&PropertyValue::String("config".to_string()))
        );
        assert_eq!(
            file.properties.get("language"),
            Some(&PropertyValue::String("yaml".to_string()))
        );
        assert!(info.functions.is_empty());
        assert!(info.classes.is_empty());
        assert!(info.traits.is_empty());
        assert!(info.imports.is_empty());
        assert_eq!(info.line_count, 0);
        assert_eq!(graph.node_count(), 1);
    }

    #[test]
    fn module_drives_file_node_metadata_and_line_count() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("config.yaml"));
        let mut module = ModuleEntity::new("config", "deploy/config.yaml", "yaml");
        module.line_count = 42;
        module.doc_comment = Some("deployment config".to_string());
        ir.set_module(module);

        let (graph, info) = build(&ir);
        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(
            file.properties.get("name"),
            Some(&PropertyValue::String("config".to_string()))
        );
        assert_eq!(
            file.properties.get("path"),
            Some(&PropertyValue::String("deploy/config.yaml".to_string()))
        );
        assert_eq!(
            file.properties.get("line_count"),
            Some(&PropertyValue::Int(42))
        );
        assert_eq!(
            file.properties.get("doc"),
            Some(&PropertyValue::String("deployment config".to_string()))
        );
        assert_eq!(info.line_count, 42);
    }

    #[test]
    fn classes_are_ignored_by_the_mapper() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("config.yaml"));
        let mut class = ClassEntity::new("Service", 1, 5).with_visibility("public");
        class
            .methods
            .push(FunctionEntity::new("start", 2, 4).with_visibility("public"));
        ir.add_class(class);

        let (graph, info) = build(&ir);
        // The yaml mapper never iterates ir.classes, so nothing is emitted.
        assert!(info.classes.is_empty());
        assert!(info.functions.is_empty());
        assert_eq!(graph.node_count(), 1);
    }

    #[test]
    fn traits_are_ignored_by_the_mapper() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("config.yaml"));
        ir.add_trait(TraitEntity::new("Deployable", 1, 3));

        let (graph, info) = build(&ir);
        // The yaml mapper never iterates ir.traits, so no Interface node exists.
        assert!(info.traits.is_empty());
        assert_eq!(graph.node_count(), 1);
        assert!(graph
            .nodes_iter()
            .all(|(_, node)| node.node_type != NodeType::Interface));
    }

    #[test]
    fn free_function_is_contained_by_file_with_bare_name_and_flags() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("config.yaml"));
        let func = FunctionEntity::new("build", 1, 8).with_signature("build:");
        ir.add_function(func);

        let (graph, info) = build(&ir);
        assert_eq!(info.functions.len(), 1);

        let func_id = info.functions[0];
        // Yaml keeps keys/anchors bare, no qualification.
        assert_eq!(name_of(&graph, func_id), "build");
        let neighbors = graph
            .get_neighbors(info.file_id, Direction::Outgoing)
            .unwrap();
        assert!(neighbors.contains(&func_id));

        let node = graph.get_node(func_id).unwrap();
        assert_eq!(
            node.properties.get("is_async"),
            Some(&PropertyValue::Bool(false))
        );
        assert_eq!(
            node.properties.get("is_static"),
            Some(&PropertyValue::Bool(false))
        );
    }

    #[test]
    fn function_records_signature_and_line_bounds_but_no_complexity() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("config.yaml"));
        let func = FunctionEntity::new("deploy", 3, 9)
            .with_signature("deploy:")
            .with_visibility("public");
        ir.add_function(func);

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.functions[0]).unwrap();
        assert_eq!(
            node.properties.get("signature"),
            Some(&PropertyValue::String("deploy:".to_string()))
        );
        assert_eq!(
            node.properties.get("visibility"),
            Some(&PropertyValue::String("public".to_string()))
        );
        assert_eq!(
            node.properties.get("line_start"),
            Some(&PropertyValue::Int(3))
        );
        assert_eq!(
            node.properties.get("line_end"),
            Some(&PropertyValue::Int(9))
        );
        assert_eq!(
            node.properties.get("is_abstract"),
            Some(&PropertyValue::Bool(false))
        );
        // The yaml mapper never reads func.complexity, so no complexity props exist.
        assert_eq!(node.properties.get("complexity"), None);
        assert_eq!(node.properties.get("complexity_grade"), None);
    }

    #[test]
    fn imports_are_ignored_by_the_mapper() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("config.yaml"));
        ir.add_import(
            ImportRelation::new("config", "base").with_symbols(vec!["shared".to_string()]),
        );

        let (graph, info) = build(&ir);
        // The yaml mapper never iterates ir.imports, so no Module node is emitted.
        assert!(info.imports.is_empty());
        assert_eq!(graph.node_count(), 1);
        assert!(graph
            .nodes_iter()
            .all(|(_, node)| node.node_type != NodeType::Module));
    }

    #[test]
    fn function_doc_and_body_prefix_props_are_emitted_when_present() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("config.yaml"));
        let func = FunctionEntity::new("release", 1, 6)
            .with_signature("release:")
            .with_doc("release pipeline stage")
            .with_body_prefix("release:\n  steps:");
        ir.add_function(func);

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.functions[0]).unwrap();
        // The `if let Some(ref doc)` and `if let Some(ref body)` arms only fire
        // when the entity carries them; every other function test leaves both None.
        assert_eq!(
            node.properties.get("doc"),
            Some(&PropertyValue::String("release pipeline stage".to_string()))
        );
        assert_eq!(
            node.properties.get("body_prefix"),
            Some(&PropertyValue::String("release:\n  steps:".to_string()))
        );
    }

    #[test]
    fn function_without_doc_or_body_prefix_omits_those_props() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("config.yaml"));
        ir.add_function(FunctionEntity::new("plain", 1, 2));

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.functions[0]).unwrap();
        // Neither optional arm fires, so the props stay absent.
        assert_eq!(node.properties.get("doc"), None);
        assert_eq!(node.properties.get("body_prefix"), None);
    }

    #[test]
    fn missing_file_stem_falls_back_to_unknown_name() {
        let ir = CodeIR::new(std::path::PathBuf::from(".."));
        let mut graph = CodeGraph::in_memory().unwrap();
        // `..` has no file_stem, so the `unwrap_or("unknown")` arm is taken; the
        // build() helper always uses config.yaml and never reaches this fallback.
        let info = ir_to_graph(&ir, &mut graph, Path::new("..")).unwrap();
        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(
            file.properties.get("name"),
            Some(&PropertyValue::String("unknown".to_string()))
        );
        assert_eq!(
            file.properties.get("language"),
            Some(&PropertyValue::String("yaml".to_string()))
        );
    }

    #[test]
    fn multiple_functions_are_each_contained_by_file() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("config.yaml"));
        ir.add_function(FunctionEntity::new("stage_build", 1, 4));
        ir.add_function(FunctionEntity::new("stage_test", 5, 9));

        let (graph, info) = build(&ir);
        assert_eq!(info.functions.len(), 2);
        // File node plus two function nodes.
        assert_eq!(graph.node_count(), 3);

        let neighbors = graph
            .get_neighbors(info.file_id, Direction::Outgoing)
            .unwrap();
        assert!(neighbors.contains(&info.functions[0]));
        assert!(neighbors.contains(&info.functions[1]));
    }
}
