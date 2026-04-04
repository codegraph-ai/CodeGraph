// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Memory node types and builders
//!
//! Core types for representing memories in CodeGraph.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::temporal::TemporalMetadata;

/// Unique identifier for memory nodes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MemoryId(pub Uuid);

impl MemoryId {
    /// Create a new random MemoryId
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Create from existing UUID
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl Default for MemoryId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for MemoryId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::str::FromStr for MemoryId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

/// Severity levels for known issues
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IssueSeverity {
    Critical,
    High,
    #[default]
    Medium,
    Low,
    Info,
}

/// Types of memories that can be stored
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MemoryKind {
    /// Architectural decisions with rationale
    ArchitecturalDecision {
        decision: String,
        rationale: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        alternatives_considered: Option<Vec<String>>,
        #[serde(default)]
        stakeholders: Vec<String>,
    },
    /// Debugging context and solutions
    DebugContext {
        problem_description: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        root_cause: Option<String>,
        solution: String,
        #[serde(default)]
        symptoms: Vec<String>,
        #[serde(default)]
        related_errors: Vec<String>,
    },
    /// Known issues and workarounds
    KnownIssue {
        description: String,
        severity: IssueSeverity,
        #[serde(skip_serializing_if = "Option::is_none")]
        workaround: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tracking_id: Option<String>,
    },
    /// Project conventions and patterns
    Convention {
        name: String,
        description: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        pattern: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        anti_pattern: Option<String>,
    },
    /// General project context
    ProjectContext {
        topic: String,
        description: String,
        #[serde(default)]
        tags: Vec<String>,
    },
}

impl MemoryKind {
    /// Clean discriminant name for serialization (e.g. "debug_context", "known_issue")
    pub fn discriminant_name(&self) -> &'static str {
        match self {
            Self::ArchitecturalDecision { .. } => "architectural_decision",
            Self::DebugContext { .. } => "debug_context",
            Self::KnownIssue { .. } => "known_issue",
            Self::Convention { .. } => "convention",
            Self::ProjectContext { .. } => "project_context",
        }
    }
}

/// Type of code node that a memory is linked to
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkedNodeType {
    Function,
    Class,
    Module,
    File,
    Variable,
    Import,
    Interface,
    Trait,
}

/// Link from memory to a code graph node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeLink {
    /// Reference to CodeGraph NodeId
    pub node_id: String,
    /// Type of the linked node
    pub node_type: LinkedNodeType,
    /// Relevance score (0.0 to 1.0)
    pub relevance: f32,
    /// Specific line range if applicable
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_range: Option<(u32, u32)>,
}

impl CodeLink {
    pub fn new(node_id: impl Into<String>, node_type: LinkedNodeType) -> Self {
        Self {
            node_id: node_id.into(),
            node_type,
            relevance: 1.0,
            line_range: None,
        }
    }

    pub fn with_relevance(mut self, relevance: f32) -> Self {
        self.relevance = relevance.clamp(0.0, 1.0);
        self
    }

    pub fn with_line_range(mut self, start: u32, end: u32) -> Self {
        self.line_range = Some((start, end));
        self
    }
}

/// Source of the memory
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MemorySource {
    /// Explicitly provided by user
    UserProvided {
        #[serde(skip_serializing_if = "Option::is_none")]
        author: Option<String>,
    },
    /// Extracted from code analysis
    CodeExtracted { file_path: String },
    /// Derived from AI conversation
    ConversationDerived { conversation_id: String },
    /// From external documentation
    ExternalDoc { url: String },
    /// Extracted from git history
    GitHistory { commit_hash: String },
}

impl Default for MemorySource {
    fn default() -> Self {
        Self::UserProvided { author: None }
    }
}

/// A memory node containing project knowledge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryNode {
    /// Unique identifier
    pub id: MemoryId,
    /// Type and structured data
    pub kind: MemoryKind,
    /// Short descriptive title
    pub title: String,
    /// Full content/description
    pub content: String,
    /// Temporal metadata (bi-temporal)
    pub temporal: TemporalMetadata,
    /// Links to code graph nodes
    #[serde(default)]
    pub code_links: Vec<CodeLink>,
    /// Embedding vector (384d for fastembed BGE-Small-EN-v1.5)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedding: Option<Vec<f32>>,
    /// Searchable tags
    #[serde(default)]
    pub tags: Vec<String>,
    /// Source of this memory
    pub source: MemorySource,
    /// Confidence score (0.0 to 1.0)
    pub confidence: f32,
}

impl MemoryNode {
    /// Create a new builder for MemoryNode
    pub fn builder() -> MemoryNodeBuilder {
        MemoryNodeBuilder::new()
    }

    /// Check if this memory is currently valid
    pub fn is_current(&self) -> bool {
        self.temporal.is_current()
    }

    /// Get the searchable text for this memory
    pub fn searchable_text(&self) -> String {
        format!("{} {} {}", self.title, self.content, self.tags.join(" "))
    }
}

/// Builder for MemoryNode with fluent API
#[derive(Debug, Default)]
pub struct MemoryNodeBuilder {
    id: Option<MemoryId>,
    kind: Option<MemoryKind>,
    title: Option<String>,
    content: Option<String>,
    temporal: Option<TemporalMetadata>,
    code_links: Vec<CodeLink>,
    embedding: Option<Vec<f32>>,
    tags: Vec<String>,
    source: Option<MemorySource>,
    confidence: f32,
}

impl MemoryNodeBuilder {
    pub fn new() -> Self {
        Self {
            confidence: 1.0,
            ..Default::default()
        }
    }

    /// Set the memory ID (auto-generated if not set)
    pub fn id(mut self, id: MemoryId) -> Self {
        self.id = Some(id);
        self
    }

    /// Set as architectural decision
    pub fn architectural_decision(
        mut self,
        decision: impl Into<String>,
        rationale: impl Into<String>,
    ) -> Self {
        self.kind = Some(MemoryKind::ArchitecturalDecision {
            decision: decision.into(),
            rationale: rationale.into(),
            alternatives_considered: None,
            stakeholders: vec![],
        });
        self
    }

    /// Set as debug context
    pub fn debug_context(
        mut self,
        problem: impl Into<String>,
        solution: impl Into<String>,
    ) -> Self {
        self.kind = Some(MemoryKind::DebugContext {
            problem_description: problem.into(),
            root_cause: None,
            solution: solution.into(),
            symptoms: vec![],
            related_errors: vec![],
        });
        self
    }

    /// Set as known issue
    pub fn known_issue(mut self, description: impl Into<String>, severity: IssueSeverity) -> Self {
        self.kind = Some(MemoryKind::KnownIssue {
            description: description.into(),
            severity,
            workaround: None,
            tracking_id: None,
        });
        self
    }

    /// Set as convention
    pub fn convention(mut self, name: impl Into<String>, description: impl Into<String>) -> Self {
        self.kind = Some(MemoryKind::Convention {
            name: name.into(),
            description: description.into(),
            pattern: None,
            anti_pattern: None,
        });
        self
    }

    /// Set as project context
    pub fn project_context(
        mut self,
        topic: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        self.kind = Some(MemoryKind::ProjectContext {
            topic: topic.into(),
            description: description.into(),
            tags: vec![],
        });
        self
    }

    /// Set the kind directly
    pub fn kind(mut self, kind: MemoryKind) -> Self {
        self.kind = Some(kind);
        self
    }

    /// Set the title
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set the content
    pub fn content(mut self, content: impl Into<String>) -> Self {
        self.content = Some(content.into());
        self
    }

    /// Set temporal metadata
    pub fn temporal(mut self, temporal: TemporalMetadata) -> Self {
        self.temporal = Some(temporal);
        self
    }

    /// Link to a code node
    pub fn link_to_code(mut self, node_id: impl Into<String>, node_type: LinkedNodeType) -> Self {
        self.code_links.push(CodeLink::new(node_id, node_type));
        self
    }

    /// Add a code link
    pub fn code_link(mut self, link: CodeLink) -> Self {
        self.code_links.push(link);
        self
    }

    /// Set multiple code links
    pub fn code_links(mut self, links: Vec<CodeLink>) -> Self {
        self.code_links = links;
        self
    }

    /// Set embedding vector
    pub fn embedding(mut self, embedding: Vec<f32>) -> Self {
        self.embedding = Some(embedding);
        self
    }

    /// Add a tag
    pub fn tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    /// Set multiple tags
    pub fn tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    /// Set the source
    pub fn source(mut self, source: MemorySource) -> Self {
        self.source = Some(source);
        self
    }

    /// Set as user provided
    pub fn user_provided(mut self, author: Option<String>) -> Self {
        self.source = Some(MemorySource::UserProvided { author });
        self
    }

    /// Set as extracted from git
    pub fn from_git(mut self, commit_hash: impl Into<String>) -> Self {
        self.source = Some(MemorySource::GitHistory {
            commit_hash: commit_hash.into(),
        });
        self
    }

    /// Set the commit hash in temporal metadata
    pub fn at_commit(mut self, hash: impl Into<String>) -> Self {
        let mut temporal = self
            .temporal
            .take()
            .unwrap_or_else(TemporalMetadata::new_current);
        temporal.commit_hash = Some(hash.into());
        self.temporal = Some(temporal);
        self
    }

    /// Set confidence score
    pub fn confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence.clamp(0.0, 1.0);
        self
    }

    /// Build the MemoryNode
    pub fn build(self) -> Result<MemoryNode, MemoryNodeBuilderError> {
        let kind = self.kind.ok_or(MemoryNodeBuilderError::MissingKind)?;
        let title = self.title.ok_or(MemoryNodeBuilderError::MissingTitle)?;
        let content = self.content.ok_or(MemoryNodeBuilderError::MissingContent)?;

        Ok(MemoryNode {
            id: self.id.unwrap_or_default(),
            kind,
            title,
            content,
            temporal: self.temporal.unwrap_or_else(TemporalMetadata::new_current),
            code_links: self.code_links,
            embedding: self.embedding,
            tags: self.tags,
            source: self.source.unwrap_or_default(),
            confidence: self.confidence,
        })
    }
}

/// Errors that can occur when building a MemoryNode
#[derive(Debug, thiserror::Error)]
pub enum MemoryNodeBuilderError {
    #[error("Missing required field: kind")]
    MissingKind,
    #[error("Missing required field: title")]
    MissingTitle,
    #[error("Missing required field: content")]
    MissingContent,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_id_generation() {
        let id1 = MemoryId::new();
        let id2 = MemoryId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_memory_id_display() {
        let id = MemoryId::new();
        let s = id.to_string();
        assert!(!s.is_empty());
        assert!(s.contains('-')); // UUID format
    }

    #[test]
    fn test_memory_id_parse() {
        let id = MemoryId::new();
        let s = id.to_string();
        let parsed: MemoryId = s.parse().unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn test_builder_debug_context() {
        let memory = MemoryNode::builder()
            .debug_context(
                "Server crashes on large uploads",
                "Increase body size limit",
            )
            .title("Upload size limit fix")
            .content("The nginx config needs client_max_body_size increased")
            .tag("nginx")
            .tag("infrastructure")
            .confidence(0.9)
            .build()
            .unwrap();

        assert!(matches!(memory.kind, MemoryKind::DebugContext { .. }));
        assert_eq!(memory.title, "Upload size limit fix");
        assert_eq!(memory.tags.len(), 2);
        assert_eq!(memory.confidence, 0.9);
    }

    #[test]
    fn test_builder_architectural_decision() {
        let memory = MemoryNode::builder()
            .architectural_decision(
                "Use RocksDB for storage",
                "Fast embedded KV store with good Rust bindings",
            )
            .title("Storage engine choice")
            .content("Chose RocksDB over SQLite for better performance")
            .link_to_code("storage_module_123", LinkedNodeType::Module)
            .build()
            .unwrap();

        assert!(matches!(
            memory.kind,
            MemoryKind::ArchitecturalDecision { .. }
        ));
        assert_eq!(memory.code_links.len(), 1);
    }

    #[test]
    fn test_builder_known_issue() {
        let memory = MemoryNode::builder()
            .known_issue("Memory leak in parser", IssueSeverity::High)
            .title("Parser memory leak")
            .content("The TypeScript parser leaks memory on large files")
            .build()
            .unwrap();

        if let MemoryKind::KnownIssue { severity, .. } = memory.kind {
            assert_eq!(severity, IssueSeverity::High);
        } else {
            panic!("Expected KnownIssue");
        }
    }

    #[test]
    fn test_builder_missing_required() {
        let result = MemoryNode::builder()
            .title("Test")
            .content("Content")
            // Missing kind
            .build();

        assert!(matches!(result, Err(MemoryNodeBuilderError::MissingKind)));
    }

    #[test]
    fn test_code_link() {
        let link = CodeLink::new("func_123", LinkedNodeType::Function)
            .with_relevance(0.8)
            .with_line_range(10, 25);

        assert_eq!(link.node_id, "func_123");
        assert_eq!(link.relevance, 0.8);
        assert_eq!(link.line_range, Some((10, 25)));
    }

    #[test]
    fn test_searchable_text() {
        let memory = MemoryNode::builder()
            .debug_context("problem", "solution")
            .title("My Title")
            .content("My content here")
            .tag("tag1")
            .tag("tag2")
            .build()
            .unwrap();

        let text = memory.searchable_text();
        assert!(text.contains("My Title"));
        assert!(text.contains("My content here"));
        assert!(text.contains("tag1"));
        assert!(text.contains("tag2"));
    }

    #[test]
    fn test_memory_serialization() {
        let memory = MemoryNode::builder()
            .debug_context("problem", "solution")
            .title("Test")
            .content("Content")
            .build()
            .unwrap();

        let json = serde_json::to_string(&memory).unwrap();
        let deserialized: MemoryNode = serde_json::from_str(&json).unwrap();

        assert_eq!(memory.id, deserialized.id);
        assert_eq!(memory.title, deserialized.title);
    }
}
