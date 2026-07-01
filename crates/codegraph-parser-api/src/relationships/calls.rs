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

    #[test]
    fn accepts_string_and_str_inputs() {
        let c = CallRelation::new(String::from("a"), "b", 1);
        assert_eq!(c.caller, "a");
        assert_eq!(c.callee, "b");
    }

    #[test]
    fn indirect_then_vtable_stays_indirect() {
        let c = CallRelation::new("a", "b", 1)
            .indirect()
            .with_vtable("S".to_string(), "f".to_string());
        assert!(!c.is_direct);
        assert_eq!(c.struct_type, Some("S".to_string()));
    }

    #[test]
    fn equality_considers_all_fields() {
        let base = CallRelation::new("a", "b", 5);
        assert_eq!(base, CallRelation::new("a", "b", 5));
        assert_ne!(base, CallRelation::new("a", "b", 6));
        assert_ne!(base, CallRelation::new("a", "b", 5).indirect());
        assert_ne!(
            base,
            CallRelation::new("a", "b", 5).with_vtable("S".into(), "f".into())
        );
    }

    #[test]
    fn hashes_by_value_in_set() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(CallRelation::new("a", "b", 5));
        assert!(!set.insert(CallRelation::new("a", "b", 5)));
        assert!(set.insert(CallRelation::new("a", "b", 5).indirect()));
    }

    #[test]
    fn round_trips_through_json() {
        let c = CallRelation::new("register", "ndo_open", 42)
            .with_vtable("net_device_ops".to_string(), "ndo_open".to_string());
        let json = serde_json::to_string(&c).unwrap();
        let back: CallRelation = serde_json::from_str(&json).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn default_serializes_snake_case_wire_names_with_null_options() {
        // Pins the exact JSON wire format for a plain (no-vtable) call: every
        // multi-word field must stay snake_case and the two Option fields must
        // serialize as explicit null. The only prior serde test used with_vtable,
        // so the None arm and the exact field names were never asserted.
        let c = CallRelation::new("a", "b", 7);
        let json = serde_json::to_string(&c).unwrap();
        assert_eq!(
            json,
            r#"{"caller":"a","callee":"b","call_site_line":7,"is_direct":true,"struct_type":null,"field_name":null}"#
        );
        let back: CallRelation = serde_json::from_str(&json).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn vtable_serializes_populated_options_exactly() {
        // Pins the Some arm of struct_type/field_name and is_direct:false.
        let c = CallRelation::new("register", "ndo_open", 42)
            .with_vtable("net_device_ops".to_string(), "ndo_open".to_string());
        let json = serde_json::to_string(&c).unwrap();
        assert_eq!(
            json,
            r#"{"caller":"register","callee":"ndo_open","call_site_line":42,"is_direct":false,"struct_type":"net_device_ops","field_name":"ndo_open"}"#
        );
    }
}
