// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Git command execution wrapper.

use super::GitMiningError;
use std::path::Path;
use std::process::Command;

/// Wrapper for executing git commands.
pub struct GitExecutor {
    repo_path: std::path::PathBuf,
}

impl GitExecutor {
    /// Create a new git executor for the given repository path.
    pub fn new(repo_path: &Path) -> Result<Self, GitMiningError> {
        // Verify git is available
        let output = Command::new("git").arg("--version").output()?;

        if !output.status.success() {
            return Err(GitMiningError::GitNotAvailable);
        }

        // Verify path is a git repository
        let output = Command::new("git")
            .current_dir(repo_path)
            .args(["rev-parse", "--git-dir"])
            .output()?;

        if !output.status.success() {
            return Err(GitMiningError::NotARepository(repo_path.to_path_buf()));
        }

        Ok(Self {
            repo_path: repo_path.to_path_buf(),
        })
    }

    /// Get commit log with custom format.
    ///
    /// Format placeholders:
    /// - %H: commit hash
    /// - %s: subject
    /// - %b: body
    /// - %an: author name
    /// - %ae: author email
    /// - %ai: author date (ISO format)
    pub fn log(
        &self,
        format: &str,
        limit: Option<usize>,
        path_filter: Option<&Path>,
    ) -> Result<String, GitMiningError> {
        let mut cmd = Command::new("git");
        cmd.current_dir(&self.repo_path);
        cmd.args(["log", &format!("--format={}", format)]);

        if let Some(n) = limit {
            cmd.arg(format!("-n{}", n));
        }

        cmd.arg("--");

        if let Some(path) = path_filter {
            cmd.arg(path);
        }

        let output = cmd.output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitMiningError::CommandFailed(stderr.to_string()));
        }

        Ok(String::from_utf8(output.stdout)?)
    }

    /// Get commits matching a grep pattern in commit messages.
    pub fn log_grep(
        &self,
        pattern: &str,
        format: &str,
        limit: Option<usize>,
    ) -> Result<String, GitMiningError> {
        let mut cmd = Command::new("git");
        cmd.current_dir(&self.repo_path);
        cmd.args([
            "log",
            &format!("--format={}", format),
            "--all",
            "-i", // case insensitive
            &format!("--grep={}", pattern),
        ]);

        if let Some(n) = limit {
            cmd.arg(format!("-n{}", n));
        }

        let output = cmd.output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitMiningError::CommandFailed(stderr.to_string()));
        }

        Ok(String::from_utf8(output.stdout)?)
    }

    /// Get the files changed in a specific commit.
    pub fn show_files(&self, commit_hash: &str) -> Result<Vec<String>, GitMiningError> {
        let output = Command::new("git")
            .current_dir(&self.repo_path)
            .args(["show", "--name-only", "--format=", commit_hash])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitMiningError::CommandFailed(stderr.to_string()));
        }

        let stdout = String::from_utf8(output.stdout)?;
        Ok(stdout
            .lines()
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect())
    }

    /// Get the diff statistics for a commit.
    pub fn show_stat(&self, commit_hash: &str) -> Result<String, GitMiningError> {
        let output = Command::new("git")
            .current_dir(&self.repo_path)
            .args(["show", "--stat", "--format=", commit_hash])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitMiningError::CommandFailed(stderr.to_string()));
        }

        Ok(String::from_utf8(output.stdout)?)
    }

    /// Get the full commit message for a specific commit.
    pub fn show_message(&self, commit_hash: &str) -> Result<String, GitMiningError> {
        let output = Command::new("git")
            .current_dir(&self.repo_path)
            .args(["show", "-s", "--format=%B", commit_hash])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitMiningError::CommandFailed(stderr.to_string()));
        }

        Ok(String::from_utf8(output.stdout)?.trim().to_string())
    }

    /// Get git blame for a specific file.
    pub fn blame(
        &self,
        path: &Path,
        line_range: Option<(u32, u32)>,
    ) -> Result<String, GitMiningError> {
        let mut cmd = Command::new("git");
        cmd.current_dir(&self.repo_path);
        cmd.args(["blame", "--porcelain"]);

        if let Some((start, end)) = line_range {
            cmd.arg(format!("-L{},{}", start, end));
        }

        cmd.arg(path);

        let output = cmd.output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitMiningError::CommandFailed(stderr.to_string()));
        }

        Ok(String::from_utf8(output.stdout)?)
    }

    /// Get the current branch name. Returns `"HEAD"` if in detached HEAD state.
    pub fn current_branch(&self) -> Result<String, GitMiningError> {
        let output = Command::new("git")
            .current_dir(&self.repo_path)
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitMiningError::CommandFailed(stderr.to_string()));
        }

        Ok(String::from_utf8(output.stdout)?.trim().to_string())
    }

    /// Get the current HEAD commit hash. Works in both normal and detached HEAD states.
    pub fn head_commit(&self) -> Result<String, GitMiningError> {
        let output = Command::new("git")
            .current_dir(&self.repo_path)
            .args(["rev-parse", "HEAD"])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitMiningError::CommandFailed(stderr.to_string()));
        }

        Ok(String::from_utf8(output.stdout)?.trim().to_string())
    }

    /// Get files changed between two refs with their status.
    ///
    /// Returns `Vec<(status, path)>` where status is `'A'` (added), `'M'` (modified),
    /// `'D'` (deleted), or `'R'` (renamed).
    pub fn diff_name_status(
        &self,
        from_ref: &str,
        to_ref: &str,
    ) -> Result<Vec<(char, std::path::PathBuf)>, GitMiningError> {
        let output = Command::new("git")
            .current_dir(&self.repo_path)
            .args([
                "diff",
                "--name-status",
                &format!("{}..{}", from_ref, to_ref),
            ])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitMiningError::CommandFailed(stderr.to_string()));
        }

        let stdout = String::from_utf8(output.stdout)?;
        let mut results = Vec::new();

        for line in stdout.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            // Format: "M\tpath/to/file" or "R100\told\tnew"
            let mut parts = line.splitn(2, '\t');
            if let (Some(status_str), Some(path_str)) = (parts.next(), parts.next()) {
                let status = status_str.chars().next().unwrap_or('M');
                // For renames (R100\told\tnew), take the new path
                let path = if status == 'R' {
                    path_str.split('\t').next_back().unwrap_or(path_str)
                } else {
                    path_str
                };
                results.push((status, std::path::PathBuf::from(path)));
            }
        }

        Ok(results)
    }

    /// Resolve the actual `.git` directory path (handles worktrees where `.git` is a file).
    pub fn git_dir(&self) -> Result<std::path::PathBuf, GitMiningError> {
        let output = Command::new("git")
            .current_dir(&self.repo_path)
            .args(["rev-parse", "--git-dir"])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitMiningError::CommandFailed(stderr.to_string()));
        }

        let git_dir = String::from_utf8(output.stdout)?.trim().to_string();
        let path = std::path::PathBuf::from(&git_dir);

        // If relative, resolve against repo_path
        if path.is_relative() {
            Ok(self.repo_path.join(path))
        } else {
            Ok(path)
        }
    }

    /// Get repository root path.
    pub fn repo_path(&self) -> &Path {
        &self.repo_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;
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

    /// Build a temp repo with two linear commits and return the executor plus
    /// the two commit hashes (oldest first).
    fn init_repo() -> (TempDir, GitExecutor, String, String) {
        let dir = TempDir::new().unwrap();
        let path = dir.path();
        git(path, &["init", "-q"]);
        git(path, &["config", "user.name", "Test User"]);
        git(path, &["config", "user.email", "test@example.com"]);
        git(path, &["config", "commit.gpgsign", "false"]);

        fs::write(path.join("a.txt"), "hello\n").unwrap();
        git(path, &["add", "a.txt"]);
        git(path, &["commit", "-q", "-m", "feat: add a"]);
        let first = git(path, &["rev-parse", "HEAD"]).trim().to_string();

        fs::write(path.join("a.txt"), "hello world\n").unwrap();
        fs::write(path.join("b.txt"), "second\n").unwrap();
        git(path, &["add", "a.txt", "b.txt"]);
        git(
            path,
            &["commit", "-q", "-m", "fix: bug in a\n\nDetailed body here."],
        );
        let second = git(path, &["rev-parse", "HEAD"]).trim().to_string();

        // Normalize branch name so current_branch is deterministic.
        git(path, &["branch", "-M", "main"]);

        let executor = GitExecutor::new(path).unwrap();
        (dir, executor, first, second)
    }

    #[test]
    fn test_git_executor_creation() {
        // This test only works if run from within a git repository
        let current_dir = env::current_dir().unwrap();
        let result = GitExecutor::new(&current_dir);
        // May or may not succeed depending on where tests are run
        if let Ok(executor) = result {
            assert!(executor.repo_path().exists());
        }
    }

    #[test]
    fn test_new_rejects_non_repository() {
        let dir = TempDir::new().unwrap();
        let result = GitExecutor::new(dir.path());
        assert!(matches!(result, Err(GitMiningError::NotARepository(_))));
    }

    #[test]
    fn test_log_returns_all_subjects() {
        let (_dir, executor, _first, _second) = init_repo();
        let out = executor.log("%s", None, None).unwrap();
        assert!(out.contains("feat: add a"));
        assert!(out.contains("fix: bug in a"));
    }

    #[test]
    fn test_log_limit_returns_only_newest() {
        let (_dir, executor, _first, _second) = init_repo();
        let out = executor.log("%s", Some(1), None).unwrap();
        assert!(out.contains("fix: bug in a"));
        assert!(!out.contains("feat: add a"));
    }

    #[test]
    fn test_log_path_filter_restricts_to_touching_commits() {
        let (_dir, executor, _first, _second) = init_repo();
        // b.txt only exists in the second commit.
        let out = executor.log("%s", None, Some(Path::new("b.txt"))).unwrap();
        assert!(out.contains("fix: bug in a"));
        assert!(!out.contains("feat: add a"));
    }

    #[test]
    fn test_log_grep_is_case_insensitive() {
        let (_dir, executor, _first, _second) = init_repo();
        let hit = executor.log_grep("FIX", "%s", None).unwrap();
        assert!(hit.contains("fix: bug in a"));
        assert!(!hit.contains("feat: add a"));

        let miss = executor.log_grep("nonexistent-token", "%s", None).unwrap();
        assert!(miss.trim().is_empty());
    }

    #[test]
    fn test_show_files_lists_changed_paths() {
        let (_dir, executor, first, second) = init_repo();

        let first_files = executor.show_files(&first).unwrap();
        assert_eq!(first_files, vec!["a.txt".to_string()]);

        let second_files = executor.show_files(&second).unwrap();
        assert!(second_files.contains(&"a.txt".to_string()));
        assert!(second_files.contains(&"b.txt".to_string()));
        assert_eq!(second_files.len(), 2);
    }

    #[test]
    fn test_show_files_bad_hash_errors() {
        let (_dir, executor, _first, _second) = init_repo();
        let result = executor.show_files("deadbeefdeadbeef");
        assert!(matches!(result, Err(GitMiningError::CommandFailed(_))));
    }

    #[test]
    fn test_show_stat_includes_file_names() {
        let (_dir, executor, _first, second) = init_repo();
        let stat = executor.show_stat(&second).unwrap();
        assert!(stat.contains("a.txt"));
        assert!(stat.contains("b.txt"));
    }

    #[test]
    fn test_show_message_returns_full_body() {
        let (_dir, executor, _first, second) = init_repo();
        let msg = executor.show_message(&second).unwrap();
        assert!(msg.starts_with("fix: bug in a"));
        assert!(msg.contains("Detailed body here."));
    }

    #[test]
    fn test_current_branch_is_main() {
        let (_dir, executor, _first, _second) = init_repo();
        assert_eq!(executor.current_branch().unwrap(), "main");
    }

    #[test]
    fn test_head_commit_matches_second() {
        let (_dir, executor, _first, second) = init_repo();
        let head = executor.head_commit().unwrap();
        assert_eq!(head, second);
        assert_eq!(head.len(), 40);
    }

    #[test]
    fn test_diff_name_status_reports_add_and_modify() {
        let (_dir, executor, first, second) = init_repo();
        let changes = executor.diff_name_status(&first, &second).unwrap();
        assert!(changes.contains(&('M', std::path::PathBuf::from("a.txt"))));
        assert!(changes.contains(&('A', std::path::PathBuf::from("b.txt"))));
        assert_eq!(changes.len(), 2);
    }

    #[test]
    fn test_git_dir_resolves_to_existing_dot_git() {
        let (_dir, executor, _first, _second) = init_repo();
        let git_dir = executor.git_dir().unwrap();
        assert!(git_dir.ends_with(".git"));
        assert!(git_dir.exists());
    }

    #[test]
    fn test_blame_includes_author() {
        let (_dir, executor, _first, _second) = init_repo();
        let blame = executor.blame(Path::new("a.txt"), None).unwrap();
        assert!(blame.contains("Test User"));

        // Line-range restricted blame still succeeds for the single line.
        let ranged = executor.blame(Path::new("a.txt"), Some((1, 1))).unwrap();
        assert!(ranged.contains("Test User"));
    }

    #[test]
    fn test_repo_path_matches_construction() {
        let (dir, executor, _first, _second) = init_repo();
        assert_eq!(executor.repo_path(), dir.path());
    }
}
