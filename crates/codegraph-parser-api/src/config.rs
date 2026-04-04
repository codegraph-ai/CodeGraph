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
