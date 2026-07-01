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

    // File node
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
            .with("language", "solidity");

        let id = graph
            .add_node(NodeType::CodeFile, props)
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
        node_map.insert(name, id);
        id
    };

    // Classes (contracts and libraries)
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

        // Methods
        for method in &class.methods {
            let method_name = format!("{}.{}", class.name, method.name);
            let mut method_props = PropertyMap::new()
                .with("name", method_name.clone())
                .with("path", file_path.display().to_string())
                .with("signature", method.signature.clone())
                .with("visibility", method.visibility.clone())
                .with("line_start", method.line_start as i64)
                .with("line_end", method.line_end as i64)
                .with("is_async", method.is_async)
                .with("is_static", method.is_static)
                .with("is_abstract", method.is_abstract)
                .with("is_method", "true")
                .with("parent_class", class.name.clone());

            if let Some(ref doc) = method.doc_comment {
                method_props = method_props.with("doc", doc.clone());
            }
            if let Some(ref return_type) = method.return_type {
                method_props = method_props.with("return_type", return_type.clone());
            }
            if !method.parameters.is_empty() {
                let param_names: Vec<String> =
                    method.parameters.iter().map(|p| p.name.clone()).collect();
                method_props = method_props.with("parameters", param_names);
            }
            if !method.attributes.is_empty() {
                method_props = method_props.with("attributes", method.attributes.clone());
            }
            if let Some(ref body) = method.body_prefix {
                method_props = method_props.with("body_prefix", body.clone());
            }
            if let Some(ref complexity) = method.complexity {
                method_props = method_props
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

    // Interfaces (traits)
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

        graph
            .add_edge(file_id, trait_id, EdgeType::Contains, PropertyMap::new())
            .map_err(|e| ParserError::GraphError(e.to_string()))?;

        // Interface methods
        for method in &trait_entity.required_methods {
            let method_name = format!("{}.{}", trait_entity.name, method.name);
            let mut method_props = PropertyMap::new()
                .with("name", method_name.clone())
                .with("path", file_path.display().to_string())
                .with("signature", method.signature.clone())
                .with("visibility", method.visibility.clone())
                .with("line_start", method.line_start as i64)
                .with("line_end", method.line_end as i64)
                .with("is_abstract", true)
                .with("is_method", "true")
                .with("parent_class", trait_entity.name.clone());

            if let Some(ref return_type) = method.return_type {
                method_props = method_props.with("return_type", return_type.clone());
            }

            let method_id = graph
                .add_node(NodeType::Function, method_props)
                .map_err(|e| ParserError::GraphError(e.to_string()))?;

            node_map.insert(method_name, method_id);
            function_ids.push(method_id);

            graph
                .add_edge(trait_id, method_id, EdgeType::Contains, PropertyMap::new())
                .map_err(|e| ParserError::GraphError(e.to_string()))?;
        }
    }

    // Top-level free functions
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
        if !func.parameters.is_empty() {
            let param_names: Vec<String> = func.parameters.iter().map(|p| p.name.clone()).collect();
            props = props.with("parameters", param_names);
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

        graph
            .add_edge(file_id, func_id, EdgeType::Contains, PropertyMap::new())
            .map_err(|e| ParserError::GraphError(e.to_string()))?;
    }

    // Imports
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

    // Call relationships
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

    let line_count = ir.module.as_ref().map(|m| m.line_count).unwrap_or(0);

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
        CallRelation, ClassEntity, ComplexityMetrics, FunctionEntity, ImportRelation, ModuleEntity,
        Parameter, TraitEntity,
    };

    fn build(ir: &CodeIR) -> (CodeGraph, FileInfo) {
        let mut graph = CodeGraph::in_memory().unwrap();
        let info = ir_to_graph(ir, &mut graph, Path::new("Token.sol")).unwrap();
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
        let ir = CodeIR::new(std::path::PathBuf::from("Token.sol"));
        let (graph, info) = build(&ir);

        // File node named after the path stem, language defaulted to solidity.
        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(
            file.properties.get("name"),
            Some(&PropertyValue::String("Token".to_string()))
        );
        assert_eq!(
            file.properties.get("language"),
            Some(&PropertyValue::String("solidity".to_string()))
        );
        assert!(info.functions.is_empty());
        assert!(info.classes.is_empty());
        assert!(info.traits.is_empty());
        assert!(info.imports.is_empty());
        assert_eq!(info.line_count, 0);
    }

    #[test]
    fn module_drives_file_node_metadata_and_line_count() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Token.sol"));
        let mut module = ModuleEntity::new("MyModule", "src/Token.sol", "solidity");
        module.line_count = 42;
        module.doc_comment = Some("file docs".to_string());
        ir.set_module(module);

        let (graph, info) = build(&ir);
        let file = graph.get_node(info.file_id).unwrap();
        assert_eq!(
            file.properties.get("name"),
            Some(&PropertyValue::String("MyModule".to_string()))
        );
        assert_eq!(
            file.properties.get("path"),
            Some(&PropertyValue::String("src/Token.sol".to_string()))
        );
        assert_eq!(
            file.properties.get("line_count"),
            Some(&PropertyValue::Int(42))
        );
        assert_eq!(
            file.properties.get("doc"),
            Some(&PropertyValue::String("file docs".to_string()))
        );
        // line_count in FileInfo comes from the module.
        assert_eq!(info.line_count, 42);
    }

    #[test]
    fn contract_with_method_links_via_contains_edges() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Token.sol"));
        let mut contract = ClassEntity::new("Token", 1, 20)
            .with_visibility("public")
            .abstract_class();
        contract
            .methods
            .push(FunctionEntity::new("mint", 5, 10).with_visibility("external"));
        ir.add_class(contract);

        let (graph, info) = build(&ir);
        assert_eq!(info.classes.len(), 1);
        // Method counts as a function.
        assert_eq!(info.functions.len(), 1);

        let class_id = info.classes[0];
        let class_node = graph.get_node(class_id).unwrap();
        assert_eq!(class_node.node_type, NodeType::Class);
        assert_eq!(
            class_node.properties.get("is_abstract"),
            Some(&PropertyValue::Bool(true))
        );

        // File -Contains-> class.
        assert!(!graph
            .get_edges_between(info.file_id, class_id)
            .unwrap()
            .is_empty());

        // class -Contains-> method, method name is qualified Class.method.
        let method_id = info.functions[0];
        assert_eq!(name_of(&graph, method_id), "Token.mint");
        let method_node = graph.get_node(method_id).unwrap();
        assert_eq!(method_node.node_type, NodeType::Function);
        assert_eq!(
            method_node.properties.get("parent_class"),
            Some(&PropertyValue::String("Token".to_string()))
        );
        assert_eq!(
            method_node.properties.get("is_method"),
            Some(&PropertyValue::String("true".to_string()))
        );
        assert!(!graph
            .get_edges_between(class_id, method_id)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn interface_maps_to_interface_node_with_abstract_methods() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Token.sol"));
        let mut iface = TraitEntity::new("IERC20", 1, 8);
        iface
            .required_methods
            .push(FunctionEntity::new("transfer", 2, 3).with_return_type("bool"));
        ir.add_trait(iface);

        let (graph, info) = build(&ir);
        assert_eq!(info.traits.len(), 1);
        assert_eq!(info.functions.len(), 1);

        let iface_node = graph.get_node(info.traits[0]).unwrap();
        assert_eq!(iface_node.node_type, NodeType::Interface);

        let method_id = info.functions[0];
        assert_eq!(name_of(&graph, method_id), "IERC20.transfer");
        let method_node = graph.get_node(method_id).unwrap();
        // Interface methods are always abstract.
        assert_eq!(
            method_node.properties.get("is_abstract"),
            Some(&PropertyValue::Bool(true))
        );
        assert_eq!(
            method_node.properties.get("return_type"),
            Some(&PropertyValue::String("bool".to_string()))
        );
    }

    #[test]
    fn free_function_is_contained_by_file_with_complexity_props() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Token.sol"));
        let metrics = ComplexityMetrics {
            cyclomatic_complexity: 12,
            branches: 6,
            loops: 2,
            ..Default::default()
        };
        let func = FunctionEntity::new("helper", 1, 30)
            .with_signature("function helper()")
            .with_complexity(metrics);
        ir.add_function(func);

        let (graph, info) = build(&ir);
        assert_eq!(info.functions.len(), 1);

        let func_id = info.functions[0];
        // File -Contains-> function.
        let neighbors = graph
            .get_neighbors(info.file_id, Direction::Outgoing)
            .unwrap();
        assert!(neighbors.contains(&func_id));

        let node = graph.get_node(func_id).unwrap();
        assert_eq!(
            node.properties.get("complexity"),
            Some(&PropertyValue::Int(12))
        );
        // Grade 12 falls in the C band.
        assert_eq!(
            node.properties.get("complexity_grade"),
            Some(&PropertyValue::String("C".to_string()))
        );
        assert_eq!(
            node.properties.get("complexity_branches"),
            Some(&PropertyValue::Int(6))
        );
    }

    #[test]
    fn import_creates_external_module_and_records_edge_props() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Token.sol"));
        ir.add_import(
            ImportRelation::new("Token", "@openzeppelin/contracts")
                .with_alias("oz")
                .wildcard()
                .with_symbols(vec!["ERC20".to_string(), "Ownable".to_string()]),
        );

        let (graph, info) = build(&ir);
        assert_eq!(info.imports.len(), 1);

        let import_id = info.imports[0];
        let import_node = graph.get_node(import_id).unwrap();
        assert_eq!(import_node.node_type, NodeType::Module);
        assert_eq!(
            import_node.properties.get("is_external"),
            Some(&PropertyValue::String("true".to_string()))
        );

        let edge_ids = graph.get_edges_between(info.file_id, import_id).unwrap();
        assert_eq!(edge_ids.len(), 1);
        let edge = graph.get_edge(edge_ids[0]).unwrap();
        assert_eq!(edge.edge_type, EdgeType::Imports);
        assert_eq!(
            edge.properties.get("alias"),
            Some(&PropertyValue::String("oz".to_string()))
        );
        assert_eq!(
            edge.properties.get("is_wildcard"),
            Some(&PropertyValue::String("true".to_string()))
        );
    }

    #[test]
    fn call_relation_wires_calls_edge_only_between_known_nodes() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Token.sol"));
        ir.add_function(FunctionEntity::new("caller", 1, 5));
        ir.add_function(FunctionEntity::new("callee", 6, 10));
        // Known caller + callee -> edge created.
        ir.add_call(CallRelation::new("caller", "callee", 3));
        // Unknown callee -> silently skipped.
        ir.add_call(CallRelation::new("caller", "ghost", 4));

        let (graph, info) = build(&ir);
        let caller_id = info
            .functions
            .iter()
            .copied()
            .find(|&id| name_of(&graph, id) == "caller")
            .unwrap();
        let callee_id = info
            .functions
            .iter()
            .copied()
            .find(|&id| name_of(&graph, id) == "callee")
            .unwrap();

        let call_edges: Vec<_> = graph
            .get_edges_between(caller_id, callee_id)
            .unwrap()
            .into_iter()
            .filter(|&e| graph.get_edge(e).unwrap().edge_type == EdgeType::Calls)
            .collect();
        assert_eq!(call_edges.len(), 1);
        let edge = graph.get_edge(call_edges[0]).unwrap();
        assert_eq!(
            edge.properties.get("call_site_line"),
            Some(&PropertyValue::Int(3))
        );

        // The ghost callee produced no extra Module/Function node.
        let outgoing = graph.get_neighbors(caller_id, Direction::Outgoing).unwrap();
        assert_eq!(outgoing, vec![callee_id]);
    }

    #[test]
    fn duplicate_import_target_reuses_existing_node() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Token.sol"));
        ir.add_import(ImportRelation::new("Token", "./Base.sol"));
        ir.add_import(ImportRelation::new("Token", "./Base.sol"));

        let (graph, info) = build(&ir);
        // Two import edges but the module node is deduplicated via node_map.
        assert_eq!(info.imports.len(), 2);
        assert_eq!(info.imports[0], info.imports[1]);
        let edges = graph
            .get_edges_between(info.file_id, info.imports[0])
            .unwrap();
        assert_eq!(edges.len(), 2);
    }

    #[test]
    fn class_records_optional_doc_attributes_and_body_prefix() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Token.sol"));
        let contract = ClassEntity::new("Token", 3, 40)
            .with_visibility("public")
            .with_doc("token contract")
            .with_attributes(vec!["payable".to_string(), "immutable".to_string()])
            .with_body_prefix("uint256 supply;");
        ir.add_class(contract);

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.classes[0]).unwrap();
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
            Some(&PropertyValue::Int(40))
        );
        assert_eq!(
            node.properties.get("doc"),
            Some(&PropertyValue::String("token contract".to_string()))
        );
        assert_eq!(
            node.properties.get("body_prefix"),
            Some(&PropertyValue::String("uint256 supply;".to_string()))
        );
        assert!(node.properties.get("attributes").is_some());
    }

    #[test]
    fn class_omits_optional_props_when_absent() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Token.sol"));
        ir.add_class(ClassEntity::new("Token", 1, 5));

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.classes[0]).unwrap();
        assert!(node.properties.get("doc").is_none());
        assert!(node.properties.get("attributes").is_none());
        assert!(node.properties.get("body_prefix").is_none());
        // Default class is not abstract.
        assert_eq!(
            node.properties.get("is_abstract"),
            Some(&PropertyValue::Bool(false))
        );
    }

    #[test]
    fn method_records_full_metadata_and_all_complexity_props() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Token.sol"));
        let metrics = ComplexityMetrics {
            cyclomatic_complexity: 7,
            branches: 3,
            loops: 1,
            logical_operators: 2,
            max_nesting_depth: 4,
            exception_handlers: 1,
            early_returns: 2,
        };
        let method = FunctionEntity::new("mint", 5, 12)
            .with_signature("function mint(address to)")
            .with_visibility("external")
            .with_return_type("bool")
            .with_parameters(vec![Parameter::new("to").with_type("address")])
            .with_attributes(vec!["onlyOwner".to_string()])
            .with_body_prefix("require(to != address(0));")
            .with_doc("mints tokens")
            .with_complexity(metrics);
        let mut contract = ClassEntity::new("Token", 1, 20);
        contract.methods.push(method);
        ir.add_class(contract);

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.functions[0]).unwrap();
        assert_eq!(
            node.properties.get("signature"),
            Some(&PropertyValue::String(
                "function mint(address to)".to_string()
            ))
        );
        assert_eq!(
            node.properties.get("visibility"),
            Some(&PropertyValue::String("external".to_string()))
        );
        assert_eq!(
            node.properties.get("return_type"),
            Some(&PropertyValue::String("bool".to_string()))
        );
        assert_eq!(
            node.properties.get("doc"),
            Some(&PropertyValue::String("mints tokens".to_string()))
        );
        assert_eq!(
            node.properties.get("body_prefix"),
            Some(&PropertyValue::String(
                "require(to != address(0));".to_string()
            ))
        );
        assert!(node.properties.get("parameters").is_some());
        assert!(node.properties.get("attributes").is_some());
        // All eight complexity sub-properties are propagated.
        assert_eq!(
            node.properties.get("complexity"),
            Some(&PropertyValue::Int(7))
        );
        assert_eq!(
            node.properties.get("complexity_branches"),
            Some(&PropertyValue::Int(3))
        );
        assert_eq!(
            node.properties.get("complexity_loops"),
            Some(&PropertyValue::Int(1))
        );
        assert_eq!(
            node.properties.get("complexity_logical_ops"),
            Some(&PropertyValue::Int(2))
        );
        assert_eq!(
            node.properties.get("complexity_nesting"),
            Some(&PropertyValue::Int(4))
        );
        assert_eq!(
            node.properties.get("complexity_exceptions"),
            Some(&PropertyValue::Int(1))
        );
        assert_eq!(
            node.properties.get("complexity_early_returns"),
            Some(&PropertyValue::Int(2))
        );
    }

    #[test]
    fn method_without_complexity_omits_complexity_props() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Token.sol"));
        let mut contract = ClassEntity::new("Token", 1, 20);
        contract.methods.push(FunctionEntity::new("mint", 5, 8));
        ir.add_class(contract);

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.functions[0]).unwrap();
        assert!(node.properties.get("complexity").is_none());
        assert!(node.properties.get("complexity_grade").is_none());
        assert!(node.properties.get("return_type").is_none());
        assert!(node.properties.get("parameters").is_none());
    }

    #[test]
    fn interface_records_line_bounds_doc_and_required_methods_prop() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Token.sol"));
        let mut iface = TraitEntity::new("IERC20", 2, 20)
            .with_visibility("public")
            .with_doc("erc20 interface");
        iface
            .required_methods
            .push(FunctionEntity::new("transfer", 3, 4));
        iface
            .required_methods
            .push(FunctionEntity::new("approve", 5, 6));
        ir.add_trait(iface);

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.traits[0]).unwrap();
        assert_eq!(node.node_type, NodeType::Interface);
        assert_eq!(
            node.properties.get("line_start"),
            Some(&PropertyValue::Int(2))
        );
        assert_eq!(
            node.properties.get("line_end"),
            Some(&PropertyValue::Int(20))
        );
        assert_eq!(
            node.properties.get("doc"),
            Some(&PropertyValue::String("erc20 interface".to_string()))
        );
        // required_methods stored as a list prop; both methods become function nodes.
        assert!(node.properties.get("required_methods").is_some());
        assert_eq!(info.functions.len(), 2);
    }

    #[test]
    fn interface_method_carries_is_method_and_parent_class() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Token.sol"));
        let mut iface = TraitEntity::new("IERC20", 1, 8);
        iface
            .required_methods
            .push(FunctionEntity::new("transfer", 2, 3).with_signature("function transfer()"));
        ir.add_trait(iface);

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.functions[0]).unwrap();
        assert_eq!(
            node.properties.get("is_method"),
            Some(&PropertyValue::String("true".to_string()))
        );
        assert_eq!(
            node.properties.get("parent_class"),
            Some(&PropertyValue::String("IERC20".to_string()))
        );
        assert_eq!(
            node.properties.get("signature"),
            Some(&PropertyValue::String("function transfer()".to_string()))
        );
        // No return type declared -> prop absent.
        assert!(node.properties.get("return_type").is_none());
    }

    #[test]
    fn free_function_records_flags_params_and_return_type() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Token.sol"));
        let func = FunctionEntity::new("compute", 1, 10)
            .with_return_type("uint256")
            .with_parameters(vec![Parameter::new("x"), Parameter::new("y")])
            .with_body_prefix("return x + y;")
            .with_doc("adds two numbers");
        ir.add_function(func);

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.functions[0]).unwrap();
        assert_eq!(
            node.properties.get("is_async"),
            Some(&PropertyValue::Bool(false))
        );
        assert_eq!(
            node.properties.get("is_static"),
            Some(&PropertyValue::Bool(false))
        );
        assert_eq!(
            node.properties.get("is_abstract"),
            Some(&PropertyValue::Bool(false))
        );
        assert_eq!(
            node.properties.get("return_type"),
            Some(&PropertyValue::String("uint256".to_string()))
        );
        assert_eq!(
            node.properties.get("doc"),
            Some(&PropertyValue::String("adds two numbers".to_string()))
        );
        assert!(node.properties.get("parameters").is_some());
        assert!(node.properties.get("body_prefix").is_some());
    }

    #[test]
    fn import_targeting_in_file_node_reuses_it_without_marking_external() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Token.sol"));
        // A contract named "Base" exists in this file.
        ir.add_class(ClassEntity::new("Base", 1, 5));
        // An import whose target name matches the in-file node reuses it via node_map.
        ir.add_import(ImportRelation::new("Token", "Base"));

        let (graph, info) = build(&ir);
        assert_eq!(info.imports.len(), 1);
        // The import id is the existing Class node, not a fresh external Module.
        assert_eq!(info.imports[0], info.classes[0]);
        let node = graph.get_node(info.imports[0]).unwrap();
        assert_eq!(node.node_type, NodeType::Class);
        // Reused node is not stamped is_external.
        assert!(node.properties.get("is_external").is_none());
    }

    #[test]
    fn bare_import_creates_edge_with_no_optional_props() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Token.sol"));
        ir.add_import(ImportRelation::new("Token", "./Base.sol"));

        let (graph, info) = build(&ir);
        let edge_ids = graph
            .get_edges_between(info.file_id, info.imports[0])
            .unwrap();
        let edge = graph.get_edge(edge_ids[0]).unwrap();
        assert_eq!(edge.edge_type, EdgeType::Imports);
        assert!(edge.properties.get("alias").is_none());
        assert!(edge.properties.get("is_wildcard").is_none());
        assert!(edge.properties.get("symbols").is_none());
    }

    #[test]
    fn import_records_symbols_without_alias_or_wildcard() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Token.sol"));
        ir.add_import(
            ImportRelation::new("Token", "./ERC20.sol").with_symbols(vec!["ERC20".to_string()]),
        );

        let (graph, info) = build(&ir);
        let edge_ids = graph
            .get_edges_between(info.file_id, info.imports[0])
            .unwrap();
        let edge = graph.get_edge(edge_ids[0]).unwrap();
        assert!(edge.properties.get("symbols").is_some());
        assert!(edge.properties.get("alias").is_none());
        assert!(edge.properties.get("is_wildcard").is_none());
    }

    #[test]
    fn call_edge_records_is_direct_flag() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Token.sol"));
        ir.add_function(FunctionEntity::new("caller", 1, 5));
        ir.add_function(FunctionEntity::new("callee", 6, 10));
        ir.add_call(CallRelation::new("caller", "callee", 3).indirect());

        let (graph, info) = build(&ir);
        let caller_id = info
            .functions
            .iter()
            .copied()
            .find(|&id| name_of(&graph, id) == "caller")
            .unwrap();
        let callee_id = info
            .functions
            .iter()
            .copied()
            .find(|&id| name_of(&graph, id) == "callee")
            .unwrap();
        let edge_ids = graph.get_edges_between(caller_id, callee_id).unwrap();
        let edge = graph.get_edge(edge_ids[0]).unwrap();
        assert_eq!(
            edge.properties.get("is_direct"),
            Some(&PropertyValue::Bool(false))
        );
    }

    #[test]
    fn multiple_classes_and_functions_all_mapped() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Token.sol"));
        ir.add_class(ClassEntity::new("Token", 1, 10));
        ir.add_class(ClassEntity::new("Vault", 11, 20));
        ir.add_function(FunctionEntity::new("helperA", 21, 25));
        ir.add_function(FunctionEntity::new("helperB", 26, 30));

        let (graph, info) = build(&ir);
        assert_eq!(info.classes.len(), 2);
        assert_eq!(info.functions.len(), 2);
        // Every function/class is contained by the file node.
        let neighbors = graph
            .get_neighbors(info.file_id, Direction::Outgoing)
            .unwrap();
        for id in info.classes.iter().chain(info.functions.iter()) {
            assert!(neighbors.contains(id));
        }
    }

    #[test]
    fn low_complexity_function_grades_a() {
        let mut ir = CodeIR::new(std::path::PathBuf::from("Token.sol"));
        let metrics = ComplexityMetrics {
            cyclomatic_complexity: 2,
            ..Default::default()
        };
        ir.add_function(FunctionEntity::new("simple", 1, 5).with_complexity(metrics));

        let (graph, info) = build(&ir);
        let node = graph.get_node(info.functions[0]).unwrap();
        assert_eq!(
            node.properties.get("complexity_grade"),
            Some(&PropertyValue::String("A".to_string()))
        );
    }
}
