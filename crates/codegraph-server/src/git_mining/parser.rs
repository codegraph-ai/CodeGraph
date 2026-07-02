// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Git output parsing for memory extraction.

use super::GitMiningError;
use codegraph_memory::MemoryKind;

/// Separator used in git log format output.
pub const FIELD_SEPARATOR: &str = "␞"; // ASCII Record Separator
pub const COMMIT_SEPARATOR: &str = "␝"; // ASCII Group Separator

/// Standard git log format for parsing commits.
pub const LOG_FORMAT: &str = concat!(
    "%H", "␞", // hash
    "%s", "␞", // subject
    "%b", "␞", // body
    "%an", "␞", // author name
    "%ae", "␞",   // author email
    "%ai", // author date
    "␝"    // commit separator
);

/// Basic commit information extracted from git log.
#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub hash: String,
    pub subject: String,
    pub body: String,
    pub author_name: String,
    pub author_email: String,
    pub author_date: String,
}

/// Pattern detected in a commit message.
#[derive(Debug, Clone, PartialEq)]
pub enum CommitPattern {
    /// Bug fix commit (fix:, bug:, fixed, closes #)
    BugFix { issue_ref: Option<String> },
    /// Feature commit (feat:, feature:, add:)
    Feature,
    /// Refactoring commit (refactor:, cleanup:)
    Refactor,
    /// Architecture decision (arch:, adr:, decision:)
    ArchitecturalDecision,
    /// Breaking change (BREAKING:, breaking change)
    BreakingChange,
    /// Deprecation (deprecate:, deprecated)
    Deprecation,
    /// Revert commit
    Revert { reverted_hash: Option<String> },
    /// Documentation change
    Documentation,
    /// Test-related commit
    Test,
    /// Other/unknown pattern
    Other,
}

/// A parsed commit with extracted pattern and metadata.
#[derive(Debug, Clone)]
pub struct ParsedCommit {
    pub info: CommitInfo,
    pub pattern: CommitPattern,
    pub files_changed: Vec<String>,
    pub confidence: f32,
}

impl ParsedCommit {
    /// Determine the memory kind for this commit.
    pub fn to_memory_kind(&self) -> Option<MemoryKind> {
        match &self.pattern {
            CommitPattern::BugFix { .. } => Some(MemoryKind::DebugContext {
                problem_description: self.extract_problem(),
                root_cause: self.extract_root_cause(),
                solution: self.info.subject.clone(),
                symptoms: vec![],
                related_errors: vec![],
            }),
            CommitPattern::ArchitecturalDecision => Some(MemoryKind::ArchitecturalDecision {
                decision: self.info.subject.clone(),
                rationale: self.info.body.clone(),
                alternatives_considered: None,
                stakeholders: vec![self.info.author_name.clone()],
            }),
            CommitPattern::Feature => {
                // Only create memory for features with substantial body text
                if !self.info.body.is_empty() && self.info.body.len() > 50 {
                    Some(MemoryKind::ArchitecturalDecision {
                        decision: self.info.subject.clone(),
                        rationale: self.info.body.clone(),
                        alternatives_considered: None,
                        stakeholders: vec![self.info.author_name.clone()],
                    })
                } else {
                    None // Skip features without explanation
                }
            }
            CommitPattern::BreakingChange => Some(MemoryKind::KnownIssue {
                description: self.info.subject.clone(),
                severity: codegraph_memory::IssueSeverity::High,
                workaround: self.extract_workaround(),
                tracking_id: None,
            }),
            CommitPattern::Deprecation => Some(MemoryKind::KnownIssue {
                description: format!("Deprecated: {}", self.info.subject),
                severity: codegraph_memory::IssueSeverity::Medium,
                workaround: self.extract_workaround(),
                tracking_id: None,
            }),
            CommitPattern::Revert { .. } => Some(MemoryKind::KnownIssue {
                description: format!("Reverted: {}", self.info.subject),
                severity: codegraph_memory::IssueSeverity::Medium,
                workaround: None,
                tracking_id: None,
            }),
            _ => None, // Refactor, Doc, Test, Other don't create memories
        }
    }

    fn extract_problem(&self) -> String {
        // Try to extract problem description from body or use subject
        if !self.info.body.is_empty() {
            // Look for common patterns like "Problem:", "Issue:", "Bug:"
            for line in self.info.body.lines() {
                let lower = line.to_lowercase();
                if lower.starts_with("problem:")
                    || lower.starts_with("issue:")
                    || lower.starts_with("bug:")
                {
                    return line
                        .split_once(':')
                        .map(|x| x.1)
                        .unwrap_or("")
                        .trim()
                        .to_string();
                }
            }
        }
        // Fall back to subject
        self.info.subject.clone()
    }

    fn extract_root_cause(&self) -> Option<String> {
        // Look for root cause in body
        for line in self.info.body.lines() {
            let lower = line.to_lowercase();
            if lower.starts_with("cause:")
                || lower.starts_with("root cause:")
                || lower.starts_with("reason:")
            {
                return Some(
                    line.split_once(':')
                        .map(|x| x.1)
                        .unwrap_or("")
                        .trim()
                        .to_string(),
                );
            }
        }
        None
    }

    fn extract_workaround(&self) -> Option<String> {
        for line in self.info.body.lines() {
            let lower = line.to_lowercase();
            if lower.starts_with("workaround:") || lower.starts_with("migration:") {
                return Some(
                    line.split_once(':')
                        .map(|x| x.1)
                        .unwrap_or("")
                        .trim()
                        .to_string(),
                );
            }
        }
        None
    }
}

/// Parse git log output into structured commit information.
pub fn parse_log_output(output: &str) -> Result<Vec<CommitInfo>, GitMiningError> {
    let mut commits = Vec::new();

    for commit_str in output.split(COMMIT_SEPARATOR) {
        let commit_str = commit_str.trim();
        if commit_str.is_empty() {
            continue;
        }

        let fields: Vec<&str> = commit_str.split(FIELD_SEPARATOR).collect();
        if fields.len() < 6 {
            continue; // Skip malformed entries
        }

        commits.push(CommitInfo {
            hash: fields[0].to_string(),
            subject: fields[1].to_string(),
            body: fields[2].trim().to_string(),
            author_name: fields[3].to_string(),
            author_email: fields[4].to_string(),
            author_date: fields[5].trim().to_string(),
        });
    }

    Ok(commits)
}

/// Detect the pattern of a commit from its subject and body.
///
/// Uses a two-tier approach:
/// - **Tier 1 (high confidence)**: Conventional prefixes like `fix:`, `feat:`
/// - **Tier 2 (moderate confidence)**: Keywords anywhere in the message,
///   catching lazy commit messages like "fixed the login bug" or "added auth"
pub fn detect_pattern(commit: &CommitInfo) -> (CommitPattern, f32) {
    let subject_lower = commit.subject.to_lowercase();
    let body_lower = commit.body.to_lowercase();

    // === Tier 1: Conventional prefix patterns (high confidence) ===

    // Bug fixes — conventional
    if subject_lower.starts_with("fix:")
        || subject_lower.starts_with("fix(")
        || subject_lower.starts_with("bug:")
        || subject_lower.starts_with("bugfix:")
    {
        let issue_ref = extract_issue_reference(&commit.subject);
        return (CommitPattern::BugFix { issue_ref }, 0.9);
    }

    // Breaking changes
    if subject_lower.starts_with("breaking:")
        || body_lower.contains("breaking change")
        || body_lower.starts_with("breaking:")
    {
        return (CommitPattern::BreakingChange, 0.95);
    }

    // Deprecations — conventional
    if subject_lower.starts_with("deprecate:") || subject_lower.starts_with("deprecated:") {
        return (CommitPattern::Deprecation, 0.9);
    }

    // Reverts
    if subject_lower.starts_with("revert") {
        let reverted_hash = extract_revert_hash(&commit.subject);
        return (CommitPattern::Revert { reverted_hash }, 0.95);
    }

    // Architectural decisions — conventional
    if subject_lower.starts_with("arch:")
        || subject_lower.starts_with("adr:")
        || subject_lower.starts_with("decision:")
        || body_lower.contains("architectural decision")
        || body_lower.contains("adr-")
    {
        return (CommitPattern::ArchitecturalDecision, 0.85);
    }

    // Features — conventional
    if subject_lower.starts_with("feat:")
        || subject_lower.starts_with("feat(")
        || subject_lower.starts_with("feature:")
        || subject_lower.starts_with("add:")
    {
        return (CommitPattern::Feature, 0.8);
    }

    // Refactoring — conventional
    if subject_lower.starts_with("refactor:")
        || subject_lower.starts_with("refactor(")
        || subject_lower.starts_with("cleanup:")
        || subject_lower.starts_with("clean:")
    {
        return (CommitPattern::Refactor, 0.8);
    }

    // Documentation — conventional
    if subject_lower.starts_with("docs:")
        || subject_lower.starts_with("doc:")
        || subject_lower.starts_with("documentation:")
    {
        return (CommitPattern::Documentation, 0.9);
    }

    // Tests — conventional
    if subject_lower.starts_with("test:")
        || subject_lower.starts_with("tests:")
        || subject_lower.starts_with("testing:")
    {
        return (CommitPattern::Test, 0.9);
    }

    // === Tier 2: Keyword detection anywhere in message (moderate confidence) ===
    // Catches lazy messages like "fixed login bug", "added new endpoint", etc.

    // Bug fixes — keyword matching
    if subject_lower.contains("fixed ")
        || subject_lower.contains("fixes ")
        || subject_lower.contains("fixing ")
        || subject_lower.contains("closes #")
        || subject_lower.contains("fixes #")
        || subject_lower.contains("bug ")
        || subject_lower.contains("bugfix")
        || subject_lower.contains("hotfix")
        || subject_lower.contains("patch ")
        || subject_lower.contains("resolve ")
        || subject_lower.contains("resolved ")
        || subject_lower.contains("resolves ")
        || subject_lower.contains("crash")
        || subject_lower.contains("error handling")
        || subject_lower.contains("workaround")
    {
        let issue_ref = extract_issue_reference(&commit.subject);
        return (CommitPattern::BugFix { issue_ref }, 0.75);
    }

    // Breaking changes — keyword matching
    if subject_lower.contains("breaking") || subject_lower.contains("incompatible") {
        return (CommitPattern::BreakingChange, 0.8);
    }

    // Deprecations — keyword matching
    if subject_lower.contains("deprecat") || body_lower.contains("deprecat") {
        return (CommitPattern::Deprecation, 0.75);
    }

    // Tests — keyword matching (before features, since "added tests" should be Test not Feature)
    if subject_lower.contains("test")
        || subject_lower.contains("spec ")
        || subject_lower.contains("coverage")
    {
        return (CommitPattern::Test, 0.7);
    }

    // Documentation — keyword matching (before features, since "added docs" should be Docs)
    if subject_lower.contains("readme")
        || subject_lower.contains("document")
        || subject_lower.contains("comment")
        || subject_lower.contains("changelog")
    {
        return (CommitPattern::Documentation, 0.7);
    }

    // Refactoring — keyword matching (before features, since "restructured" is not a feature)
    if subject_lower.contains("refactor")
        || subject_lower.contains("restructur")
        || subject_lower.contains("reorganiz")
        || subject_lower.contains("simplif")
        || subject_lower.contains("clean up")
        || subject_lower.contains("cleanup")
        || subject_lower.contains("move ")
        || subject_lower.contains("rename ")
        || subject_lower.contains("extract ")
    {
        return (CommitPattern::Refactor, 0.7);
    }

    // Features — keyword matching (last among content patterns, most generic)
    if subject_lower.contains("added ")
        || subject_lower.contains("adding ")
        || subject_lower.contains("implement")
        || subject_lower.contains("introduce")
        || subject_lower.contains("new ")
        || subject_lower.contains("support for")
        || subject_lower.contains("enable ")
    {
        return (CommitPattern::Feature, 0.7);
    }

    (CommitPattern::Other, 0.5)
}

/// Extract issue reference (e.g., #123) from commit message.
fn extract_issue_reference(text: &str) -> Option<String> {
    // Look for patterns like #123, GH-123, JIRA-123
    let re_patterns = [
        r"#(\d+)",
        r"(?i)gh-(\d+)",
        r"(?i)closes?\s+#(\d+)",
        r"(?i)fixes?\s+#(\d+)",
    ];

    for pattern in &re_patterns {
        if let Ok(re) = regex::Regex::new(pattern) {
            if let Some(caps) = re.captures(text) {
                if let Some(m) = caps.get(1) {
                    return Some(format!("#{}", m.as_str()));
                }
            }
        }
    }
    None
}

/// Extract reverted commit hash from revert commit message.
fn extract_revert_hash(subject: &str) -> Option<String> {
    // Pattern: "Revert "original message"" or "Revert abc123"
    if let Ok(re) = regex::Regex::new(r"(?i)revert\s+([a-f0-9]{7,40})") {
        if let Some(caps) = re.captures(subject) {
            if let Some(m) = caps.get(1) {
                return Some(m.as_str().to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_bug_fix() {
        let commit = CommitInfo {
            hash: "abc123".to_string(),
            subject: "fix: resolve null pointer in parser".to_string(),
            body: "".to_string(),
            author_name: "Test".to_string(),
            author_email: "test@example.com".to_string(),
            author_date: "2024-01-01".to_string(),
        };

        let (pattern, confidence) = detect_pattern(&commit);
        assert!(matches!(pattern, CommitPattern::BugFix { .. }));
        assert!(confidence >= 0.9);
    }

    #[test]
    fn test_detect_breaking_change() {
        let commit = CommitInfo {
            hash: "abc123".to_string(),
            subject: "feat: new API".to_string(),
            body: "BREAKING CHANGE: removed old endpoint".to_string(),
            author_name: "Test".to_string(),
            author_email: "test@example.com".to_string(),
            author_date: "2024-01-01".to_string(),
        };

        let (pattern, _) = detect_pattern(&commit);
        assert!(matches!(pattern, CommitPattern::BreakingChange));
    }

    #[test]
    fn test_extract_issue_reference() {
        assert_eq!(
            extract_issue_reference("fix: resolve #123"),
            Some("#123".to_string())
        );
        assert_eq!(
            extract_issue_reference("closes #456"),
            Some("#456".to_string())
        );
        assert_eq!(extract_issue_reference("no issue"), None);
    }

    #[test]
    fn extract_issue_reference_gh_prefix_arm() {
        // With no `#` in the text the first pattern misses, so resolution falls
        // through to the `(?i)gh-(\d+)` arm - the only arm previously untested,
        // since every prior case contained a `#` that the leading `#(\d+)`
        // pattern matched first. The captured digits are re-emitted as `#N`.
        assert_eq!(
            extract_issue_reference("see GH-42 for details"),
            Some("#42".to_string())
        );
        // The pattern is case-insensitive, so lowercase `gh-` resolves too.
        assert_eq!(
            extract_issue_reference("fixes gh-7"),
            Some("#7".to_string())
        );
        // When both a bare `#` and a `gh-` token are present, the earlier
        // `#(\d+)` pattern wins and the gh- arm is never reached.
        assert_eq!(
            extract_issue_reference("gh-1 tracked as #99"),
            Some("#99".to_string())
        );
    }

    #[test]
    fn detect_pattern_bugfix_carries_extracted_issue_ref() {
        // detect_pattern's BugFix arms both call extract_issue_reference and
        // thread the result into the pattern, but no prior test asserted the
        // populated issue_ref field - only that the pattern was a BugFix.
        // Conventional `fix:` prefix (0.9 tier).
        let (pattern, _) = detect_pattern(&make_commit("fix: resolve crash, closes #321"));
        match pattern {
            CommitPattern::BugFix { issue_ref } => {
                assert_eq!(issue_ref.as_deref(), Some("#321"));
            }
            other => panic!("expected BugFix, got {:?}", other),
        }
        // A conventional bug fix with no issue token leaves issue_ref None.
        let (pattern, _) = detect_pattern(&make_commit("fix: tidy up logging"));
        match pattern {
            CommitPattern::BugFix { issue_ref } => assert!(issue_ref.is_none()),
            other => panic!("expected BugFix, got {:?}", other),
        }
    }

    fn make_commit(subject: &str) -> CommitInfo {
        CommitInfo {
            hash: "abc123".to_string(),
            subject: subject.to_string(),
            body: String::new(),
            author_name: "Test".to_string(),
            author_email: "test@example.com".to_string(),
            author_date: "2024-01-01".to_string(),
        }
    }

    #[test]
    fn test_detect_lazy_bug_fix() {
        // "fixed" without conventional prefix
        let (pattern, confidence) = detect_pattern(&make_commit("Fixed the login page crash"));
        assert!(matches!(pattern, CommitPattern::BugFix { .. }));
        assert!(confidence >= 0.7);
        assert!(confidence < 0.9); // Lower than conventional

        // "resolved" keyword
        let (pattern, _) = detect_pattern(&make_commit("Resolved null pointer in parser"));
        assert!(matches!(pattern, CommitPattern::BugFix { .. }));
    }

    #[test]
    fn test_detect_lazy_feature() {
        let (pattern, confidence) = detect_pattern(&make_commit("Added dark mode support"));
        assert!(matches!(pattern, CommitPattern::Feature));
        assert!(confidence >= 0.7);

        let (pattern, _) = detect_pattern(&make_commit("Implement user authentication"));
        assert!(matches!(pattern, CommitPattern::Feature));
    }

    #[test]
    fn test_detect_lazy_refactor() {
        let (pattern, _) = detect_pattern(&make_commit("Refactored the database layer"));
        assert!(matches!(pattern, CommitPattern::Refactor));

        let (pattern, _) = detect_pattern(&make_commit("Rename UserService to AuthService"));
        assert!(matches!(pattern, CommitPattern::Refactor));
    }

    #[test]
    fn test_detect_lazy_docs() {
        let (pattern, _) = detect_pattern(&make_commit("Updated README with install instructions"));
        assert!(matches!(pattern, CommitPattern::Documentation));
    }

    #[test]
    fn test_detect_lazy_test() {
        let (pattern, _) = detect_pattern(&make_commit("Added unit tests for auth module"));
        assert!(matches!(pattern, CommitPattern::Test));
    }

    #[test]
    fn test_detect_deprecation_conventional_and_keyword() {
        // Conventional `deprecate:` prefix — high confidence. No prior test
        // ever produced a Deprecation pattern, so both arms were unexercised.
        let (pattern, confidence) = detect_pattern(&make_commit("deprecate: old config format"));
        assert!(matches!(pattern, CommitPattern::Deprecation));
        assert!((confidence - 0.9).abs() < f32::EPSILON);

        // The `deprecated:` spelling is the other half of the conventional arm.
        let (pattern, _) = detect_pattern(&make_commit("deprecated: legacy auth flow"));
        assert!(matches!(pattern, CommitPattern::Deprecation));

        // Keyword anywhere in a lazy subject drops to the moderate tier (0.75).
        let (pattern, confidence) =
            detect_pattern(&make_commit("Marked the legacy API as deprecated"));
        assert!(matches!(pattern, CommitPattern::Deprecation));
        assert!((confidence - 0.75).abs() < f32::EPSILON);
    }

    #[test]
    fn test_detect_architectural_decision() {
        // All three conventional prefixes map to ArchitecturalDecision at 0.85;
        // detect_pattern's arch tier had no prior coverage (only the
        // to_memory_kind path constructed the pattern directly).
        for subject in [
            "arch: adopt hexagonal layering",
            "adr: use RocksDB",
            "decision: split API",
        ] {
            let (pattern, confidence) = detect_pattern(&make_commit(subject));
            assert!(
                matches!(pattern, CommitPattern::ArchitecturalDecision),
                "{subject:?} should be an architectural decision"
            );
            assert!((confidence - 0.85).abs() < f32::EPSILON);
        }

        // A body mention triggers it even when the subject is neutral (the
        // body_lower.contains("architectural decision") arm).
        let mut commit = make_commit("update storage layer");
        commit.body = "This records an architectural decision to use append-only logs.".to_string();
        let (pattern, _) = detect_pattern(&commit);
        assert!(matches!(pattern, CommitPattern::ArchitecturalDecision));
    }

    #[test]
    fn test_conventional_higher_confidence_than_keyword() {
        let (_, conv_confidence) = detect_pattern(&make_commit("fix: null pointer"));
        let (_, kw_confidence) = detect_pattern(&make_commit("Fixed null pointer"));
        assert!(conv_confidence > kw_confidence);
    }

    #[test]
    fn test_unknown_commit_stays_other() {
        let (pattern, confidence) = detect_pattern(&make_commit("wip"));
        assert!(matches!(pattern, CommitPattern::Other));
        assert!(confidence < 0.7);
    }

    #[test]
    fn detect_pattern_breaking_change_subject_keyword_tier() {
        // Prior breaking tests all seed the body with "BREAKING CHANGE", hitting
        // the Tier-1 body arm (0.95). The Tier-2 subject-keyword arm (0.8) that
        // matches a bare "breaking"/"incompatible" token stayed unexercised.
        let (pattern, confidence) =
            detect_pattern(&make_commit("Made an incompatible schema adjustment"));
        assert!(matches!(pattern, CommitPattern::BreakingChange));
        assert!((confidence - 0.8).abs() < f32::EPSILON);

        // The "breaking" keyword alone (no "breaking change" phrase, no prefix)
        // resolves through the same 0.8 keyword arm.
        let (pattern, confidence) = detect_pattern(&make_commit("A breaking rework of the API"));
        assert!(matches!(pattern, CommitPattern::BreakingChange));
        assert!((confidence - 0.8).abs() < f32::EPSILON);
    }

    #[test]
    fn detect_pattern_deprecation_body_keyword_arm() {
        // The keyword deprecation test puts "deprecated" in the subject; the
        // body_lower.contains("deprecat") half of that arm had no coverage.
        let mut commit = make_commit("update the client wrapper");
        commit.body = "This quietly deprecates the legacy connection path.".to_string();
        let (pattern, confidence) = detect_pattern(&commit);
        assert!(matches!(pattern, CommitPattern::Deprecation));
        assert!((confidence - 0.75).abs() < f32::EPSILON);
    }

    #[test]
    fn detect_pattern_test_keyword_coverage_and_spec_arms() {
        // lazy_test uses the "tests" token; the "coverage" and "spec " arms of
        // the same keyword tier were never reached.
        let (pattern, confidence) = detect_pattern(&make_commit("Improve coverage of the parser"));
        assert!(matches!(pattern, CommitPattern::Test));
        assert!((confidence - 0.7).abs() < f32::EPSILON);

        let (pattern, _) = detect_pattern(&make_commit("Add spec for the widget renderer"));
        assert!(matches!(pattern, CommitPattern::Test));
    }

    #[test]
    fn detect_pattern_refactor_alternate_keywords() {
        // lazy_refactor only covers "refactored" and "rename"; the reorganiz /
        // simplif / extract keyword arms stayed unexercised.
        for subject in [
            "Reorganized the module layout",
            "Simplify the config loader",
            "Extract helper from the parser",
        ] {
            let (pattern, _) = detect_pattern(&make_commit(subject));
            assert!(
                matches!(pattern, CommitPattern::Refactor),
                "{subject:?} should be a refactor"
            );
        }
    }

    #[test]
    fn detect_pattern_feature_alternate_keywords() {
        // lazy_feature covers "added" and "implement"; introduce / support for /
        // enable were untested feature keyword arms.
        for subject in [
            "Introduce a plugin registry",
            "Support for streaming responses",
            "Enable background indexing",
        ] {
            let (pattern, confidence) = detect_pattern(&make_commit(subject));
            assert!(
                matches!(pattern, CommitPattern::Feature),
                "{subject:?} should be a feature"
            );
            assert!((confidence - 0.7).abs() < f32::EPSILON);
        }
    }

    // --- parse_log_output ---

    /// Build a single formatted git-log record from its six fields.
    fn record(fields: &[&str]) -> String {
        let mut s = fields.join(FIELD_SEPARATOR);
        s.push_str(COMMIT_SEPARATOR);
        s
    }

    #[test]
    fn parse_log_output_empty_input_yields_no_commits() {
        assert!(parse_log_output("").unwrap().is_empty());
        assert!(parse_log_output("   \n  ").unwrap().is_empty());
    }

    #[test]
    fn parse_log_output_parses_fields_and_trims_body_and_date() {
        let out = record(&[
            "deadbeef",
            "fix: thing",
            "  body text  ",
            "Jane",
            "jane@example.com",
            "  2024-01-01 12:00:00 +0000  ",
        ]);
        let commits = parse_log_output(&out).unwrap();
        assert_eq!(commits.len(), 1);
        let c = &commits[0];
        assert_eq!(c.hash, "deadbeef");
        assert_eq!(c.subject, "fix: thing");
        // body and author_date are trimmed; subject is not
        assert_eq!(c.body, "body text");
        assert_eq!(c.author_name, "Jane");
        assert_eq!(c.author_email, "jane@example.com");
        assert_eq!(c.author_date, "2024-01-01 12:00:00 +0000");
    }

    #[test]
    fn parse_log_output_skips_malformed_records_with_too_few_fields() {
        // First record has only 3 fields (< 6) and must be dropped; second is valid.
        let mut out = ["h", "s", "b"].join(FIELD_SEPARATOR);
        out.push_str(COMMIT_SEPARATOR);
        out.push_str(&record(&["h2", "s2", "b2", "an", "ae", "ad"]));
        let commits = parse_log_output(&out).unwrap();
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].hash, "h2");
    }

    #[test]
    fn parse_log_output_parses_multiple_commits() {
        let mut out = record(&["h1", "s1", "b1", "a1", "e1", "d1"]);
        out.push_str(&record(&["h2", "s2", "b2", "a2", "e2", "d2"]));
        let commits = parse_log_output(&out).unwrap();
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].hash, "h1");
        assert_eq!(commits[1].subject, "s2");
    }

    // --- extract_revert_hash (via detect_pattern) ---

    #[test]
    fn revert_extracts_hash_when_present() {
        let (pattern, _) = detect_pattern(&make_commit("Revert 1a2b3c4d5e"));
        match pattern {
            CommitPattern::Revert { reverted_hash } => {
                assert_eq!(reverted_hash.as_deref(), Some("1a2b3c4d5e"));
            }
            other => panic!("expected Revert, got {:?}", other),
        }
    }

    #[test]
    fn revert_without_hash_leaves_none() {
        let (pattern, _) = detect_pattern(&make_commit("Revert \"feat: add login\""));
        match pattern {
            CommitPattern::Revert { reverted_hash } => assert!(reverted_hash.is_none()),
            other => panic!("expected Revert, got {:?}", other),
        }
    }

    // --- to_memory_kind ---

    /// Build a ParsedCommit from a subject/body, deriving the pattern via
    /// detect_pattern so the memory-kind mapping exercises real detection.
    fn parsed(subject: &str, body: &str) -> ParsedCommit {
        let info = CommitInfo {
            hash: "abc123".to_string(),
            subject: subject.to_string(),
            body: body.to_string(),
            author_name: "Jane".to_string(),
            author_email: "jane@example.com".to_string(),
            author_date: "2024-01-01".to_string(),
        };
        let (pattern, confidence) = detect_pattern(&info);
        ParsedCommit {
            info,
            pattern,
            files_changed: vec![],
            confidence,
        }
    }

    #[test]
    fn bug_fix_maps_to_debug_context_with_extracted_problem_and_cause() {
        let pc = parsed(
            "fix: null pointer",
            "Problem: crash on empty input\nRoot cause: missing guard\nunrelated line",
        );
        match pc.to_memory_kind() {
            Some(MemoryKind::DebugContext {
                problem_description,
                root_cause,
                solution,
                ..
            }) => {
                assert_eq!(problem_description, "crash on empty input");
                assert_eq!(root_cause.as_deref(), Some("missing guard"));
                assert_eq!(solution, "fix: null pointer");
            }
            other => panic!("expected DebugContext, got {:?}", other),
        }
    }

    #[test]
    fn bug_fix_without_markers_falls_back_to_subject_problem() {
        let pc = parsed("fix: broken thing", "just some free-form notes");
        match pc.to_memory_kind() {
            Some(MemoryKind::DebugContext {
                problem_description,
                root_cause,
                ..
            }) => {
                assert_eq!(problem_description, "fix: broken thing");
                assert!(root_cause.is_none());
            }
            other => panic!("expected DebugContext, got {:?}", other),
        }
    }

    #[test]
    fn architectural_decision_maps_to_arch_kind() {
        let pc = parsed("arch: adopt event sourcing", "because it scales");
        match pc.to_memory_kind() {
            Some(MemoryKind::ArchitecturalDecision {
                decision,
                rationale,
                stakeholders,
                ..
            }) => {
                assert_eq!(decision, "arch: adopt event sourcing");
                assert_eq!(rationale, "because it scales");
                assert_eq!(stakeholders, vec!["Jane".to_string()]);
            }
            other => panic!("expected ArchitecturalDecision, got {:?}", other),
        }
    }

    #[test]
    fn feature_with_substantial_body_becomes_architectural_decision() {
        let body = "This adds a full plugin system so third parties can extend behavior.";
        assert!(body.len() > 50);
        let pc = parsed("feat: plugin system", body);
        assert!(matches!(
            pc.to_memory_kind(),
            Some(MemoryKind::ArchitecturalDecision { .. })
        ));
    }

    #[test]
    fn feature_without_substantial_body_creates_no_memory() {
        let pc = parsed("feat: small tweak", "short");
        assert!(pc.to_memory_kind().is_none());
    }

    #[test]
    fn breaking_change_maps_to_high_severity_known_issue_with_workaround() {
        let pc = parsed("breaking: drop v1 API", "Migration: switch to v2 endpoints");
        match pc.to_memory_kind() {
            Some(MemoryKind::KnownIssue {
                severity,
                workaround,
                ..
            }) => {
                assert_eq!(severity, codegraph_memory::IssueSeverity::High);
                assert_eq!(workaround.as_deref(), Some("switch to v2 endpoints"));
            }
            other => panic!("expected KnownIssue, got {:?}", other),
        }
    }

    #[test]
    fn deprecation_maps_to_medium_known_issue_prefixed_description() {
        let pc = parsed("deprecate: legacy client", "");
        match pc.to_memory_kind() {
            Some(MemoryKind::KnownIssue {
                description,
                severity,
                ..
            }) => {
                assert!(description.starts_with("Deprecated: "));
                assert_eq!(severity, codegraph_memory::IssueSeverity::Medium);
            }
            other => panic!("expected KnownIssue, got {:?}", other),
        }
    }

    #[test]
    fn revert_maps_to_medium_known_issue_without_workaround() {
        let pc = parsed("Revert 1a2b3c4d", "");
        match pc.to_memory_kind() {
            Some(MemoryKind::KnownIssue {
                description,
                severity,
                workaround,
                ..
            }) => {
                assert!(description.starts_with("Reverted: "));
                assert_eq!(severity, codegraph_memory::IssueSeverity::Medium);
                assert!(workaround.is_none());
            }
            other => panic!("expected KnownIssue, got {:?}", other),
        }
    }

    #[test]
    fn refactor_docs_test_and_other_produce_no_memory() {
        assert!(parsed("refactor: tidy up", "").to_memory_kind().is_none());
        assert!(parsed("docs: update readme", "").to_memory_kind().is_none());
        assert!(parsed("test: add coverage", "").to_memory_kind().is_none());
        assert!(parsed("wip", "").to_memory_kind().is_none());
    }
}
