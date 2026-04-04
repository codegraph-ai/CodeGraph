// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for Edge creation and property handling.

use codegraph::{Edge, EdgeType, PropertyMap, PropertyValue};

#[test]
fn test_edge_creation_with_properties() {
    let props = PropertyMap::new().with("line", 42i64).with("column", 10i64);

    let edge = Edge::new(1, 100, 200, EdgeType::Calls, props);

    assert_eq!(edge.id, 1);
    assert_eq!(edge.source_id, 100);
    assert_eq!(edge.target_id, 200);
    assert_eq!(edge.edge_type, EdgeType::Calls);
    assert_eq!(edge.properties.get_int("line"), Some(42));
    assert_eq!(edge.properties.get_int("column"), Some(10));
}

#[test]
fn test_edge_set_property() {
    let mut edge = Edge::new(1, 100, 200, EdgeType::Calls, PropertyMap::new());

    edge.set_property("conditional", true);
    edge.set_property("error_handling", "try_catch");

    assert_eq!(
        edge.get_property("conditional"),
        Some(&PropertyValue::Bool(true))
    );
    assert_eq!(
        edge.get_property("error_handling"),
        Some(&PropertyValue::String("try_catch".to_string()))
    );
}
