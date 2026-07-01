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
    let mut import_ids = Vec::new();

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
            .with("language", "css");

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
        let info = ir_to_graph(ir, &mut graph, Path::new("theme.css")).unwrap();
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
        let ir = CodeIR::new(std::path::PathBuf::from("theme.css"));
        let (graph, info) = build(&ir);

        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(
            file.properties.get("name"),
            Some(&PropertyValue::String("theme".to_string()))
        );
        assert_eq!(
            file.properties.get("language"),
            Some(&PropertyValue::String("css".to_string()))
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
        let mut ir = CodeIR::new(std::path::PathBuf::from("theme.css"));
        let mut module = ModuleEntity::new("theme", "styles/theme.css", "css");
        module.line_count = 120;
        module.doc_comment = Some("theme styles".to_string());
        ir.set_module(module);

        let (graph, info) = build(&ir);
        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(
            file.properties.get("name"),
            Some(&PropertyValue::String("theme".to_string()))
        );
        assert_eq!(
            file.properties.get("path"),
            Some(&PropertyValue::String("styles/theme.css".to_string()))
        );
        assert_eq!(
            file.properties.get("line_count"),
            Some(&PropertyValue::Int(120))
        );
        assert_eq!(
            file.properties.get("doc"),
            Some(&PropertyValue::String("theme styles".to_string()))
        );
        assert_eq!(info.line_count, 120);
    }

    #[test]
    fn classes_are_ignored_by_the_mapper() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("theme.css"));
        let mut class = ClassEntity::new("Card", 1, 5).with_visibility("public");
        class
            .methods
            .push(FunctionEntity::new("hover", 2, 4).with_visibility("public"));
        ir.add_class(class);

        let (graph, info) = build(&ir);
        // The css mapper never iterates ir.classes, so nothing is emitted.
        assert!(info.classes.is_empty());
        assert!(info.functions.is_empty());
        assert_eq!(graph.node_count(), 1);
    }

    #[test]
    fn traits_are_ignored_by_the_mapper() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("theme.css"));
        ir.add_trait(TraitEntity::new("Themeable", 1, 3));

        let (graph, info) = build(&ir);
        // The css mapper never iterates ir.traits, so no Interface node exists.
        assert!(info.traits.is_empty());
        assert_eq!(graph.node_count(), 1);
        assert!(graph
            .nodes_iter()
            .all(|(_, node)| node.node_type != NodeType::Interface));
    }

    #[test]
    fn free_function_is_contained_by_file_with_bare_name_and_flags() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("theme.css"));
        let func = FunctionEntity::new("mix", 1, 8).with_signature("mix()");
        ir.add_function(func);

        let (graph, info) = build(&ir);
        assert_eq!(info.functions.len(), 1);

        let func_id = info.functions[0];
        // Css keeps function (selector/at-rule) names bare, no qualification.
        assert_eq!(name_of(&graph, func_id), "mix");
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
        let mut ir = CodeIR::new(std::path::PathBuf::from("theme.css"));
        let func = FunctionEntity::new("grid", 3, 9)
            .with_signature(".grid { ... }")
            .with_visibility("public");
        ir.add_function(func);

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.functions[0]).unwrap();
        assert_eq!(
            node.properties.get("signature"),
            Some(&PropertyValue::String(".grid { ... }".to_string()))
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
        // The css mapper never reads func.complexity, so no complexity props exist.
        assert_eq!(node.properties.get("complexity"), None);
        assert_eq!(node.properties.get("complexity_grade"), None);
    }

    #[test]
    fn import_creates_external_module_with_empty_edge_props() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("theme.css"));
        ir.add_import(ImportRelation::new("theme", "reset").with_symbols(vec!["base".to_string()]));

        let (graph, info) = build(&ir);
        assert_eq!(info.imports.len(), 1);

        let import_id = info.imports[0];
        let import_node = graph.get_node(import_id).unwrap();
        assert_eq!(import_node.node_type, NodeType::Module);
        assert_eq!(
            import_node.properties.get("name"),
            Some(&PropertyValue::String("reset".to_string()))
        );
        assert_eq!(
            import_node.properties.get("is_external"),
            Some(&PropertyValue::String("true".to_string()))
        );

        let edge_ids = graph.get_edges_between(info.file_id, import_id).unwrap();
        assert_eq!(edge_ids.len(), 1);
        let edge = graph.get_edge(edge_ids[0]).unwrap();
        assert_eq!(edge.edge_type, EdgeType::Imports);
        // The css mapper records NO props on the Imports edge (symbols dropped).
        assert_eq!(edge.properties.get("symbols"), None);
        assert_eq!(edge.properties.get("alias"), None);
        assert_eq!(edge.properties.get("is_wildcard"), None);
    }

    #[test]
    fn duplicate_import_target_reuses_existing_node() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("theme.css"));
        ir.add_import(ImportRelation::new("theme", "common"));
        ir.add_import(ImportRelation::new("theme", "common"));

        let (graph, info) = build(&ir);
        assert_eq!(info.imports.len(), 2);
        assert_eq!(info.imports[0], info.imports[1]);
        let edges = graph
            .get_edges_between(info.file_id, info.imports[0])
            .unwrap();
        assert_eq!(edges.len(), 2);
    }
}
