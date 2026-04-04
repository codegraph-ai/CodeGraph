// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Metrics collected during parsing
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParserMetrics {
    /// Total files attempted to parse
    pub files_attempted: usize,

    /// Files successfully parsed
    pub files_succeeded: usize,

    /// Files that failed parsing
    pub files_failed: usize,

    /// Total time spent parsing
    #[serde(with = "duration_serde")]
    pub total_parse_time: Duration,

    /// Total entities extracted
    pub total_entities: usize,

    /// Total relationships extracted
    pub total_relationships: usize,

    /// Peak memory usage (if available)
    pub peak_memory_bytes: Option<usize>,
}

// Helper module for serializing Duration
mod duration_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        duration.as_secs().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs: u64 = u64::deserialize(deserializer)?;
        Ok(Duration::from_secs(secs))
    }
}

impl Default for ParserMetrics {
    fn default() -> Self {
        Self {
            files_attempted: 0,
            files_succeeded: 0,
            files_failed: 0,
            total_parse_time: Duration::ZERO,
            total_entities: 0,
            total_relationships: 0,
            peak_memory_bytes: None,
        }
    }
}

impl ParserMetrics {
    /// Success rate (0.0 to 1.0)
    pub fn success_rate(&self) -> f64 {
        if self.files_attempted == 0 {
            0.0
        } else {
            self.files_succeeded as f64 / self.files_attempted as f64
        }
    }

    /// Average parse time per file
    pub fn avg_parse_time(&self) -> Duration {
        if self.files_succeeded == 0 {
            Duration::ZERO
        } else {
            self.total_parse_time / self.files_succeeded as u32
        }
    }

    /// Average entities per file
    pub fn avg_entities_per_file(&self) -> f64 {
        if self.files_succeeded == 0 {
            0.0
        } else {
            self.total_entities as f64 / self.files_succeeded as f64
        }
    }

    /// Merge another metrics object into this one
    pub fn merge(&mut self, other: &ParserMetrics) {
        self.files_attempted += other.files_attempted;
        self.files_succeeded += other.files_succeeded;
        self.files_failed += other.files_failed;
        self.total_parse_time += other.total_parse_time;
        self.total_entities += other.total_entities;
        self.total_relationships += other.total_relationships;

        // Take max memory
        self.peak_memory_bytes = match (self.peak_memory_bytes, other.peak_memory_bytes) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };
    }
}
