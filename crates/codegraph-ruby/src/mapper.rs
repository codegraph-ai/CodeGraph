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
            .with("language", "ruby");

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

        // Convention-based HTTP handler detection for Rails:
        // Public methods in classes ending with "Controller" are HTTP handlers.
        if func.visibility == "public" {
            if let Some(ref parent) = func.parent_class {
                if is_controller_class(parent) && !is_controller_callback(&func.name) {
                    props = props
                        .with("http_method", "ANY")
                        .with("route", format!("/{}", func.name))
                        .with("is_entry_point", true);
                }
            }
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

            // Link method to class
            graph
                .add_edge(class_id, method_id, EdgeType::Contains, PropertyMap::new())
                .map_err(|e| ParserError::GraphError(e.to_string()))?;
        }
    }

    // Add modules (traits in Ruby)
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

        // Link module to file
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
            let is_relative = import.alias.as_deref() == Some("require_relative");

            let props = PropertyMap::new()
                .with("name", imported_module.clone())
                .with("is_external", if is_relative { "false" } else { "true" });

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

    // Add implementation relationships (include/extend/prepend)
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

/// Check if a class name follows the controller convention.
fn is_controller_class(name: &str) -> bool {
    name.ends_with("Controller") || name.ends_with("_controller")
}

/// Skip Rails callback/hook methods that aren't HTTP actions.
fn is_controller_callback(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.starts_with("before_")
        || lower.starts_with("after_")
        || lower.starts_with("around_")
        || lower.starts_with("set_")
        || lower.starts_with("validate_")
        || lower.starts_with("check_")
        || lower.starts_with("require_")
        || lower.starts_with("authenticate_")
        || lower.starts_with("authorize_")
        || lower == "initialize"
        || lower == "new"
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegraph::PropertyValue;
    use codegraph_parser_api::{
        CallRelation, ClassEntity, ComplexityMetrics, FunctionEntity, ImportRelation, Parameter,
        TraitEntity,
    };
    use std::path::PathBuf;

    fn prop<'a>(
        graph: &'a CodeGraph,
        id: NodeId,
        key: &str,
    ) -> Option<&'a codegraph::PropertyValue> {
        graph.get_node(id).ok().and_then(|n| n.properties.get(key))
    }

    #[test]
    fn test_ir_to_graph_empty() {
        let ir = CodeIR::new(PathBuf::from("test.rb"));
        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, PathBuf::from("test.rb").as_path());

        assert!(result.is_ok());
        let file_info = result.unwrap();
        assert_eq!(file_info.functions.len(), 0);
        assert_eq!(file_info.classes.len(), 0);
        assert_eq!(file_info.traits.len(), 0);
    }

    #[test]
    fn test_ir_to_graph_with_function() {
        let mut ir = CodeIR::new(PathBuf::from("test.rb"));
        ir.add_function(FunctionEntity::new("test_func", 1, 5));

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, PathBuf::from("test.rb").as_path());

        assert!(result.is_ok());
        let file_info = result.unwrap();
        assert_eq!(file_info.functions.len(), 1);
    }

    #[test]
    fn test_ir_to_graph_with_class() {
        let mut ir = CodeIR::new(PathBuf::from("test.rb"));
        ir.add_class(ClassEntity::new("Person", 1, 10));

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, PathBuf::from("test.rb").as_path());

        assert!(result.is_ok());
        let file_info = result.unwrap();
        assert_eq!(file_info.classes.len(), 1);
    }

    #[test]
    fn test_ir_to_graph_with_module() {
        let mut ir = CodeIR::new(PathBuf::from("test.rb"));
        ir.add_trait(TraitEntity::new("Loggable", 1, 5));

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, PathBuf::from("test.rb").as_path());

        assert!(result.is_ok());
        let file_info = result.unwrap();
        assert_eq!(file_info.traits.len(), 1);
    }

    #[test]
    fn test_ir_to_graph_with_module_entity() {
        let mut ir = CodeIR::new(PathBuf::from("test.rb"));
        ir.set_module(codegraph_parser_api::ModuleEntity::new(
            "main", "test.rb", "ruby",
        ));

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, PathBuf::from("test.rb").as_path());

        assert!(result.is_ok());
        let file_info = result.unwrap();
        graph.get_node(file_info.file_id).unwrap();
    }

    #[test]
    fn test_ir_to_graph_with_imports() {
        let mut ir = CodeIR::new(PathBuf::from("test.rb"));
        ir.add_import(ImportRelation::new("main", "json"));
        ir.add_import(ImportRelation::new("main", "./helper"));

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, PathBuf::from("test.rb").as_path());

        assert!(result.is_ok());
        let file_info = result.unwrap();
        assert_eq!(file_info.imports.len(), 2);
    }

    #[test]
    fn test_ir_to_graph_with_methods() {
        let mut ir = CodeIR::new(PathBuf::from("test.rb"));

        let mut class = ClassEntity::new("Calculator", 1, 10);
        class.methods.push(FunctionEntity::new("add", 2, 4));
        class.methods.push(FunctionEntity::new("subtract", 5, 7));
        ir.add_class(class);

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, PathBuf::from("test.rb").as_path());

        assert!(result.is_ok());
        let file_info = result.unwrap();
        assert_eq!(file_info.classes.len(), 1);
        assert_eq!(file_info.functions.len(), 2);
    }

    #[test]
    fn test_ir_to_graph_function_properties() {
        let mut ir = CodeIR::new(PathBuf::from("test.rb"));
        let func = FunctionEntity::new("public_func", 1, 5)
            .with_visibility("public")
            .with_signature("def public_func");
        ir.add_function(func);

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, PathBuf::from("test.rb").as_path());

        assert!(result.is_ok());
        let file_info = result.unwrap();
        assert_eq!(file_info.functions.len(), 1);

        let func_node = graph.get_node(file_info.functions[0]).unwrap();
        assert_eq!(
            func_node.properties.get("name"),
            Some(&codegraph::PropertyValue::String("public_func".to_string()))
        );
        assert_eq!(
            func_node.properties.get("visibility"),
            Some(&codegraph::PropertyValue::String("public".to_string()))
        );
    }

    #[test]
    fn test_ir_to_graph_with_implementation() {
        use codegraph::EdgeType;
        use codegraph_parser_api::ImplementationRelation;

        let mut ir = CodeIR::new(PathBuf::from("test.rb"));
        ir.add_class(ClassEntity::new("Person", 1, 20));
        ir.add_trait(TraitEntity::new("Walkable", 22, 30));
        ir.add_implementation(ImplementationRelation::new("Person", "Walkable"));

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, PathBuf::from("test.rb").as_path());

        assert!(result.is_ok());
        let file_info = result.unwrap();
        assert_eq!(file_info.classes.len(), 1);
        assert_eq!(file_info.traits.len(), 1);

        let class_id = file_info.classes[0];
        let module_id = file_info.traits[0];

        let edges = graph.get_edges_between(class_id, module_id).unwrap();
        assert!(
            !edges.is_empty(),
            "Should have implements edge between class and module"
        );

        let edge = graph.get_edge(edges[0]).unwrap();
        assert_eq!(
            edge.edge_type,
            EdgeType::Implements,
            "Edge should be of type Implements"
        );
    }

    #[test]
    fn test_ir_to_graph_with_inheritance() {
        use codegraph::EdgeType;
        use codegraph_parser_api::InheritanceRelation;

        let mut ir = CodeIR::new(PathBuf::from("test.rb"));
        ir.add_class(ClassEntity::new("Animal", 1, 10));
        ir.add_class(ClassEntity::new("Dog", 12, 25));
        ir.add_inheritance(InheritanceRelation::new("Dog", "Animal"));

        let mut graph = CodeGraph::in_memory().unwrap();
        let result = ir_to_graph(&ir, &mut graph, PathBuf::from("test.rb").as_path());

        assert!(result.is_ok());
        let file_info = result.unwrap();
        assert_eq!(file_info.classes.len(), 2);

        // Find Dog and Animal node IDs
        let dog_id = file_info.classes[1];
        let animal_id = file_info.classes[0];

        let edges = graph.get_edges_between(dog_id, animal_id).unwrap();
        assert!(
            !edges.is_empty(),
            "Should have extends edge between Dog and Animal"
        );

        let edge = graph.get_edge(edges[0]).unwrap();
        assert_eq!(
            edge.edge_type,
            EdgeType::Extends,
            "Edge should be of type Extends"
        );
    }
    #[test]
    fn test_property_types() {
        use codegraph::PropertyValue;
        use codegraph_parser_api::{FunctionEntity, ModuleEntity};
        use std::path::PathBuf;
        let mut ir = CodeIR::new(PathBuf::from("test.rb"));
        ir.set_module(ModuleEntity::new("test", "test.rb", "ruby").with_line_count(100));
        let func = FunctionEntity::new("test_fn", 10, 20)
            .with_signature("func test_fn()")
            .with_visibility("public")
            .async_fn();
        ir.add_function(func);

        let mut graph = CodeGraph::in_memory().unwrap();
        let file_info = ir_to_graph(&ir, &mut graph, std::path::Path::new("test.rb")).unwrap();

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
        assert!(
            matches!(
                func_node.properties.get("line_start"),
                Some(PropertyValue::Int(10))
            ),
            "line_start should be Int(10), got {:?}",
            func_node.properties.get("line_start")
        );
        assert!(
            matches!(
                func_node.properties.get("line_end"),
                Some(PropertyValue::Int(20))
            ),
            "line_end should be Int(20), got {:?}",
            func_node.properties.get("line_end")
        );
        assert!(
            matches!(
                func_node.properties.get("is_async"),
                Some(PropertyValue::Bool(true))
            ),
            "is_async should be Bool(true), got {:?}",
            func_node.properties.get("is_async")
        );
    }

    // --- is_controller_class / is_controller_callback helpers ---

    #[test]
    fn test_is_controller_class() {
        assert!(is_controller_class("UsersController"));
        assert!(is_controller_class("api_controller"));
        assert!(!is_controller_class("UserService"));
        assert!(!is_controller_class("Controllers"));
    }

    #[test]
    fn test_is_controller_callback() {
        for name in [
            "before_action",
            "after_save",
            "around_filter",
            "set_user",
            "validate_input",
            "check_perms",
            "require_login",
            "authenticate_user",
            "authorize_admin",
            "initialize",
            "new",
        ] {
            assert!(is_controller_callback(name), "{name} should be a callback");
        }
        assert!(!is_controller_callback("index"));
        assert!(!is_controller_callback("show"));
    }

    // --- Rails HTTP handler prop stamping ---

    #[test]
    fn test_controller_action_gets_http_props() {
        let mut ir = CodeIR::new(PathBuf::from("test.rb"));
        let mut graph = CodeGraph::in_memory().unwrap();
        // HTTP action prop stamping fires in the top-level function loop for a
        // public function whose parent_class is a controller (class.methods do
        // not carry the stamping path).
        let func = FunctionEntity::new("index", 2, 4)
            .with_visibility("public")
            .with_parent_class("UsersController");
        ir.add_function(func);

        let file_info = ir_to_graph(&ir, &mut graph, Path::new("test.rb")).unwrap();
        let action = file_info.functions[0];
        assert_eq!(
            prop(&graph, action, "http_method"),
            Some(&PropertyValue::String("ANY".to_string()))
        );
        assert_eq!(
            prop(&graph, action, "route"),
            Some(&PropertyValue::String("/index".to_string()))
        );
        assert_eq!(
            prop(&graph, action, "is_entry_point"),
            Some(&PropertyValue::Bool(true))
        );
    }

    #[test]
    fn test_controller_callback_no_http_props() {
        let mut ir = CodeIR::new(PathBuf::from("test.rb"));
        let func = FunctionEntity::new("before_action", 2, 4)
            .with_visibility("public")
            .with_parent_class("UsersController");
        ir.add_function(func);

        let mut graph = CodeGraph::in_memory().unwrap();
        let file_info = ir_to_graph(&ir, &mut graph, Path::new("test.rb")).unwrap();
        assert_eq!(prop(&graph, file_info.functions[0], "http_method"), None);
    }

    #[test]
    fn test_private_controller_method_no_http_props() {
        let mut ir = CodeIR::new(PathBuf::from("test.rb"));
        let func = FunctionEntity::new("index", 2, 4)
            .with_visibility("private")
            .with_parent_class("UsersController");
        ir.add_function(func);

        let mut graph = CodeGraph::in_memory().unwrap();
        let file_info = ir_to_graph(&ir, &mut graph, Path::new("test.rb")).unwrap();
        assert_eq!(prop(&graph, file_info.functions[0], "http_method"), None);
    }

    // --- Function optional props ---

    #[test]
    fn test_function_optional_props_present() {
        let mut ir = CodeIR::new(PathBuf::from("test.rb"));
        let func = FunctionEntity::new("parse", 1, 5)
            .with_doc("parses input")
            .with_return_type("Hash")
            .with_body_prefix("def parse")
            .with_parameters(vec![Parameter::new("raw"), Parameter::new("opts")]);
        ir.add_function(func);

        let mut graph = CodeGraph::in_memory().unwrap();
        let file_info = ir_to_graph(&ir, &mut graph, Path::new("test.rb")).unwrap();
        let id = file_info.functions[0];
        assert_eq!(
            prop(&graph, id, "doc"),
            Some(&PropertyValue::String("parses input".to_string()))
        );
        assert_eq!(
            prop(&graph, id, "return_type"),
            Some(&PropertyValue::String("Hash".to_string()))
        );
        assert_eq!(
            prop(&graph, id, "body_prefix"),
            Some(&PropertyValue::String("def parse".to_string()))
        );
        assert_eq!(
            prop(&graph, id, "parameters"),
            Some(&PropertyValue::StringList(vec![
                "raw".to_string(),
                "opts".to_string()
            ]))
        );
    }

    #[test]
    fn test_function_optional_props_absent() {
        let mut ir = CodeIR::new(PathBuf::from("test.rb"));
        ir.add_function(FunctionEntity::new("bare", 1, 3));

        let mut graph = CodeGraph::in_memory().unwrap();
        let file_info = ir_to_graph(&ir, &mut graph, Path::new("test.rb")).unwrap();
        let id = file_info.functions[0];
        assert_eq!(prop(&graph, id, "doc"), None);
        assert_eq!(prop(&graph, id, "return_type"), None);
        assert_eq!(prop(&graph, id, "body_prefix"), None);
        assert_eq!(prop(&graph, id, "parameters"), None);
    }

    // --- Complexity sub-props + grade ---

    #[test]
    fn test_function_complexity_all_subprops() {
        let mut ir = CodeIR::new(PathBuf::from("test.rb"));
        let metrics = ComplexityMetrics::new()
            .with_branches(3)
            .with_loops(2)
            .with_logical_operators(4)
            .with_nesting_depth(5)
            .with_exception_handlers(1)
            .with_early_returns(2);
        let mut func = FunctionEntity::new("complex", 1, 40);
        func = func.with_complexity(metrics.clone());
        ir.add_function(func);

        let mut graph = CodeGraph::in_memory().unwrap();
        let file_info = ir_to_graph(&ir, &mut graph, Path::new("test.rb")).unwrap();
        let id = file_info.functions[0];
        assert_eq!(
            prop(&graph, id, "complexity"),
            Some(&PropertyValue::Int(metrics.cyclomatic_complexity as i64))
        );
        assert_eq!(
            prop(&graph, id, "complexity_grade"),
            Some(&PropertyValue::String(metrics.grade().to_string()))
        );
        assert_eq!(
            prop(&graph, id, "complexity_branches"),
            Some(&PropertyValue::Int(3))
        );
        assert_eq!(
            prop(&graph, id, "complexity_loops"),
            Some(&PropertyValue::Int(2))
        );
        assert_eq!(
            prop(&graph, id, "complexity_logical_ops"),
            Some(&PropertyValue::Int(4))
        );
        assert_eq!(
            prop(&graph, id, "complexity_nesting"),
            Some(&PropertyValue::Int(5))
        );
        assert_eq!(
            prop(&graph, id, "complexity_exceptions"),
            Some(&PropertyValue::Int(1))
        );
        assert_eq!(
            prop(&graph, id, "complexity_early_returns"),
            Some(&PropertyValue::Int(2))
        );
    }

    // --- Import edge props and is_external branches ---

    #[test]
    fn test_import_edge_props_and_external() {
        let mut ir = CodeIR::new(PathBuf::from("test.rb"));
        ir.add_import(
            ImportRelation::new("main", "active_support")
                .with_alias("as")
                .wildcard()
                .with_symbols(vec!["Concern".to_string(), "Callbacks".to_string()]),
        );

        let mut graph = CodeGraph::in_memory().unwrap();
        let file_info = ir_to_graph(&ir, &mut graph, Path::new("test.rb")).unwrap();
        let import_id = file_info.imports[0];
        assert_eq!(
            prop(&graph, import_id, "is_external"),
            Some(&PropertyValue::String("true".to_string()))
        );

        let edges = graph
            .get_edges_between(file_info.file_id, import_id)
            .unwrap();
        let edge = graph.get_edge(edges[0]).unwrap();
        assert_eq!(edge.edge_type, EdgeType::Imports);
        assert_eq!(
            edge.properties.get("alias"),
            Some(&PropertyValue::String("as".to_string()))
        );
        assert_eq!(
            edge.properties.get("is_wildcard"),
            Some(&PropertyValue::String("true".to_string()))
        );
        assert_eq!(
            edge.properties.get("symbols"),
            Some(&PropertyValue::StringList(vec![
                "Concern".to_string(),
                "Callbacks".to_string()
            ]))
        );
    }

    #[test]
    fn test_require_relative_import_not_external() {
        let mut ir = CodeIR::new(PathBuf::from("test.rb"));
        ir.add_import(ImportRelation::new("main", "./helper").with_alias("require_relative"));

        let mut graph = CodeGraph::in_memory().unwrap();
        let file_info = ir_to_graph(&ir, &mut graph, Path::new("test.rb")).unwrap();
        assert_eq!(
            prop(&graph, file_info.imports[0], "is_external"),
            Some(&PropertyValue::String("false".to_string()))
        );
    }

    #[test]
    fn test_bare_import_edge_has_no_props() {
        let mut ir = CodeIR::new(PathBuf::from("test.rb"));
        ir.add_import(ImportRelation::new("main", "json"));

        let mut graph = CodeGraph::in_memory().unwrap();
        let file_info = ir_to_graph(&ir, &mut graph, Path::new("test.rb")).unwrap();
        let edges = graph
            .get_edges_between(file_info.file_id, file_info.imports[0])
            .unwrap();
        let edge = graph.get_edge(edges[0]).unwrap();
        assert_eq!(edge.properties.get("alias"), None);
        assert_eq!(edge.properties.get("is_wildcard"), None);
        assert_eq!(edge.properties.get("symbols"), None);
    }

    #[test]
    fn test_import_reuses_in_file_node() {
        let mut ir = CodeIR::new(PathBuf::from("test.rb"));
        // A class named the same as an import: the import loop reuses the
        // existing in-file class node rather than creating an external Module.
        ir.add_class(ClassEntity::new("Widget", 1, 10));
        ir.add_import(ImportRelation::new("main", "Widget"));

        let mut graph = CodeGraph::in_memory().unwrap();
        let file_info = ir_to_graph(&ir, &mut graph, Path::new("test.rb")).unwrap();
        let import_id = file_info.imports[0];
        // Reused class node: it is the same node as the class, not an external Module.
        assert_eq!(import_id, file_info.classes[0]);
        assert_eq!(prop(&graph, import_id, "is_external"), None);
        let node = graph.get_node(import_id).unwrap();
        assert_eq!(node.node_type, NodeType::Class);
    }

    // --- Class method qualified naming ---

    #[test]
    fn test_method_qualified_name_and_props() {
        let mut ir = CodeIR::new(PathBuf::from("test.rb"));
        let mut class = ClassEntity::new("Calc", 1, 10);
        class
            .methods
            .push(FunctionEntity::new("add", 2, 4).with_visibility("public"));
        ir.add_class(class);

        let mut graph = CodeGraph::in_memory().unwrap();
        let file_info = ir_to_graph(&ir, &mut graph, Path::new("test.rb")).unwrap();
        let method_id = file_info.functions[0];
        assert_eq!(
            prop(&graph, method_id, "name"),
            Some(&PropertyValue::String("Calc#add".to_string()))
        );
        assert_eq!(
            prop(&graph, method_id, "is_method"),
            Some(&PropertyValue::String("true".to_string()))
        );
        assert_eq!(
            prop(&graph, method_id, "parent_class"),
            Some(&PropertyValue::String("Calc".to_string()))
        );
        // Method is contained by the class, not the file.
        let edges = graph
            .get_edges_between(file_info.classes[0], method_id)
            .unwrap();
        assert_eq!(
            graph.get_edge(edges[0]).unwrap().edge_type,
            EdgeType::Contains
        );
    }

    // --- Trait required methods + doc ---

    #[test]
    fn test_trait_required_methods_and_doc() {
        let mut ir = CodeIR::new(PathBuf::from("test.rb"));
        let trait_entity = TraitEntity::new("Walkable", 1, 8)
            .with_doc("walk mixin")
            .with_methods(vec![
                FunctionEntity::new("walk", 2, 3),
                FunctionEntity::new("run", 4, 5),
            ]);
        ir.add_trait(trait_entity);

        let mut graph = CodeGraph::in_memory().unwrap();
        let file_info = ir_to_graph(&ir, &mut graph, Path::new("test.rb")).unwrap();
        let trait_id = file_info.traits[0];
        assert_eq!(
            prop(&graph, trait_id, "doc"),
            Some(&PropertyValue::String("walk mixin".to_string()))
        );
        assert_eq!(
            prop(&graph, trait_id, "required_methods"),
            Some(&PropertyValue::StringList(vec![
                "walk".to_string(),
                "run".to_string()
            ]))
        );
        assert_eq!(
            graph.get_node(trait_id).unwrap().node_type,
            NodeType::Interface
        );
    }

    // --- Call edges: direct / indirect / unresolved ---

    #[test]
    fn test_direct_call_edge_is_direct() {
        let mut ir = CodeIR::new(PathBuf::from("test.rb"));
        ir.add_function(FunctionEntity::new("caller_fn", 1, 5));
        ir.add_function(FunctionEntity::new("callee_fn", 6, 10));
        ir.add_call(CallRelation::new("caller_fn", "callee_fn", 3));

        let mut graph = CodeGraph::in_memory().unwrap();
        let file_info = ir_to_graph(&ir, &mut graph, Path::new("test.rb")).unwrap();
        let caller = file_info.functions[0];
        let callee = file_info.functions[1];
        let edges = graph.get_edges_between(caller, callee).unwrap();
        let edge = graph.get_edge(edges[0]).unwrap();
        assert_eq!(edge.edge_type, EdgeType::Calls);
        assert_eq!(
            edge.properties.get("is_direct"),
            Some(&PropertyValue::Bool(true))
        );
        assert_eq!(
            edge.properties.get("call_site_line"),
            Some(&PropertyValue::Int(3))
        );
    }

    #[test]
    fn test_indirect_call_edge_flag() {
        let mut ir = CodeIR::new(PathBuf::from("test.rb"));
        ir.add_function(FunctionEntity::new("a", 1, 5));
        ir.add_function(FunctionEntity::new("b", 6, 10));
        ir.add_call(CallRelation::new("a", "b", 3).indirect());

        let mut graph = CodeGraph::in_memory().unwrap();
        let file_info = ir_to_graph(&ir, &mut graph, Path::new("test.rb")).unwrap();
        let edges = graph
            .get_edges_between(file_info.functions[0], file_info.functions[1])
            .unwrap();
        assert_eq!(
            graph
                .get_edge(edges[0])
                .unwrap()
                .properties
                .get("is_direct"),
            Some(&PropertyValue::Bool(false))
        );
    }

    #[test]
    fn test_unresolved_calls_stored_and_deduped() {
        let mut ir = CodeIR::new(PathBuf::from("test.rb"));
        ir.add_function(FunctionEntity::new("caller_fn", 1, 5));
        // callee not in node_map -> accumulates into unresolved_calls, deduped.
        ir.add_call(CallRelation::new("caller_fn", "missing", 2));
        ir.add_call(CallRelation::new("caller_fn", "missing", 3));
        ir.add_call(CallRelation::new("caller_fn", "other", 4));

        let mut graph = CodeGraph::in_memory().unwrap();
        let file_info = ir_to_graph(&ir, &mut graph, Path::new("test.rb")).unwrap();
        assert_eq!(
            prop(&graph, file_info.functions[0], "unresolved_calls"),
            Some(&PropertyValue::StringList(vec![
                "missing".to_string(),
                "other".to_string()
            ]))
        );
    }
}
