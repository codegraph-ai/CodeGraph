// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! codegraph-harness — JSON-RPC integration test runner for codegraph-server.
//!
//! Discovers `*.case.yml` files, runs each as a subprocess of the
//! `codegraph-server --mcp` binary, compares the JSON-RPC response to
//! the case's expected payload. Pure programmatic match.
//!
//! Usage:
//!   codegraph-harness                            # run all cases
//!   codegraph-harness --filter symbol_search     # run cases whose id contains a string
//!   codegraph-harness --binary <path>            # override codegraph-server path
//!
//! Exit code: 0 if all cases pass, 1 if any fail.

mod bless;
mod case;
mod compare;
mod jsonrpc;
mod normalize;
mod profiles;
mod report;
mod runner;

use anyhow::{anyhow, Result};
use clap::Parser;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(version, about = "codegraph-server JSON-RPC test harness")]
struct Args {
    /// Override path to the codegraph-server binary. Default: search the
    /// workspace target/release / target/debug, then $PATH.
    #[arg(long)]
    binary: Option<PathBuf>,
    /// Override the cases directory. Default:
    /// crates/codegraph-harness/cases (relative to repo root).
    #[arg(long)]
    cases_dir: Option<PathBuf>,
    /// Override the fixtures directory. Default:
    /// crates/codegraph-harness/fixtures (relative to repo root).
    #[arg(long)]
    fixtures_dir: Option<PathBuf>,
    /// Substring filter — only run cases whose id contains this
    /// string. Comma-separated for multiple patterns; a case matches
    /// if its id contains ANY of the patterns.
    #[arg(long)]
    filter: Option<String>,
    /// Print verbose case-by-case progress instead of just the summary.
    #[arg(long, short)]
    verbose: bool,
    /// Active embedding model name. Cases that pin
    /// `expect.normalize.embedding_model` to a different value fail
    /// loudly — prevents silent drift when the model changes and
    /// expectations haven't been re-blessed. Optional; if unset, no
    /// assertion is performed.
    #[arg(long)]
    embedding_model: Option<String>,
    /// Bless / record mode: instead of comparing, overwrite each
    /// matched case's `expect.data` with the normalized actual
    /// response. Use with `--filter` to scope. Comments in the YAML
    /// are NOT preserved (serde_yaml round-trip). Workflow:
    /// 1. Author writes a stub case with `data: {}`.
    /// 2. Runs `--bless --filter <id>` to fill in expected.
    /// 3. Reviews the diff, commits.
    #[arg(long)]
    bless: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let binary = resolve_binary(args.binary.as_deref(), crate_root)?;
    let cases_dir = args
        .cases_dir
        .unwrap_or_else(|| crate_root.join("cases"));
    let fixtures_dir = args
        .fixtures_dir
        .unwrap_or_else(|| crate_root.join("fixtures"));

    eprintln!("=== codegraph-harness ===");
    eprintln!("binary:    {}", binary.display());
    eprintln!("cases:     {}", cases_dir.display());
    eprintln!("fixtures:  {}", fixtures_dir.display());
    if let Some(f) = &args.filter {
        eprintln!("filter:    {}", f);
    }
    eprintln!();

    let cases = discover_cases(&cases_dir, args.filter.as_deref())?;
    if cases.is_empty() {
        eprintln!("no cases discovered");
        return Ok(());
    }

    let mut records: Vec<report::CaseRecord> = Vec::new();
    let mut blessed_count = 0usize;
    let mut bless_errors: Vec<(String, String)> = Vec::new();
    let started = std::time::Instant::now();

    if args.bless {
        eprintln!("bless mode: rewriting expect.data for matched cases");
        eprintln!();
    }

    for case in &cases {
        if args.verbose {
            eprintln!("running {}...", case.id);
        }
        let result = runner::run_case(
            case,
            &binary,
            &fixtures_dir,
            args.embedding_model.as_deref(),
            args.bless,
        );
        if args.bless {
            if let Some(actual) = &result.blessed_actual {
                match bless::rewrite_expect_data(&case.source_path, actual) {
                    Ok(()) => {
                        blessed_count += 1;
                        eprintln!("  ✓ blessed {}", case.id);
                    }
                    Err(e) => {
                        bless_errors.push((case.id.clone(), format!("{:#}", e)));
                        eprintln!("  ✗ failed to bless {}: {:#}", case.id, e);
                    }
                }
            } else if let Some(err) = &result.error {
                bless_errors.push((case.id.clone(), err.clone()));
                eprintln!("  ✗ failed to run {}: {}", case.id, err);
            }
        } else if args.verbose {
            if result.passed {
                eprintln!("  ✓ {} ({:?})", case.id, result.duration);
            } else {
                eprintln!("  ✗ {} ({:?})", case.id, result.duration);
            }
        }
        records.push(report::record_from_result(case, &result));
    }

    let duration = started.elapsed();

    if args.bless {
        eprintln!();
        eprintln!("=== bless summary ===");
        eprintln!("Matched:  {}", cases.len());
        eprintln!("Blessed:  {}", blessed_count);
        eprintln!("Errors:   {}", bless_errors.len());
        eprintln!("Duration: {:?}", duration);
        if !bless_errors.is_empty() {
            eprintln!();
            eprintln!("=== errors ===");
            for (id, err) in &bless_errors {
                eprintln!("  ✗ {}: {}", id, err);
            }
            std::process::exit(1);
        }
        return Ok(());
    }

    let any_failed = records.iter().any(|r| !r.passed);
    let report = report::Report::new(records, duration);

    // Drift / snapshot persistence is only meaningful for full runs.
    // Filter-narrowed runs would otherwise mis-report every unfiltered
    // case as "removed" and overwrite the snapshot with a partial set.
    let full_run = args.filter.is_none();
    let coverage_path = crate_root
        .join("..")
        .join("..")
        .join("target")
        .join("codegraph-harness")
        .join("coverage-latest.yml");
    let drift = if full_run {
        match report.drift_against(&coverage_path) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("warning: could not read previous coverage snapshot: {:#}", e);
                None
            }
        }
    } else {
        None
    };

    report.print();
    if let Some(d) = &drift {
        d.print();
    }

    if full_run {
        if let Err(e) = report.write_coverage_yaml(&coverage_path) {
            eprintln!("warning: could not write coverage snapshot: {:#}", e);
        }
    }

    if any_failed {
        std::process::exit(1);
    }

    Ok(())
}

fn discover_cases(cases_dir: &Path, filter: Option<&str>) -> Result<Vec<case::TestCase>> {
    let mut cases = Vec::new();
    if !cases_dir.exists() {
        return Err(anyhow!(
            "cases directory does not exist: {}",
            cases_dir.display()
        ));
    }
    let patterns: Vec<&str> = filter
        .map(|f| f.split(',').map(str::trim).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();
    for entry in walkdir::WalkDir::new(cases_dir) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let p = entry.path();
        if !p.to_string_lossy().ends_with(".case.yml") {
            continue;
        }
        let case = case::TestCase::from_path(p)?;
        if !patterns.is_empty() && !patterns.iter().any(|pat| case.id.contains(pat)) {
            continue;
        }
        cases.push(case);
    }
    cases.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(cases)
}

fn resolve_binary(override_path: Option<&Path>, crate_root: &Path) -> Result<PathBuf> {
    if let Some(p) = override_path {
        if !p.exists() {
            return Err(anyhow!("--binary path does not exist: {}", p.display()));
        }
        return Ok(p.to_path_buf());
    }
    // Search ../../target/{release,debug}/codegraph-server
    let workspace_target = crate_root.join("..").join("..").join("target");
    let candidates = [
        workspace_target.join("release").join("codegraph-server"),
        workspace_target.join("debug").join("codegraph-server"),
    ];
    for c in &candidates {
        if c.exists() {
            // Canonicalize so the spawned process sees a stable path.
            return Ok(c.canonicalize()?);
        }
    }
    // Fall back to $PATH.
    if let Ok(p) = which::which("codegraph-server") {
        return Ok(p);
    }
    Err(anyhow!(
        "codegraph-server binary not found. Run `cargo build -p codegraph-server` \
         or pass --binary <path>."
    ))
}

#[allow(unused)]
const _UNUSED: Duration = Duration::ZERO;
