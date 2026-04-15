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
    let mut trait_ids = Vec::new();
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
            .with("language", "scala");

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
        if let Some(ref return_type) = func.return_type {
            props = props.with("return_type", return_type.clone());
        }
        if let Some(ref parent) = func.parent_class {
            props = props.with("parent_class", parent.clone());
        }
        if let Some(ref body) = func.body_prefix {
            props = props.with("body_prefix", body.clone());
        }
        if !func.parameters.is_empty() {
            let param_names: Vec<String> = func.parameters.iter().map(|p| p.name.clone()).collect();
            props = props.with("parameters", param_names);
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

        if let Some(ref parent_class) = func.parent_class {
            if let Some(&class_id) = node_map.get(parent_class) {
                graph
                    .add_edge(class_id, func_id, EdgeType::Contains, PropertyMap::new())
                    .map_err(|e| ParserError::GraphError(e.to_string()))?;
            }
        } else {
            graph
                .add_edge(file_id, func_id, EdgeType::Contains, PropertyMap::new())
                .map_err(|e| ParserError::GraphError(e.to_string()))?;
        }
    }

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
        if !class.attributes.is_empty() {
            props = props.with("attributes", class.attributes.clone());
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

        for method in &class.methods {
            let method_name = format!("{}#{}", class.name, method.name);
            let mut method_props = PropertyMap::new()
                .with("name", method_name.clone())
                .with("path", file_path.display().to_string())
                .with("signature", method.signature.clone())
                .with("visibility", method.visibility.clone())
                .with("line_start", method.line_start as i64)
                .with("line_end", method.line_end as i64)
                .with("is_method", "true")
                .with("parent_class", class.name.clone());

            if let Some(ref doc) = method.doc_comment {
                method_props = method_props.with("doc", doc.clone());
            }
            if let Some(ref body) = method.body_prefix {
                method_props = method_props.with("body_prefix", body.clone());
            }

            let method_id = graph
                .add_node(NodeType::Function, method_props)
                .map_err(|e| ParserError::GraphError(e.to_string()))?;

            node_map.insert(method_name, method_id);
            function_ids.push(method_id);

            graph
                .add_edge(class_id, method_id, EdgeType::Contains, PropertyMap::new())
                .map_err(|e| ParserError::GraphError(e.to_string()))?;
        }
    }

    for trait_entity in &ir.traits {
        let mut props = PropertyMap::new()
            .with("name", trait_entity.name.clone())
            .with("path", file_path.display().to_string())
            .with("visibility", trait_entity.visibility.clone())
            .with("line_start", trait_entity.line_start as i64)
            .with("line_end", trait_entity.line_end as i64);

        if let Some(ref doc) = trait_entity.doc_comment {
            props = props.with("doc", doc.clone());
        }

        let trait_id = graph
            .add_node(NodeType::Interface, props)
            .map_err(|e| ParserError::GraphError(e.to_string()))?;

        node_map.insert(trait_entity.name.clone(), trait_id);
        trait_ids.push(trait_id);

        graph
            .add_edge(file_id, trait_id, EdgeType::Contains, PropertyMap::new())
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

        let mut edge_props = PropertyMap::new();
        if !import.symbols.is_empty() {
            edge_props = edge_props.with("symbols", import.symbols.clone());
        }
        graph
            .add_edge(file_id, import_id, EdgeType::Imports, edge_props)
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
    }

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

    for inheritance in &ir.inheritance {
        if let (Some(&child_id), Some(&parent_id)) = (
            node_map.get(&inheritance.child),
            node_map.get(&inheritance.parent),
        ) {
            let edge_props = PropertyMap::new().with("order", inheritance.order as i64);

            graph
                .add_edge(child_id, parent_id, EdgeType::Extends, edge_props)
                .map_err(|e| ParserError::GraphError(e.to_string()))?;
        }
    }

    for impl_rel in &ir.implementations {
        if let (Some(&implementor_id), Some(&trait_id)) = (
            node_map.get(&impl_rel.implementor),
            node_map.get(&impl_rel.trait_name),
        ) {
            graph
                .add_edge(
                    implementor_id,
                    trait_id,
                    EdgeType::Implements,
                    PropertyMap::new(),
                )
                .map_err(|e| ParserError::GraphError(e.to_string()))?;
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
