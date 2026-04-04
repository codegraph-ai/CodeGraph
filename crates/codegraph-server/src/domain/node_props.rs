// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Typed property accessors for graph nodes.
//!
//! These functions eliminate scattered get_int/get_string fallback chains
//! by providing a single canonical accessor for each property.
//!
use codegraph::{Node, PropertyMap};

// Line accessors (from Node)

/// Get the start line of a node. Tries line_start then start_line. Returns 0 if absent.
pub(crate) fn line_start(node: &Node) -> u32 {
    line_start_from_props(&node.properties)
}

/// Get the end line of a node. Tries line_end then end_line. Returns 0 if absent.
pub(crate) fn line_end(node: &Node) -> u32 {
    line_end_from_props(&node.properties)
}

/// Optional variant — returns None when neither key is present.
#[allow(dead_code)]
pub(crate) fn line_start_opt(node: &Node) -> Option<u32> {
    line_start_opt_from_props(&node.properties)
}

/// Optional variant — returns None when neither key is present.
#[allow(dead_code)]
pub(crate) fn line_end_opt(node: &Node) -> Option<u32> {
    line_end_opt_from_props(&node.properties)
}

// Line accessors (from PropertyMap — for callers without a Node)

pub(crate) fn line_start_from_props(props: &PropertyMap) -> u32 {
    props
        .get_int("line_start")
        .or_else(|| props.get_int("start_line"))
        .unwrap_or(0) as u32
}

pub(crate) fn line_end_from_props(props: &PropertyMap) -> u32 {
    props
        .get_int("line_end")
        .or_else(|| props.get_int("end_line"))
        .unwrap_or(0) as u32
}

pub(crate) fn line_start_opt_from_props(props: &PropertyMap) -> Option<u32> {
    props
        .get_int("line_start")
        .or_else(|| props.get_int("start_line"))
        .map(|v| v as u32)
}

pub(crate) fn line_end_opt_from_props(props: &PropertyMap) -> Option<u32> {
    props
        .get_int("line_end")
        .or_else(|| props.get_int("end_line"))
        .map(|v| v as u32)
}

pub(crate) fn col_start_from_props(props: &PropertyMap) -> u32 {
    props
        .get_int("col_start")
        .or_else(|| props.get_int("start_col"))
        .unwrap_or(0) as u32
}

pub(crate) fn col_end_from_props(props: &PropertyMap) -> u32 {
    props
        .get_int("col_end")
        .or_else(|| props.get_int("end_col"))
        .unwrap_or(10000) as u32
}

// String property accessors

/// Get the node name. Returns "" when absent.
pub(crate) fn name(node: &Node) -> &str {
    node.properties.get_string("name").unwrap_or("")
}

/// Get the node file path. Returns "" when absent.
pub(crate) fn path(node: &Node) -> &str {
    node.properties.get_string("path").unwrap_or("")
}

/// Get the node visibility string. Returns "public" when absent.
#[allow(dead_code)]
pub(crate) fn visibility(node: &Node) -> &str {
    node.properties.get_string("visibility").unwrap_or("public")
}

/// Get the node language. Returns "" when absent.
pub(crate) fn language(node: &Node) -> &str {
    node.properties.get_string("language").unwrap_or("")
}

// Boolean property accessors

/// Whether the node is public/exported.
/// Checks is_public, then exported, then falls back to visibility string.
pub(crate) fn is_public(node: &Node) -> bool {
    node.properties
        .get_bool("is_public")
        .or_else(|| node.properties.get_bool("exported"))
        .unwrap_or_else(|| matches!(visibility(node), "public" | "pub"))
}
