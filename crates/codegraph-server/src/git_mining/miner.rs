// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Git history miner for extracting memories from commits.

use super::{
    executor::GitExecutor,
    parser::{self, CommitInfo, CommitPattern, ParsedCommit, LOG_FORMAT},
    GitMiningError,
};
use crate::memory::MemoryManager;
use codegraph::CodeGraph;
use codegraph_memory::{LinkedNodeType, MemoryNode, MemorySource};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Configuration for git mining operations.
#[derive(Debug, Clone)]
pub struct MiningConfig {
    /// Maximum number of commits to process.
    pub max_commits: usize,
    /// Minimum confidence score to create a memory.
    pub min_confidence: f32,
    /// Whether to mine bug fixes.
    pub mine_bug_fixes: bool,
    /// Whether to mine architectural decisions.
    pub mine_arch_decisions: bool,
    /// Whether to mine breaking changes.
    pub mine_breaking_changes: bool,
    /// Whether to mine reverts.
    pub mine_reverts: bool,
    /// Whether to mine features as architectural decisions.
    pub mine_features: bool,
    /// Whether to mine deprecations as known issues.
    pub mine_deprecations: bool,
}

impl Default for MiningConfig {
    fn default() -> Self {
        Self {
            max_commits: 500,
            min_confidence: 0.7,
            mine_bug_fixes: true,
            mine_arch_decisions: true,
            mine_breaking_changes: true,
            mine_reverts: true,
            mine_features: true,
            mine_deprecations: true,
        }
    }
}

/// Result of a mining operation.
#[derive(Debug, Default)]
pub struct MiningResult {
    /// Number of commits processed.
    pub commits_processed: usize,
    /// Number of memories created.
    pub memories_created: usize,
    /// Number of commits skipped due to low confidence.
    pub commits_skipped: usize,
    /// IDs of created memories.
    pub memory_ids: Vec<String>,
    /// Errors encountered (non-fatal).
    pub warnings: Vec<String>,
}

/// Data about file churn for hotspot detection.
#[derive(Debug)]
struct FileChurnData {
    path: String,
    change_count: usize,
    unique_commits: std::collections::HashSet<String>,
    recent_changes: Vec<String>,
}

/// A code hotspot (high-churn file).
#[derive(Debug, Clone)]
pub struct ChurnHotspot {
    pub file_path: String,
    pub change_count: usize,
    pub unique_commits: usize,
    pub recent_changes: Vec<String>,
}

/// File coupling information (co-change pattern).
#[derive(Debug, Clone)]
pub struct FileCoupling {
    pub file_a: String,
    pub file_b: String,
    pub co_change_count: usize,
    pub total_changes: usize,
    pub coupling_strength: f32,
}

/// Git history miner that extracts memories from commit history.
pub struct GitMiner {
    executor: GitExecutor,
}

impl GitMiner {
    /// Create a new git miner for the given repository.
    pub fn new(repo_path: &Path) -> Result<Self, GitMiningError> {
        let executor = GitExecutor::new(repo_path)?;
        Ok(Self { executor })
    }

    /// Mine repository history and create memories.
    pub async fn mine_repository(
        &self,
        memory_manager: &MemoryManager,
        graph: &Arc<RwLock<CodeGraph>>,
        config: &MiningConfig,
    ) -> Result<MiningResult, GitMiningError> {
        let mut result = MiningResult::default();

        // Collect already-mined commit hashes to avoid duplicates
        let already_mined = Self::collect_mined_commits(memory_manager).await;

        // Collect commits matching our patterns
        let commits = self.collect_relevant_commits(config)?;
        result.commits_processed = commits.len();

        tracing::info!(
            "Found {} relevant commits to process ({} already mined)",
            commits.len(),
            already_mined.len()
        );

        // Process each commit
        for commit in commits {
            match self
                .process_commit(&commit, memory_manager, graph, config, &already_mined)
                .await
            {
                Ok(Some(memory_id)) => {
                    result.memories_created += 1;
                    result.memory_ids.push(memory_id);
                }
                Ok(None) => {
                    result.commits_skipped += 1;
                }
                Err(e) => {
                    result.warnings.push(format!(
                        "Failed to process commit {}: {}",
                        &commit.hash[..7],
                        e
                    ));
                }
            }
        }

        tracing::info!(
            "Mining complete: {} memories created from {} commits ({} skipped)",
            result.memories_created,
            result.commits_processed,
            result.commits_skipped
        );

        Ok(result)
    }

    /// Mine history for a specific file.
    pub async fn mine_file(
        &self,
        file_path: &Path,
        memory_manager: &MemoryManager,
        graph: &Arc<RwLock<CodeGraph>>,
        config: &MiningConfig,
    ) -> Result<MiningResult, GitMiningError> {
        let mut result = MiningResult::default();

        // Collect already-mined commit hashes to avoid duplicates
        let already_mined = Self::collect_mined_commits(memory_manager).await;

        // Get commits that touched this file
        let output = self
            .executor
            .log(LOG_FORMAT, Some(config.max_commits), Some(file_path))?;
        let commits = parser::parse_log_output(&output)?;
        result.commits_processed = commits.len();

        tracing::info!(
            "Found {} commits for file {} ({} already mined)",
            commits.len(),
            file_path.display(),
            already_mined.len()
        );

        for commit in commits {
            match self
                .process_commit(&commit, memory_manager, graph, config, &already_mined)
                .await
            {
                Ok(Some(memory_id)) => {
                    result.memories_created += 1;
                    result.memory_ids.push(memory_id);
                }
                Ok(None) => {
                    result.commits_skipped += 1;
                }
                Err(e) => {
                    result.warnings.push(format!(
                        "Failed to process commit {}: {}",
                        &commit.hash[..7],
                        e
                    ));
                }
            }
        }

        Ok(result)
    }

    /// Collect commit hashes that have already been mined into memories.
    async fn collect_mined_commits(
        memory_manager: &MemoryManager,
    ) -> std::collections::HashSet<String> {
        let memories = memory_manager
            .get_all_memories(false) // include invalidated too — still counts as mined
            .await
            .unwrap_or_default();

        memories
            .into_iter()
            .filter_map(|m| {
                if let MemorySource::GitHistory { commit_hash } = m.source {
                    Some(commit_hash)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Collect recent commits for mining.
    ///
    /// Fetches the last `max_commits` commits directly (no grep filter),
    /// so commits with non-conventional messages are still processed.
    fn collect_relevant_commits(
        &self,
        config: &MiningConfig,
    ) -> Result<Vec<CommitInfo>, GitMiningError> {
        let output = self
            .executor
            .log(LOG_FORMAT, Some(config.max_commits), None)?;
        parser::parse_log_output(&output)
    }

    /// Process a single commit and optionally create a memory.
    async fn process_commit(
        &self,
        commit: &CommitInfo,
        memory_manager: &MemoryManager,
        graph: &Arc<RwLock<CodeGraph>>,
        config: &MiningConfig,
        already_mined: &std::collections::HashSet<String>,
    ) -> Result<Option<String>, GitMiningError> {
        // Skip commits that have already been mined
        if already_mined.contains(&commit.hash) {
            return Ok(None);
        }

        // Detect pattern
        let (pattern, confidence) = parser::detect_pattern(commit);

        // Check if we should process this pattern
        if !self.should_process_pattern(&pattern, config) {
            return Ok(None);
        }

        // Check confidence threshold
        if confidence < config.min_confidence {
            return Ok(None);
        }

        // Get files changed in this commit
        let files_changed = self.executor.show_files(&commit.hash)?;

        // Create parsed commit
        let parsed = ParsedCommit {
            info: commit.clone(),
            pattern: pattern.clone(),
            files_changed: files_changed.clone(),
            confidence,
        };

        // Get memory kind
        let memory_kind = match parsed.to_memory_kind() {
            Some(kind) => kind,
            None => return Ok(None),
        };

        // Find code nodes to link to
        let code_links = self.find_code_links(&files_changed, graph).await;

        // Build the memory
        let mut builder = MemoryNode::builder()
            .kind(memory_kind)
            .title(format!("[Git] {}", commit.subject))
            .content(format!(
                "Commit: {}\nAuthor: {} <{}>\nDate: {}\n\n{}",
                commit.hash,
                commit.author_name,
                commit.author_email,
                commit.author_date,
                if commit.body.is_empty() {
                    &commit.subject
                } else {
                    &commit.body
                }
            ))
            .from_git(&commit.hash)
            .at_commit(&commit.hash)
            .tag("git-mined")
            .tag("auto")
            .confidence(confidence);

        // Add pattern-specific tag
        builder = match pattern {
            CommitPattern::BugFix { .. } => builder.tag("bug-fix"),
            CommitPattern::ArchitecturalDecision => builder.tag("architecture"),
            CommitPattern::BreakingChange => builder.tag("breaking-change"),
            CommitPattern::Revert { .. } => builder.tag("revert"),
            _ => builder,
        };

        // Add code links
        for (node_id, node_type) in code_links {
            builder = builder.link_to_code(&node_id, node_type);
        }

        let memory = builder
            .build()
            .map_err(|e| GitMiningError::MemoryError(format!("Failed to build memory: {}", e)))?;

        // Store the memory
        let id = memory_manager.put(memory).await?;

        tracing::debug!(
            "Created memory {} from commit {} ({})",
            id,
            &commit.hash[..7],
            commit.subject
        );

        Ok(Some(id))
    }

    /// Detect code hotspots (high-churn files) in repository history.
    pub async fn detect_hotspots(
        &self,
        threshold: usize,
    ) -> Result<Vec<ChurnHotspot>, GitMiningError> {
        // Get all commits
        let output = self.executor.log(
            parser::LOG_FORMAT,
            None, // No limit
            None, // All files
        )?;
        let commits = parser::parse_log_output(&output)?;

        // Track file changes
        let mut file_changes: std::collections::HashMap<String, FileChurnData> =
            std::collections::HashMap::new();

        for commit in &commits {
            let files = self.executor.show_files(&commit.hash)?;
            for file in files {
                let data = file_changes.entry(file.clone()).or_insert(FileChurnData {
                    path: file.clone(),
                    change_count: 0,
                    unique_commits: std::collections::HashSet::new(),
                    recent_changes: Vec::new(),
                });
                data.change_count += 1;
                data.unique_commits.insert(commit.hash.clone());
                if data.recent_changes.len() < 5 {
                    data.recent_changes.push(commit.subject.clone());
                }
            }
        }

        // Filter and convert to hotspots
        let mut hotspots: Vec<ChurnHotspot> = file_changes
            .into_iter()
            .filter(|(_, data)| data.change_count >= threshold)
            .map(|(_, data)| ChurnHotspot {
                file_path: data.path,
                change_count: data.change_count,
                unique_commits: data.unique_commits.len(),
                recent_changes: data.recent_changes,
            })
            .collect();

        // Sort by change count descending
        hotspots.sort_by(|a, b| b.change_count.cmp(&a.change_count));

        Ok(hotspots)
    }

    /// Detect file coupling (files that frequently change together).
    pub async fn detect_coupling(
        &self,
        min_coupling: f32,
    ) -> Result<Vec<FileCoupling>, GitMiningError> {
        // Get all commits
        let output = self.executor.log(
            parser::LOG_FORMAT,
            None, // No limit
            None, // All files
        )?;
        let commits = parser::parse_log_output(&output)?;

        // Track co-changes
        let mut co_changes: std::collections::HashMap<(String, String), usize> =
            std::collections::HashMap::new();
        let mut file_changes: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();

        for commit in &commits {
            let files = self.executor.show_files(&commit.hash)?;

            // Track individual file changes
            for file in &files {
                *file_changes.entry(file.clone()).or_insert(0) += 1;
            }

            // Track co-changes (pairs of files changed together)
            for i in 0..files.len() {
                for j in (i + 1)..files.len() {
                    let pair = if files[i] < files[j] {
                        (files[i].clone(), files[j].clone())
                    } else {
                        (files[j].clone(), files[i].clone())
                    };
                    *co_changes.entry(pair).or_insert(0) += 1;
                }
            }
        }

        // Calculate coupling strength
        let mut couplings = Vec::new();
        for ((file_a, file_b), co_count) in co_changes {
            let changes_a = *file_changes.get(&file_a).unwrap_or(&1) as f32;
            let changes_b = *file_changes.get(&file_b).unwrap_or(&1) as f32;
            let co_count = co_count as f32;

            // Coupling strength = co-changes / min(changes_a, changes_b)
            let strength = co_count / changes_a.min(changes_b);

            if strength >= min_coupling {
                couplings.push(FileCoupling {
                    file_a: file_a.clone(),
                    file_b: file_b.clone(),
                    co_change_count: co_count as usize,
                    total_changes: (changes_a.max(changes_b)) as usize,
                    coupling_strength: strength,
                });
            }
        }

        // Sort by coupling strength descending
        couplings.sort_by(|a, b| {
            b.coupling_strength
                .partial_cmp(&a.coupling_strength)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(couplings)
    }

    /// Check if we should process a given pattern based on config.
    fn should_process_pattern(&self, pattern: &CommitPattern, config: &MiningConfig) -> bool {
        match pattern {
            CommitPattern::BugFix { .. } => config.mine_bug_fixes,
            CommitPattern::ArchitecturalDecision => config.mine_arch_decisions,
            CommitPattern::BreakingChange => config.mine_breaking_changes,
            CommitPattern::Revert { .. } => config.mine_reverts,
            CommitPattern::Feature => config.mine_features,
            CommitPattern::Deprecation => config.mine_deprecations,
            _ => false, // Don't create memories for refactors, docs, tests, other
        }
    }

    /// Find code graph nodes to link memories to based on changed files.
    async fn find_code_links(
        &self,
        files: &[String],
        graph: &Arc<RwLock<CodeGraph>>,
    ) -> Vec<(String, LinkedNodeType)> {
        let mut links = Vec::new();
        let graph = graph.read().await;

        for file in files {
            // Query for nodes in this file
            let repo_path = self.executor.repo_path();
            let full_path = repo_path.join(file);
            let path_str = full_path.to_string_lossy().to_string();

            if let Ok(nodes) = graph.query().property("path", path_str).execute() {
                for node_id in nodes.iter().take(5) {
                    // Limit links per file
                    // Determine node type from the graph
                    if let Ok(node) = graph.get_node(*node_id) {
                        let node_type = match node.node_type {
                            codegraph::NodeType::Function => LinkedNodeType::Function,
                            codegraph::NodeType::Class => LinkedNodeType::Class,
                            codegraph::NodeType::Module => LinkedNodeType::Module,
                            codegraph::NodeType::Interface => LinkedNodeType::Interface,
                            _ => LinkedNodeType::File,
                        };
                        links.push((node_id.to_string(), node_type));
                    }
                }
            }
        }

        links
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;
    use tempfile::TempDir;

    /// Run a git command in `dir`, panicking on failure.
    fn git(dir: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .expect("git command should run");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    /// Commit the current index with `subject` and return nothing; identity and
    /// gpgsign are configured by `init_repo`.
    fn commit(dir: &Path, subject: &str) {
        git(dir, &["commit", "-q", "-m", subject]);
    }

    /// Build a temp repo whose churn/coupling profile is deterministic:
    /// - `a.txt` is touched by 6 commits (a hotspot),
    /// - `b.txt` by 3 commits (co-changing with `a.txt` each time),
    /// - `c.txt` by 1 commit (standalone).
    ///
    /// Returns the temp dir (kept alive by the caller) and a `GitMiner`.
    fn init_repo() -> (TempDir, GitMiner) {
        let dir = TempDir::new().unwrap();
        let path = dir.path();
        git(path, &["init", "-q"]);
        git(path, &["config", "user.name", "Test User"]);
        git(path, &["config", "user.email", "test@example.com"]);
        git(path, &["config", "commit.gpgsign", "false"]);

        // Commit 1: a + b together.
        fs::write(path.join("a.txt"), "a1\n").unwrap();
        fs::write(path.join("b.txt"), "b1\n").unwrap();
        git(path, &["add", "a.txt", "b.txt"]);
        commit(path, "feat: add a and b");

        // Commits 2 and 3: a + b together.
        for n in 2..=3 {
            fs::write(path.join("a.txt"), format!("a{}\n", n)).unwrap();
            fs::write(path.join("b.txt"), format!("b{}\n", n)).unwrap();
            git(path, &["add", "a.txt", "b.txt"]);
            commit(path, &format!("fix: touch a and b #{}", n));
        }

        // Commits 4, 5, 6: a alone.
        for n in 4..=6 {
            fs::write(path.join("a.txt"), format!("a{}\n", n)).unwrap();
            git(path, &["add", "a.txt"]);
            commit(path, &format!("refactor: touch a #{}", n));
        }

        // Commit 7: c alone.
        fs::write(path.join("c.txt"), "c1\n").unwrap();
        git(path, &["add", "c.txt"]);
        commit(path, "feat: add c");

        git(path, &["branch", "-M", "main"]);

        let miner = GitMiner::new(path).unwrap();
        (dir, miner)
    }

    #[test]
    fn test_mining_config_default() {
        let config = MiningConfig::default();
        assert!(config.mine_bug_fixes);
        assert!(config.mine_arch_decisions);
        assert!(config.mine_breaking_changes);
        assert_eq!(config.max_commits, 500);
        assert!(config.min_confidence >= 0.0 && config.min_confidence <= 1.0);
    }

    #[test]
    fn test_mining_result_default() {
        let result = MiningResult::default();
        assert_eq!(result.commits_processed, 0);
        assert_eq!(result.memories_created, 0);
        assert_eq!(result.commits_skipped, 0);
        assert!(result.memory_ids.is_empty());
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn test_new_rejects_non_repository() {
        let dir = TempDir::new().unwrap();
        let result = GitMiner::new(dir.path());
        assert!(matches!(result, Err(GitMiningError::NotARepository(_))));
    }

    #[test]
    fn test_collect_relevant_commits_returns_all() {
        let (_dir, miner) = init_repo();
        let config = MiningConfig::default();
        let commits = miner.collect_relevant_commits(&config).unwrap();
        assert_eq!(commits.len(), 7);
        // Newest commit is first in git log order.
        assert_eq!(commits[0].subject, "feat: add c");
    }

    #[test]
    fn test_collect_relevant_commits_respects_max() {
        let (_dir, miner) = init_repo();
        let config = MiningConfig {
            max_commits: 3,
            ..MiningConfig::default()
        };
        let commits = miner.collect_relevant_commits(&config).unwrap();
        assert_eq!(commits.len(), 3);
    }

    #[tokio::test]
    async fn test_detect_hotspots_threshold_filters() {
        let (_dir, miner) = init_repo();
        // Threshold 3 keeps a.txt (6) and b.txt (3); c.txt (1) is excluded.
        let hotspots = miner.detect_hotspots(3).await.unwrap();
        let paths: Vec<&str> = hotspots.iter().map(|h| h.file_path.as_str()).collect();
        assert!(paths.contains(&"a.txt"));
        assert!(paths.contains(&"b.txt"));
        assert!(!paths.contains(&"c.txt"));
    }

    #[tokio::test]
    async fn test_detect_hotspots_sorted_descending() {
        let (_dir, miner) = init_repo();
        let hotspots = miner.detect_hotspots(1).await.unwrap();
        // a.txt has the most changes and must sort first.
        assert_eq!(hotspots[0].file_path, "a.txt");
        assert_eq!(hotspots[0].change_count, 6);
        for pair in hotspots.windows(2) {
            assert!(pair[0].change_count >= pair[1].change_count);
        }
    }

    #[tokio::test]
    async fn test_detect_hotspots_unique_commits_count() {
        let (_dir, miner) = init_repo();
        let hotspots = miner.detect_hotspots(1).await.unwrap();
        let a = hotspots.iter().find(|h| h.file_path == "a.txt").unwrap();
        assert_eq!(a.change_count, 6);
        assert_eq!(a.unique_commits, 6);
        let b = hotspots.iter().find(|h| h.file_path == "b.txt").unwrap();
        assert_eq!(b.change_count, 3);
        assert_eq!(b.unique_commits, 3);
    }

    #[tokio::test]
    async fn test_detect_hotspots_recent_changes_capped_at_5() {
        let (_dir, miner) = init_repo();
        let hotspots = miner.detect_hotspots(1).await.unwrap();
        let a = hotspots.iter().find(|h| h.file_path == "a.txt").unwrap();
        // a.txt changed in 6 commits but recent_changes is capped at 5.
        assert_eq!(a.recent_changes.len(), 5);
        let c = hotspots.iter().find(|h| h.file_path == "c.txt").unwrap();
        assert_eq!(c.recent_changes, vec!["feat: add c".to_string()]);
    }

    #[tokio::test]
    async fn test_detect_hotspots_high_threshold_excludes_all_but_top() {
        let (_dir, miner) = init_repo();
        let hotspots = miner.detect_hotspots(6).await.unwrap();
        assert_eq!(hotspots.len(), 1);
        assert_eq!(hotspots[0].file_path, "a.txt");
    }

    #[tokio::test]
    async fn test_detect_hotspots_empty_when_threshold_exceeds_max() {
        let (_dir, miner) = init_repo();
        let hotspots = miner.detect_hotspots(100).await.unwrap();
        assert!(hotspots.is_empty());
    }

    #[tokio::test]
    async fn test_detect_coupling_pair_detected() {
        let (_dir, miner) = init_repo();
        let couplings = miner.detect_coupling(0.0).await.unwrap();
        // Only a.txt and b.txt ever change together (3 times).
        let ab = couplings
            .iter()
            .find(|c| c.file_a == "a.txt" && c.file_b == "b.txt")
            .expect("a/b coupling present");
        assert_eq!(ab.co_change_count, 3);
        // Pairs are ordered lexicographically, so file_a < file_b.
        assert!(ab.file_a < ab.file_b);
    }

    #[tokio::test]
    async fn test_detect_coupling_strength_computed() {
        let (_dir, miner) = init_repo();
        let couplings = miner.detect_coupling(0.0).await.unwrap();
        let ab = couplings
            .iter()
            .find(|c| c.file_a == "a.txt" && c.file_b == "b.txt")
            .unwrap();
        // co(3) / min(changes_a=6, changes_b=3) = 1.0
        assert!((ab.coupling_strength - 1.0).abs() < f32::EPSILON);
        // total_changes is the max of the two files' change counts.
        assert_eq!(ab.total_changes, 6);
    }

    #[tokio::test]
    async fn test_detect_coupling_min_filter_excludes() {
        let (_dir, miner) = init_repo();
        // Strength is 1.0, so a threshold above it drops the pair.
        let couplings = miner.detect_coupling(1.1).await.unwrap();
        assert!(couplings.is_empty());
    }

    #[tokio::test]
    async fn test_detect_coupling_only_ab_pair() {
        let (_dir, miner) = init_repo();
        let couplings = miner.detect_coupling(0.0).await.unwrap();
        // c.txt only ever changes alone, so it forms no pair.
        assert_eq!(couplings.len(), 1);
        assert!(!couplings
            .iter()
            .any(|c| c.file_a == "c.txt" || c.file_b == "c.txt"));
    }

    #[test]
    fn test_should_process_pattern_respects_bug_fix_flag() {
        let (_dir, miner) = init_repo();
        let pattern = CommitPattern::BugFix { issue_ref: None };
        let mut config = MiningConfig::default();
        assert!(miner.should_process_pattern(&pattern, &config));
        config.mine_bug_fixes = false;
        assert!(!miner.should_process_pattern(&pattern, &config));
    }

    #[test]
    fn test_should_process_pattern_each_toggled_kind() {
        let (_dir, miner) = init_repo();
        let cases: Vec<(CommitPattern, fn(&mut MiningConfig))> = vec![
            (CommitPattern::ArchitecturalDecision, |c| {
                c.mine_arch_decisions = false
            }),
            (CommitPattern::BreakingChange, |c| {
                c.mine_breaking_changes = false
            }),
            (CommitPattern::Feature, |c| c.mine_features = false),
            (CommitPattern::Deprecation, |c| c.mine_deprecations = false),
            (
                CommitPattern::Revert {
                    reverted_hash: None,
                },
                |c| c.mine_reverts = false,
            ),
        ];
        for (pattern, disable) in cases {
            let mut config = MiningConfig::default();
            assert!(
                miner.should_process_pattern(&pattern, &config),
                "{:?} should process when enabled",
                pattern
            );
            disable(&mut config);
            assert!(
                !miner.should_process_pattern(&pattern, &config),
                "{:?} should be skipped when disabled",
                pattern
            );
        }
    }

    #[test]
    fn test_should_process_pattern_non_memory_kinds_never_process() {
        let (_dir, miner) = init_repo();
        let config = MiningConfig::default();
        for pattern in [
            CommitPattern::Refactor,
            CommitPattern::Documentation,
            CommitPattern::Test,
            CommitPattern::Other,
        ] {
            assert!(
                !miner.should_process_pattern(&pattern, &config),
                "{:?} must never create a memory",
                pattern
            );
        }
    }
}
