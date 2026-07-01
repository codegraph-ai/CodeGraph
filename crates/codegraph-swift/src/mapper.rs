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
    let mut trait_ids = Vec::new();
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
            .with("language", "swift");

        let id = graph
            .add_node(NodeType::CodeFile, props)
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
        node_map.insert(name, id);
        id
    };

    // Add functions
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
        if let Some(ref return_type) = func.return_type {
            props = props.with("return_type", return_type.clone());
        }
        if let Some(ref parent) = func.parent_class {
            props = props.with("parent_class", parent.clone());
        }
        if !func.parameters.is_empty() {
            let param_names: Vec<String> = func.parameters.iter().map(|p| p.name.clone()).collect();
            props = props.with("parameters", param_names);
        }
        if !func.attributes.is_empty() {
            props = props.with("attributes", func.attributes.clone());
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

        // Link function to file or parent class
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

    // Add classes
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
        if !class.type_parameters.is_empty() {
            props = props.with("type_parameters", class.type_parameters.clone());
        }
        if let Some(ref body) = class.body_prefix {
            props = props.with("body_prefix", body.clone());
        }

        let class_id = graph
            .add_node(NodeType::Class, props)
            .map_err(|e| ParserError::GraphError(e.to_string()))?;

        node_map.insert(class.name.clone(), class_id);
        class_ids.push(class_id);

        // Link class to file
        graph
            .add_edge(file_id, class_id, EdgeType::Contains, PropertyMap::new())
            .map_err(|e| ParserError::GraphError(e.to_string()))?;

        // Add methods
        for method in &class.methods {
            let method_name = format!("{}::{}", class.name, method.name);
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

            // Link method to class
            graph
                .add_edge(class_id, method_id, EdgeType::Contains, PropertyMap::new())
                .map_err(|e| ParserError::GraphError(e.to_string()))?;
        }
    }

    // Add traits (protocols in Swift)
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
        if !trait_entity.required_methods.is_empty() {
            let method_names: Vec<String> = trait_entity
                .required_methods
                .iter()
                .map(|m| m.name.clone())
                .collect();
            props = props.with("required_methods", method_names);
        }

        let trait_id = graph
            .add_node(NodeType::Interface, props)
            .map_err(|e| ParserError::GraphError(e.to_string()))?;

        node_map.insert(trait_entity.name.clone(), trait_id);
        trait_ids.push(trait_id);

        // Link trait to file
        graph
            .add_edge(file_id, trait_id, EdgeType::Contains, PropertyMap::new())
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
    }

    // Add import nodes and relationships
    for import in &ir.imports {
        let imported_module = &import.imported;

        // Create or get import node
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

        // Create import edge from file to imported module
        let mut edge_props = PropertyMap::new();
        if let Some(ref alias) = import.alias {
            edge_props = edge_props.with("alias", alias.clone());
        }
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

    // Add call relationships
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

    // Store unresolved calls on caller nodes for post-processing
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

    // Add inheritance relationships
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

    // Add implementation relationships
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

    // Count source lines
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
        CallRelation, ClassEntity, FunctionEntity, ImplementationRelation, ImportRelation,
        InheritanceRelation, ModuleEntity, TraitEntity,
    };
    use std::path::{Path, PathBuf};

    fn map(ir: &CodeIR) -> (CodeGraph, FileInfo) {
        let mut graph = CodeGraph::in_memory().unwrap();
        let info = ir_to_graph(ir, &mut graph, Path::new("test.swift")).unwrap();
        (graph, info)
    }

    /// Return the single edge between two nodes (fails if not exactly one).
    fn edge_between(graph: &CodeGraph, src: NodeId, dst: NodeId) -> &codegraph::Edge {
        let ids = graph.get_edges_between(src, dst).unwrap();
        assert_eq!(ids.len(), 1, "expected exactly one edge {src}->{dst}");
        graph.get_edge(ids[0]).unwrap()
    }

    #[test]
    fn test_property_types() {
        let mut ir = CodeIR::new(PathBuf::from("test.swift"));
        ir.set_module(ModuleEntity::new("test", "test.swift", "swift").with_line_count(100));
        let func = FunctionEntity::new("test_fn", 10, 20)
            .with_signature("func test_fn()")
            .with_visibility("public")
            .async_fn();
        ir.add_function(func);

        let (graph, file_info) = map(&ir);

        // Verify file node line_count is Int
        let file_node = graph.get_node(file_info.file_id).unwrap();
        assert!(
            matches!(
                file_node.properties.get("line_count"),
                Some(PropertyValue::Int(100))
            ),
            "line_count should be Int, got {:?}",
            file_node.properties.get("line_count")
        );

        // Verify function properties are correct types
        let func_node = graph.get_node(file_info.functions[0]).unwrap();
        assert!(matches!(
            func_node.properties.get("line_start"),
            Some(PropertyValue::Int(10))
        ));
        assert!(matches!(
            func_node.properties.get("line_end"),
            Some(PropertyValue::Int(20))
        ));
        assert!(matches!(
            func_node.properties.get("is_async"),
            Some(PropertyValue::Bool(true))
        ));
    }

    #[test]
    fn empty_ir_builds_file_node_from_path_stem() {
        // No module set: name is derived from the file stem, language is
        // hard-coded to "swift", and the graph holds only the file node.
        let ir = CodeIR::new(PathBuf::from("test.swift"));
        let (graph, info) = map(&ir);

        assert_eq!(graph.node_count(), 1);
        assert_eq!(graph.edge_count(), 0);
        assert_eq!(info.line_count, 0);
        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(file.properties.get_string("name"), Some("test"));
        assert_eq!(file.properties.get_string("language"), Some("swift"));
        assert!(matches!(file.node_type, NodeType::CodeFile));
    }

    #[test]
    fn free_function_gets_file_contains_edge() {
        let mut ir = CodeIR::new(PathBuf::from("test.swift"));
        ir.add_function(FunctionEntity::new("free", 1, 2));
        let (graph, info) = map(&ir);

        let func_id = info.functions[0];
        let edge = edge_between(&graph, info.file_id, func_id);
        assert!(matches!(edge.edge_type, EdgeType::Contains));
    }

    #[test]
    fn class_emits_node_methods_and_contains_edges() {
        // A class with a method yields a Class node (file->class Contains)
        // plus a "Class::method" Function node (class->method Contains).
        let mut ir = CodeIR::new(PathBuf::from("test.swift"));
        let class = ClassEntity::new("Widget", 1, 30)
            .with_visibility("public")
            .with_methods(vec![FunctionEntity::new("render", 5, 9)]);
        ir.add_class(class);
        let (graph, info) = map(&ir);

        // file + class + method
        assert_eq!(graph.node_count(), 3);
        assert_eq!(info.classes.len(), 1);
        assert_eq!(info.functions.len(), 1);

        let class_id = info.classes[0];
        let class_node = graph.get_node(class_id).unwrap();
        assert!(matches!(class_node.node_type, NodeType::Class));
        assert_eq!(class_node.properties.get_string("name"), Some("Widget"));

        // file -> class
        assert!(matches!(
            edge_between(&graph, info.file_id, class_id).edge_type,
            EdgeType::Contains
        ));

        // method is qualified and contained by the class, not the file
        let method_id = info.functions[0];
        let method = graph.get_node(method_id).unwrap();
        assert_eq!(method.properties.get_string("name"), Some("Widget::render"));
        assert_eq!(method.properties.get_string("parent_class"), Some("Widget"));
        assert!(matches!(
            edge_between(&graph, class_id, method_id).edge_type,
            EdgeType::Contains
        ));
        // no direct file -> method containment
        assert!(graph
            .get_edges_between(info.file_id, method_id)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn trait_becomes_interface_with_required_methods() {
        let mut ir = CodeIR::new(PathBuf::from("test.swift"));
        let t = TraitEntity::new("Drawable", 1, 5)
            .with_methods(vec![FunctionEntity::new("draw", 2, 3)]);
        ir.add_trait(t);
        let (graph, info) = map(&ir);

        assert_eq!(info.traits.len(), 1);
        let trait_node = graph.get_node(info.traits[0]).unwrap();
        assert!(matches!(trait_node.node_type, NodeType::Interface));
        assert_eq!(
            trait_node
                .properties
                .get_string_list_compat("required_methods"),
            Some(vec!["draw".to_string()])
        );
        // file -> interface Contains
        assert!(matches!(
            edge_between(&graph, info.file_id, info.traits[0]).edge_type,
            EdgeType::Contains
        ));
    }

    #[test]
    fn import_creates_external_module_with_edge_props() {
        let mut ir = CodeIR::new(PathBuf::from("test.swift"));
        ir.add_import(
            ImportRelation::new("test", "Foundation")
                .with_alias("F")
                .with_symbols(vec!["URL".to_string(), "Data".to_string()]),
        );
        let (graph, info) = map(&ir);

        assert_eq!(info.imports.len(), 1);
        let module = graph.get_node(info.imports[0]).unwrap();
        assert!(matches!(module.node_type, NodeType::Module));
        assert_eq!(module.properties.get_string("name"), Some("Foundation"));
        assert_eq!(module.properties.get_string("is_external"), Some("true"));

        let edge = edge_between(&graph, info.file_id, info.imports[0]);
        assert!(matches!(edge.edge_type, EdgeType::Imports));
        assert_eq!(edge.properties.get_string("alias"), Some("F"));
        assert_eq!(
            edge.properties.get_string_list_compat("symbols"),
            Some(vec!["URL".to_string(), "Data".to_string()])
        );
    }

    #[test]
    fn duplicate_imports_reuse_one_module_node() {
        let mut ir = CodeIR::new(PathBuf::from("test.swift"));
        ir.add_import(ImportRelation::new("test", "UIKit"));
        ir.add_import(ImportRelation::new("test", "UIKit"));
        let (graph, info) = map(&ir);

        // Both import_ids point at the same reused Module node.
        assert_eq!(info.imports.len(), 2);
        assert_eq!(info.imports[0], info.imports[1]);
        // file + single module node only.
        assert_eq!(graph.node_count(), 2);
    }

    #[test]
    fn resolved_call_creates_calls_edge_with_props() {
        let mut ir = CodeIR::new(PathBuf::from("test.swift"));
        ir.add_function(FunctionEntity::new("caller", 1, 5));
        ir.add_function(FunctionEntity::new("callee", 6, 8));
        ir.add_call(CallRelation::new("caller", "callee", 3));
        let (graph, info) = map(&ir);

        let caller_id = info.functions[0];
        let callee_id = info.functions[1];
        let edge = edge_between(&graph, caller_id, callee_id);
        assert!(matches!(edge.edge_type, EdgeType::Calls));
        assert!(matches!(
            edge.properties.get("call_site_line"),
            Some(PropertyValue::Int(3))
        ));
    }

    #[test]
    fn unresolved_call_is_stored_on_caller_node() {
        // Callee absent from the node map: the call is recorded as an
        // `unresolved_calls` string list on the caller rather than an edge.
        let mut ir = CodeIR::new(PathBuf::from("test.swift"));
        ir.add_function(FunctionEntity::new("caller", 1, 5));
        ir.add_call(CallRelation::new("caller", "external_fn", 2));
        let (graph, info) = map(&ir);

        let caller = graph.get_node(info.functions[0]).unwrap();
        assert_eq!(
            caller.properties.get_string_list_compat("unresolved_calls"),
            Some(vec!["external_fn".to_string()])
        );
        // no Calls edge was created.
        assert_eq!(
            graph
                .iter_edges()
                .filter(|(_, e)| matches!(e.edge_type, EdgeType::Calls))
                .count(),
            0
        );
    }

    #[test]
    fn inheritance_creates_extends_edge_with_order() {
        let mut ir = CodeIR::new(PathBuf::from("test.swift"));
        ir.add_class(ClassEntity::new("Dog", 1, 4));
        ir.add_class(ClassEntity::new("Animal", 5, 8));
        ir.add_inheritance(InheritanceRelation::new("Dog", "Animal").with_order(2));
        let (graph, info) = map(&ir);

        let dog = info.classes[0];
        let animal = info.classes[1];
        let edge = edge_between(&graph, dog, animal);
        assert!(matches!(edge.edge_type, EdgeType::Extends));
        assert!(matches!(
            edge.properties.get("order"),
            Some(PropertyValue::Int(2))
        ));
    }

    #[test]
    fn implementation_creates_implements_edge() {
        let mut ir = CodeIR::new(PathBuf::from("test.swift"));
        ir.add_class(ClassEntity::new("Dog", 1, 4));
        ir.add_trait(TraitEntity::new("Barkable", 5, 6));
        ir.add_implementation(ImplementationRelation::new("Dog", "Barkable"));
        let (graph, info) = map(&ir);

        let dog = info.classes[0];
        let barkable = info.traits[0];
        let edge = edge_between(&graph, dog, barkable);
        assert!(matches!(edge.edge_type, EdgeType::Implements));
    }
}
