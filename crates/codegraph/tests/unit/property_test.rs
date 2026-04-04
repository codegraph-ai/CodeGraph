// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Unit tests for `PropertyMap` builder pattern and type-safe getters.

use codegraph::PropertyMap;

#[test]
fn test_property_map_builder() {
    let props = PropertyMap::new()
        .with("name", "test")
        .with("count", 42)
        .with("enabled", true)
        .with("score", 3.15);

    assert_eq!(props.get_string("name"), Some("test"));
    assert_eq!(props.get_int("count"), Some(42));
    assert_eq!(props.get_bool("enabled"), Some(true));
    assert_eq!(props.get_float("score"), Some(3.15));
}

#[test]
fn test_property_map_type_safe_getters() {
    let props = PropertyMap::new()
        .with("text", "value")
        .with("number", 123i64);

    // Type-safe getters return None for wrong type
    assert_eq!(props.get_int("text"), None);
    assert_eq!(props.get_string("number"), None);
}

#[test]
fn test_property_map_lists() {
    let props = PropertyMap::new()
        .with("symbols", vec!["foo".to_string(), "bar".to_string()])
        .with("lines", vec![1i64, 2i64, 3i64]);

    assert_eq!(
        props.get_string_list("symbols").map(<[String]>::len),
        Some(2)
    );
    assert_eq!(props.get_int_list("lines"), Some(&[1i64, 2i64, 3i64][..]));
}

#[test]
fn test_property_map_insert_and_remove() {
    let mut props = PropertyMap::new();

    props.insert("key1", "value1");
    assert!(props.contains_key("key1"));
    assert_eq!(props.len(), 1);

    props.remove("key1");
    assert!(!props.contains_key("key1"));
    assert_eq!(props.len(), 0);
}
