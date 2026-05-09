// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Reporting layer. Builds a single-screen summary from per-case
//! results: counts per family, language × family coverage matrix,
//! failure list with case YAML paths, and drift detection
//! (regression / fix / new / removed) versus the previous run.
//!
//! Drift state is persisted to
//! `<repo>/target/codegraph-harness/coverage-latest.yml` after every
//! run and re-read on the next invocation. Delete the file to reset.

use crate::profiles::{family_of, Family};
use crate::runner::CaseResult;
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// One row per case, the fully-resolved data needed for reporting.
pub struct CaseRecord {
    pub id: String,
    pub tool: String,
    pub family: Family,
    pub language: String,
    pub passed: bool,
    pub source_path: PathBuf,
    pub duration: Duration,
    pub error: Option<String>,
    pub diff: String,
}

/// Top-level run report.
pub struct Report {
    pub records: Vec<CaseRecord>,
    pub total_duration: Duration,
}

impl Report {
    pub fn new(records: Vec<CaseRecord>, total_duration: Duration) -> Self {
        Self { records, total_duration }
    }

    pub fn passed(&self) -> usize {
        self.records.iter().filter(|r| r.passed).count()
    }

    pub fn failed(&self) -> usize {
        self.records.iter().filter(|r| !r.passed).count()
    }

    pub fn total(&self) -> usize {
        self.records.len()
    }

    /// Cases per family, with pass/fail counts. Ordered for stable
    /// display.
    pub fn by_family(&self) -> Vec<(Family, usize, usize)> {
        let mut m: BTreeMap<&'static str, (Family, usize, usize)> = BTreeMap::new();
        for r in &self.records {
            let entry = m.entry(r.family.as_str()).or_insert((r.family, 0, 0));
            if r.passed {
                entry.1 += 1;
            } else {
                entry.2 += 1;
            }
        }
        m.into_values().collect()
    }

    /// Coverage matrix: language → family → count. Sorted on both
    /// axes for stable text output.
    pub fn matrix(&self) -> BTreeMap<String, BTreeMap<&'static str, usize>> {
        let mut out: BTreeMap<String, BTreeMap<&'static str, usize>> = BTreeMap::new();
        for r in &self.records {
            *out.entry(r.language.clone())
                .or_default()
                .entry(r.family.as_str())
                .or_insert(0) += 1;
        }
        out
    }

    /// Print the rollup, matrix, and failure list to stderr.
    pub fn print(&self) {
        eprintln!();
        eprintln!("=== summary ===");
        eprintln!("Total:    {}", self.total());
        eprintln!("Passed:   {}", self.passed());
        eprintln!("Failed:   {}", self.failed());
        eprintln!("Duration: {:?}", self.total_duration);

        let by_fam = self.by_family();
        if !by_fam.is_empty() {
            eprintln!();
            eprintln!("=== by family ===");
            for (fam, pass, fail) in &by_fam {
                eprintln!(
                    "  {:>14}  pass={:<3} fail={:<3}",
                    fam.as_str(),
                    pass,
                    fail
                );
            }
        }

        let matrix = self.matrix();
        if !matrix.is_empty() {
            eprintln!();
            eprintln!("=== coverage matrix (language × family) ===");
            // Collect all family columns that appear anywhere.
            let mut families: Vec<&'static str> = matrix
                .values()
                .flat_map(|m| m.keys().copied())
                .collect();
            families.sort();
            families.dedup();
            // Header
            eprint!("  {:<12}", "");
            for f in &families {
                eprint!(" {:>10}", f);
            }
            eprintln!();
            for (lang, m) in &matrix {
                eprint!("  {:<12}", lang);
                for f in &families {
                    let n = m.get(f).copied().unwrap_or(0);
                    if n == 0 {
                        eprint!(" {:>10}", "·");
                    } else {
                        eprint!(" {:>10}", n);
                    }
                }
                eprintln!();
            }
        }

        if self.failed() > 0 {
            eprintln!();
            eprintln!("=== failures ===");
            for r in self.records.iter().filter(|r| !r.passed) {
                eprintln!();
                eprintln!("✗ {} ({:?})", r.id, r.duration);
                eprintln!("  tool: {}", r.tool);
                eprintln!("  case: {}:1", r.source_path.display());
                if let Some(err) = &r.error {
                    eprintln!("  error: {}", err);
                }
                if !r.diff.is_empty() {
                    for line in r.diff.lines() {
                        eprintln!("  {}", line);
                    }
                }
            }
        }
    }

    /// Persist the per-case status to disk for next run's drift diff.
    /// Stable shape: id → passed boolean, plus generated metadata.
    pub fn write_coverage_yaml(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create {}", parent.display()))?;
        }
        let mut cases: BTreeMap<String, CaseStatus> = BTreeMap::new();
        for r in &self.records {
            cases.insert(
                r.id.clone(),
                CaseStatus {
                    passed: r.passed,
                    family: r.family.as_str().to_string(),
                    language: r.language.clone(),
                },
            );
        }
        let snapshot = CoverageSnapshot {
            total: self.total(),
            passed: self.passed(),
            failed: self.failed(),
            cases,
        };
        let yaml = serde_yaml::to_string(&snapshot)
            .context("serialise coverage snapshot")?;
        std::fs::write(path, yaml).with_context(|| format!("write {}", path.display()))?;
        Ok(())
    }

    /// Compare the current run to a saved snapshot. Returns the four
    /// drift categories sorted by case id.
    pub fn drift_against(&self, prev_path: &Path) -> Result<Option<Drift>> {
        if !prev_path.exists() {
            return Ok(None);
        }
        let raw = std::fs::read_to_string(prev_path)
            .with_context(|| format!("read {}", prev_path.display()))?;
        let prev: CoverageSnapshot = serde_yaml::from_str(&raw)
            .with_context(|| format!("parse {}", prev_path.display()))?;
        let mut drift = Drift::default();
        let cur: BTreeMap<&str, bool> =
            self.records.iter().map(|r| (r.id.as_str(), r.passed)).collect();
        for (id, status) in &prev.cases {
            match cur.get(id.as_str()) {
                None => drift.removed.push(id.clone()),
                Some(&passed_now) => {
                    if status.passed && !passed_now {
                        drift.regressed.push(id.clone());
                    } else if !status.passed && passed_now {
                        drift.fixed.push(id.clone());
                    }
                }
            }
        }
        for r in &self.records {
            if !prev.cases.contains_key(&r.id) {
                drift.added.push(r.id.clone());
            }
        }
        drift.added.sort();
        drift.removed.sort();
        drift.regressed.sort();
        drift.fixed.sort();
        Ok(Some(drift))
    }
}

#[derive(Default)]
pub struct Drift {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub regressed: Vec<String>,
    pub fixed: Vec<String>,
}

impl Drift {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty()
            && self.removed.is_empty()
            && self.regressed.is_empty()
            && self.fixed.is_empty()
    }

    pub fn print(&self) {
        if self.is_empty() {
            return;
        }
        eprintln!();
        eprintln!("=== drift vs last run ===");
        if !self.regressed.is_empty() {
            eprintln!("  regressions ({}):", self.regressed.len());
            for id in &self.regressed {
                eprintln!("    ✗ {}", id);
            }
        }
        if !self.fixed.is_empty() {
            eprintln!("  fixes ({}):", self.fixed.len());
            for id in &self.fixed {
                eprintln!("    ✓ {}", id);
            }
        }
        if !self.added.is_empty() {
            eprintln!("  new cases ({}):", self.added.len());
            for id in &self.added {
                eprintln!("    + {}", id);
            }
        }
        if !self.removed.is_empty() {
            eprintln!("  removed cases ({}):", self.removed.len());
            for id in &self.removed {
                eprintln!("    - {}", id);
            }
        }
    }
}

/// On-disk coverage snapshot. Stable shape — fields can be added but
/// not renamed without breaking diff against older snapshots.
#[derive(serde::Serialize, serde::Deserialize)]
struct CoverageSnapshot {
    total: usize,
    passed: usize,
    failed: usize,
    cases: BTreeMap<String, CaseStatus>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CaseStatus {
    passed: bool,
    family: String,
    language: String,
}

/// Build a `CaseRecord` from a `CaseResult` plus the case metadata.
pub fn record_from_result(
    case: &crate::case::TestCase,
    result: &CaseResult,
) -> CaseRecord {
    CaseRecord {
        id: result.id.clone(),
        tool: case.invoke.tool.clone(),
        family: family_of(&case.invoke.tool),
        language: language_from_fixture(&case.setup.fixture),
        passed: result.passed,
        source_path: case.source_path.clone(),
        duration: result.duration,
        error: result.error.clone(),
        diff: result.diff.clone(),
    }
}

/// Pull the language token out of `languages/<lang>/<file>` fixture
/// paths. Falls back to `unknown` for off-convention paths.
fn language_from_fixture(fixture: &str) -> String {
    let parts: Vec<&str> = fixture.split('/').collect();
    let mut prev = "";
    for p in &parts {
        if prev == "languages" {
            return (*p).to_string();
        }
        prev = p;
    }
    "unknown".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_from_fixture_extracts_segment() {
        assert_eq!(
            language_from_fixture("languages/rust/basic.rs"),
            "rust"
        );
        assert_eq!(
            language_from_fixture("languages/python/foo.py"),
            "python"
        );
        assert_eq!(language_from_fixture("misc/foo.txt"), "unknown");
    }

    #[test]
    fn drift_categorises_changes() {
        // Build a minimal report with 2 passing cases.
        let cur_records = vec![
            CaseRecord {
                id: "a".to_string(),
                tool: "codegraph_symbol_search".to_string(),
                family: Family::Search,
                language: "rust".to_string(),
                passed: true,
                source_path: PathBuf::new(),
                duration: Duration::from_millis(0),
                error: None,
                diff: String::new(),
            },
            CaseRecord {
                id: "c".to_string(),
                tool: "codegraph_symbol_search".to_string(),
                family: Family::Search,
                language: "rust".to_string(),
                passed: false,
                source_path: PathBuf::new(),
                duration: Duration::from_millis(0),
                error: None,
                diff: String::new(),
            },
        ];
        let report = Report::new(cur_records, Duration::from_millis(0));

        // Previous snapshot: a was failing (now fixed), b existed
        // (now removed), c was passing (now regressed).
        let prev_yaml = r#"
total: 2
passed: 2
failed: 0
cases:
  a:
    passed: false
    family: search
    language: rust
  b:
    passed: true
    family: search
    language: rust
  c:
    passed: true
    family: search
    language: rust
"#;
        let dir = tempfile::tempdir().unwrap();
        let prev_path = dir.path().join("prev.yml");
        std::fs::write(&prev_path, prev_yaml).unwrap();

        let drift = report.drift_against(&prev_path).unwrap().unwrap();
        assert_eq!(drift.fixed, vec!["a"]);
        assert_eq!(drift.regressed, vec!["c"]);
        assert_eq!(drift.removed, vec!["b"]);
        assert!(drift.added.is_empty());
    }
}
