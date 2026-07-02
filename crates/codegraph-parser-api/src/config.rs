// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Configuration for parser behavior
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParserConfig {
    /// Skip private/internal entities (language-specific)
    pub skip_private: bool,

    /// Skip test files and test functions
    pub skip_tests: bool,

    /// Maximum file size to parse (in bytes)
    /// Files larger than this will be skipped
    pub max_file_size: usize,

    /// Timeout per file (None = no timeout)
    #[serde(with = "duration_option")]
    pub timeout_per_file: Option<Duration>,

    /// Enable parallel parsing (for `parse_files`)
    pub parallel: bool,

    /// Number of parallel workers (None = use num_cpus)
    pub parallel_workers: Option<usize>,

    /// Include documentation/comments in entities
    pub include_docs: bool,

    /// Extract type information (when available)
    pub extract_types: bool,
}

// Helper module for serializing Duration
mod duration_option {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Option<Duration>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match duration {
            Some(d) => d.as_secs().serialize(serializer),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Duration>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs: Option<u64> = Option::deserialize(deserializer)?;
        Ok(secs.map(Duration::from_secs))
    }
}

impl Default for ParserConfig {
    fn default() -> Self {
        Self {
            skip_private: false,
            skip_tests: false,
            max_file_size: 10 * 1024 * 1024, // 10 MB
            timeout_per_file: Some(Duration::from_secs(30)),
            parallel: false,
            parallel_workers: None,
            include_docs: true,
            extract_types: true,
        }
    }
}

impl ParserConfig {
    /// Create config for fast parsing (skips tests, docs, types)
    pub fn fast() -> Self {
        Self {
            skip_tests: true,
            include_docs: false,
            extract_types: false,
            ..Default::default()
        }
    }

    /// Create config for comprehensive parsing
    pub fn comprehensive() -> Self {
        Self {
            skip_private: false,
            skip_tests: false,
            include_docs: true,
            extract_types: true,
            ..Default::default()
        }
    }

    /// Enable parallel parsing
    pub fn with_parallel(mut self, parallel: bool) -> Self {
        self.parallel = parallel;
        self
    }

    /// Set maximum file size
    pub fn with_max_file_size(mut self, size: usize) -> Self {
        self.max_file_size = size;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_timeout_is_30s() {
        let c = ParserConfig::default();
        assert_eq!(c.timeout_per_file, Some(Duration::from_secs(30)));
    }

    #[test]
    fn fast_disables_docs_types_and_skips_tests() {
        let c = ParserConfig::fast();
        assert!(c.skip_tests);
        assert!(!c.include_docs);
        assert!(!c.extract_types);
        // Other fields keep their defaults.
        assert_eq!(c.max_file_size, 10 * 1024 * 1024);
        assert!(!c.parallel);
    }

    #[test]
    fn comprehensive_keeps_docs_and_types() {
        let c = ParserConfig::comprehensive();
        assert!(!c.skip_private);
        assert!(!c.skip_tests);
        assert!(c.include_docs);
        assert!(c.extract_types);
    }

    #[test]
    fn builder_setters_apply() {
        let c = ParserConfig::default()
            .with_parallel(true)
            .with_max_file_size(42);
        assert!(c.parallel);
        assert_eq!(c.max_file_size, 42);
    }

    #[test]
    fn serde_round_trip_with_some_timeout() {
        let c = ParserConfig::default();
        let json = serde_json::to_string(&c).unwrap();
        let back: ParserConfig = serde_json::from_str(&json).unwrap();
        // duration_option serializes as whole seconds.
        assert_eq!(back.timeout_per_file, Some(Duration::from_secs(30)));
        assert_eq!(c, back);
    }

    #[test]
    fn serde_round_trip_with_none_timeout() {
        let c = ParserConfig {
            timeout_per_file: None,
            ..Default::default()
        };
        let json = serde_json::to_string(&c).unwrap();
        let back: ParserConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.timeout_per_file, None);
    }

    #[test]
    fn default_serializes_to_exact_wire_format() {
        // Pins the full snake_case wire contract: every field name plus the
        // custom duration_option serializer emitting timeout_per_file as a bare
        // whole-second number (not a {secs,nanos} Duration struct), None as
        // explicit null (parallel_workers), and max_file_size as 10 MiB.
        let json = serde_json::to_string(&ParserConfig::default()).unwrap();
        assert_eq!(
            json,
            r#"{"skip_private":false,"skip_tests":false,"max_file_size":10485760,"timeout_per_file":30,"parallel":false,"parallel_workers":null,"include_docs":true,"extract_types":true}"#
        );
    }

    #[test]
    fn sub_second_timeout_truncates_to_whole_seconds() {
        // duration_option::serialize calls Duration::as_secs(), which floors to
        // whole seconds - so any sub-second component is silently dropped and
        // the value does NOT round-trip. Pin this lossy behavior so a future
        // switch to as_secs_f64/as_millis is a deliberate, test-visible change.
        let c = ParserConfig {
            timeout_per_file: Some(Duration::from_millis(1500)),
            ..Default::default()
        };
        let json = serde_json::to_string(&c).unwrap();
        assert!(
            json.contains(r#""timeout_per_file":1"#),
            "1500ms should serialize as bare 1, got: {json}"
        );
        let back: ParserConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.timeout_per_file, Some(Duration::from_secs(1)));
        assert_ne!(back.timeout_per_file, c.timeout_per_file);
    }
}
