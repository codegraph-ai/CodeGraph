// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Bi-temporal metadata for memory tracking
//!
//! Implements Graphiti-inspired bi-temporal model:
//! - **Valid time**: When the knowledge became true in the real world
//! - **Transaction time**: When the knowledge was recorded in the system

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Bi-temporal metadata for memory nodes
///
/// Tracks both when knowledge became true (valid time) and when it was
/// recorded (transaction time), enabling point-in-time queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalMetadata {
    /// When the knowledge became true in the real world
    pub valid_at: DateTime<Utc>,

    /// When the knowledge ceased to be true (None if still valid)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invalid_at: Option<DateTime<Utc>>,

    /// When this record was created in the system
    pub created_at: DateTime<Utc>,

    /// When this record was superseded by a newer version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub superseded_at: Option<DateTime<Utc>>,

    /// Git commit hash when this knowledge was valid
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_hash: Option<String>,

    /// Version tag (e.g., "v1.2.3") when this knowledge was valid
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_tag: Option<String>,
}

impl TemporalMetadata {
    /// Create metadata for currently valid knowledge
    pub fn new_current() -> Self {
        let now = Utc::now();
        Self {
            valid_at: now,
            invalid_at: None,
            created_at: now,
            superseded_at: None,
            commit_hash: None,
            version_tag: None,
        }
    }

    /// Create metadata with specific valid_at time
    pub fn new_with_valid_at(valid_at: DateTime<Utc>) -> Self {
        Self {
            valid_at,
            invalid_at: None,
            created_at: Utc::now(),
            superseded_at: None,
            commit_hash: None,
            version_tag: None,
        }
    }

    /// Check if this knowledge is currently valid
    ///
    /// Returns true if:
    /// - No invalid_at is set, OR
    /// - invalid_at is in the future
    pub fn is_current(&self) -> bool {
        match self.invalid_at {
            None => true,
            Some(invalid_at) => invalid_at > Utc::now(),
        }
    }

    /// Check if this knowledge was valid at a specific point in time
    pub fn was_valid_at(&self, time: DateTime<Utc>) -> bool {
        let valid_start = self.valid_at <= time;
        let valid_end = match self.invalid_at {
            None => true,
            Some(invalid_at) => invalid_at > time,
        };
        valid_start && valid_end
    }

    /// Check if this record was the current version at a specific point in time
    pub fn was_current_at(&self, time: DateTime<Utc>) -> bool {
        let created_before = self.created_at <= time;
        let not_superseded = match self.superseded_at {
            None => true,
            Some(superseded_at) => superseded_at > time,
        };
        created_before && not_superseded
    }

    /// Mark this knowledge as invalid from now
    pub fn invalidate(&mut self) {
        self.invalid_at = Some(Utc::now());
    }

    /// Mark this knowledge as invalid with a specific timestamp
    pub fn invalidate_at(&mut self, at: DateTime<Utc>) {
        self.invalid_at = Some(at);
    }

    /// Mark this record as superseded by a newer version
    pub fn supersede(&mut self) {
        self.superseded_at = Some(Utc::now());
    }

    /// Set the commit hash
    pub fn with_commit(mut self, hash: impl Into<String>) -> Self {
        self.commit_hash = Some(hash.into());
        self
    }

    /// Set the version tag
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version_tag = Some(version.into());
        self
    }

    /// Get the duration this knowledge has been valid
    pub fn valid_duration(&self) -> chrono::Duration {
        let end = self.invalid_at.unwrap_or_else(Utc::now);
        end - self.valid_at
    }

    /// Check if this knowledge is associated with a specific commit
    pub fn is_at_commit(&self, hash: &str) -> bool {
        self.commit_hash.as_deref() == Some(hash)
    }
}

impl Default for TemporalMetadata {
    fn default() -> Self {
        Self::new_current()
    }
}

/// Result of checking if knowledge needs review based on code changes
#[derive(Debug, Clone)]
pub struct MemoryReviewSuggestion {
    /// The memory that needs review
    pub memory_id: crate::node::MemoryId,
    /// Reason for the review suggestion
    pub reason: String,
    /// Suggested action
    pub suggested_action: SuggestedAction,
    /// How strongly we suggest this action (0.0 to 1.0)
    pub confidence: f32,
}

/// Suggested action for a memory after code changes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestedAction {
    /// Memory should be invalidated
    Invalidate,
    /// Memory should be reviewed by user
    Review,
    /// Memory content might need updating
    Update,
    /// No action needed
    None,
}

/// Type of code change that occurred
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeChangeType {
    /// Code was deleted
    Deleted,
    /// Function/class signature changed
    SignatureChanged,
    /// Major refactoring occurred
    MajorRefactor,
    /// Minor edit (formatting, comments, etc.)
    MinorEdit,
    /// File was renamed
    Renamed,
    /// File was moved
    Moved,
}

impl CodeChangeType {
    /// Get the suggested action for this type of change
    pub fn suggested_action(&self) -> SuggestedAction {
        match self {
            Self::Deleted => SuggestedAction::Invalidate,
            Self::SignatureChanged => SuggestedAction::Review,
            Self::MajorRefactor => SuggestedAction::Review,
            Self::MinorEdit => SuggestedAction::None,
            Self::Renamed => SuggestedAction::Update,
            Self::Moved => SuggestedAction::Update,
        }
    }

    /// Get confidence for the suggested action
    pub fn action_confidence(&self) -> f32 {
        match self {
            Self::Deleted => 1.0,
            Self::SignatureChanged => 0.9,
            Self::MajorRefactor => 0.8,
            Self::MinorEdit => 0.0,
            Self::Renamed => 0.7,
            Self::Moved => 0.7,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn test_new_current() {
        let meta = TemporalMetadata::new_current();
        assert!(meta.is_current());
        assert!(meta.invalid_at.is_none());
        assert!(meta.superseded_at.is_none());
    }

    #[test]
    fn test_is_current_with_future_invalid_at() {
        let mut meta = TemporalMetadata::new_current();
        meta.invalid_at = Some(Utc::now() + Duration::hours(1));
        assert!(meta.is_current());
    }

    #[test]
    fn test_is_current_with_past_invalid_at() {
        let mut meta = TemporalMetadata::new_current();
        meta.invalid_at = Some(Utc::now() - Duration::hours(1));
        assert!(!meta.is_current());
    }

    #[test]
    fn test_was_valid_at() {
        let mut meta = TemporalMetadata::new_current();
        let now = Utc::now();

        // Set valid_at to 1 hour ago
        meta.valid_at = now - Duration::hours(1);

        // Should be valid now
        assert!(meta.was_valid_at(now));

        // Should be valid 30 minutes ago
        assert!(meta.was_valid_at(now - Duration::minutes(30)));

        // Should NOT be valid 2 hours ago (before valid_at)
        assert!(!meta.was_valid_at(now - Duration::hours(2)));
    }

    #[test]
    fn test_was_valid_at_with_invalid() {
        let mut meta = TemporalMetadata::new_current();
        let now = Utc::now();

        meta.valid_at = now - Duration::hours(2);
        meta.invalid_at = Some(now - Duration::hours(1));

        // Should be valid 90 minutes ago (between valid_at and invalid_at)
        assert!(meta.was_valid_at(now - Duration::minutes(90)));

        // Should NOT be valid now (after invalid_at)
        assert!(!meta.was_valid_at(now));
    }

    #[test]
    fn test_invalidate() {
        let mut meta = TemporalMetadata::new_current();
        assert!(meta.is_current());

        meta.invalidate();

        // Should no longer be current after invalidation
        assert!(!meta.is_current());
        assert!(meta.invalid_at.is_some());
    }

    #[test]
    fn test_supersede() {
        let mut meta = TemporalMetadata::new_current();
        assert!(meta.superseded_at.is_none());

        meta.supersede();

        assert!(meta.superseded_at.is_some());
    }

    #[test]
    fn test_with_commit() {
        let meta = TemporalMetadata::new_current().with_commit("abc123");
        assert_eq!(meta.commit_hash.as_deref(), Some("abc123"));
        assert!(meta.is_at_commit("abc123"));
        assert!(!meta.is_at_commit("def456"));
    }

    #[test]
    fn test_with_version() {
        let meta = TemporalMetadata::new_current().with_version("v1.2.3");
        assert_eq!(meta.version_tag.as_deref(), Some("v1.2.3"));
    }

    #[test]
    fn test_valid_duration() {
        let mut meta = TemporalMetadata::new_current();
        meta.valid_at = Utc::now() - Duration::hours(2);
        meta.invalid_at = Some(Utc::now() - Duration::hours(1));

        let duration = meta.valid_duration();
        assert_eq!(duration.num_hours(), 1);
    }

    #[test]
    fn test_code_change_type_suggested_action() {
        assert_eq!(
            CodeChangeType::Deleted.suggested_action(),
            SuggestedAction::Invalidate
        );
        assert_eq!(
            CodeChangeType::SignatureChanged.suggested_action(),
            SuggestedAction::Review
        );
        assert_eq!(
            CodeChangeType::MinorEdit.suggested_action(),
            SuggestedAction::None
        );
    }

    #[test]
    fn test_serialization() {
        let meta = TemporalMetadata::new_current()
            .with_commit("abc123")
            .with_version("v1.0.0");

        let json = serde_json::to_string(&meta).unwrap();
        let deserialized: TemporalMetadata = serde_json::from_str(&json).unwrap();

        assert_eq!(meta.commit_hash, deserialized.commit_hash);
        assert_eq!(meta.version_tag, deserialized.version_tag);
    }
}
