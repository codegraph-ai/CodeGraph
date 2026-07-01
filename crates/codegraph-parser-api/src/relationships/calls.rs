// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

/// Represents a function call relationship
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CallRelation {
    /// Caller function name
    pub caller: String,

    /// Callee function name
    pub callee: String,

    /// Line number where the call occurs
    pub call_site_line: usize,

    /// Is this a direct call or indirect (e.g., through function pointer)?
    pub is_direct: bool,

    /// For ops struct / vtable assignments: the struct type name (e.g., "net_device_ops")
    pub struct_type: Option<String>,

    /// For ops struct / vtable assignments: the field name (e.g., "ndo_open")
    pub field_name: Option<String>,
}

impl CallRelation {
    pub fn new(caller: impl Into<String>, callee: impl Into<String>, line: usize) -> Self {
        Self {
            caller: caller.into(),
            callee: callee.into(),
            call_site_line: line,
            is_direct: true,
            struct_type: None,
            field_name: None,
        }
    }

    pub fn indirect(mut self) -> Self {
        self.is_direct = false;
        self
    }

    pub fn with_vtable(mut self, struct_type: String, field_name: String) -> Self {
        self.struct_type = Some(struct_type);
        self.field_name = Some(field_name);
        self.is_direct = false;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_defaults_to_direct_no_vtable() {
        let c = CallRelation::new("a", "b", 7);
        assert_eq!(c.caller, "a");
        assert_eq!(c.callee, "b");
        assert_eq!(c.call_site_line, 7);
        assert!(c.is_direct);
        assert_eq!(c.struct_type, None);
        assert_eq!(c.field_name, None);
    }

    #[test]
    fn indirect_clears_direct_flag() {
        assert!(!CallRelation::new("a", "b", 1).indirect().is_direct);
    }

    #[test]
    fn with_vtable_sets_fields_and_marks_indirect() {
        let c = CallRelation::new("register", "ndo_open", 42)
            .with_vtable("net_device_ops".to_string(), "ndo_open".to_string());
        assert_eq!(c.struct_type, Some("net_device_ops".to_string()));
        assert_eq!(c.field_name, Some("ndo_open".to_string()));
        assert!(!c.is_direct);
    }
}
