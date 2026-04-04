// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for Node creation and property handling.

use codegraph::{Node, NodeType, PropertyMap, PropertyValue};

#[test]
fn test_node_creation_with_properties() {
    let props = PropertyMap::new()
        .with("name", "test_function")
        .with("line_start", 10i64);

    let node = Node::new(1, NodeType::Function, props);

    assert_eq!(node.id, 1);
    assert_eq!(node.node_type, NodeType::Function);
    assert_eq!(node.properties.get_string("name"), Some("test_function"));
    assert_eq!(node.properties.get_int("line_start"), Some(10));
}

#[test]
fn test_node_set_property() {
    let mut node = Node::new(1, NodeType::Function, PropertyMap::new());

    node.set_property("visibility", "public");
    node.set_property("is_async", true);

    assert_eq!(
        node.get_property("visibility"),
        Some(&PropertyValue::String("public".to_string()))
    );
    assert_eq!(
        node.get_property("is_async"),
        Some(&PropertyValue::Bool(true))
    );
}

#[test]
fn test_node_get_property_missing() {
    let node = Node::new(1, NodeType::Function, PropertyMap::new());

    assert_eq!(node.get_property("missing"), None);
}
