// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Per-case execution. Sets up a temporary workspace, spawns the
//! codegraph-server binary, calls the tool, compares the response.

use crate::case::{NormalizeOpts, Setup, TestCase, WorkspaceLayout};
use crate::compare::compare;
use crate::jsonrpc::McpClient;
use crate::normalize::{normalize, normalize_expected};
use crate::profiles;
use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub struct CaseResult {
    pub id: String,
    pub passed: bool,
    pub diff: String,
    pub duration: Duration,
    pub error: Option<String>,
    /// The normalized actual response, populated only when the runner
    /// is invoked in bless mode. Used by main to overwrite the
    /// case's `expect.data` block.
    pub blessed_actual: Option<serde_json::Value>,
}

pub fn run_case(
    case: &TestCase,
    binary: &Path,
    fixtures_root: &Path,
    active_embedding_model: Option<&str>,
    bless: bool,
) -> CaseResult {
    let started = Instant::now();
    match run_inner(case, binary, fixtures_root, active_embedding_model, bless) {
        Ok(result) => CaseResult {
            id: case.id.clone(),
            passed: result.passed,
            diff: result.diff,
            duration: started.elapsed(),
            error: None,
            blessed_actual: result.blessed_actual,
        },
        Err(e) => CaseResult {
            id: case.id.clone(),
            passed: false,
            diff: String::new(),
            duration: started.elapsed(),
            error: Some(format!("{:#}", e)),
            blessed_actual: None,
        },
    }
}

struct InnerResult {
    passed: bool,
    diff: String,
    blessed_actual: Option<serde_json::Value>,
}

fn run_inner(
    case: &TestCase,
    binary: &Path,
    fixtures_root: &Path,
    active_embedding_model: Option<&str>,
    bless: bool,
) -> Result<InnerResult> {
    // 0a. Resolve effective normalisation: per-tool profile (base) +
    //     per-case overrides (overlay). Case wins per-field; vec
    //     fields concatenate.
    let resolved_opts = NormalizeOpts::merge(
        profiles::default_for(&case.invoke.tool),
        case.expect.normalize.clone(),
    );

    // 0b. Embedding-model gate. If the resolved opts pin a model and
    //     the active model differs (or wasn't supplied), fail loudly —
    //     these cases encode cosine values that change when the model
    //     changes.
    if let Some(pinned) = resolved_opts.embedding_model.as_deref() {
        match active_embedding_model {
            Some(active) if active == pinned => {}
            Some(active) => {
                return Err(anyhow!(
                    "case pins embedding_model={} but active is {} — re-bless expectations or pass --embedding-model {}",
                    pinned, active, pinned,
                ));
            }
            None => {
                return Err(anyhow!(
                    "case pins embedding_model={} but harness was started without --embedding-model — refusing to run",
                    pinned,
                ));
            }
        }
    }

    // 1. Build workspace
    let workspace = make_workspace(&case.setup, fixtures_root)?;
    let workspace_path = workspace.path().to_path_buf();
    let fixture_in_workspace = resolve_fixture_path(&case.setup, &workspace_path)?;

    // 1b. Optionally init a git repo. Deterministic author + email so
    //     the resulting commit hash is stable across runs on the same
    //     fixture content + git version. The hash *can* still shift
    //     between git versions or across OSes, so git-history cases
    //     should not assert on sha values directly (mark them volatile
    //     via the global VOLATILE_FIELDS list).
    if case.setup.init_git {
        init_git_repo(&workspace_path)?;
    }

    // 2. Resolve effective binary — per-case override wins over the
    //    harness-default. Used to point pro-only cases at codegraph-pro
    //    while OSS cases use the OSS binary.
    let owned_binary;
    let effective_binary: &Path = match case.invoke.binary.as_deref() {
        Some(p) => {
            owned_binary = std::path::PathBuf::from(p);
            if !owned_binary.exists() {
                return Err(anyhow!(
                    "case {} pins binary `{}` but path does not exist",
                    case.id,
                    p
                ));
            }
            owned_binary.as_path()
        }
        None => binary,
    };
    let mut client = McpClient::spawn(effective_binary, &workspace_path)?;

    // 3. Substitute ${fixture} / ${workspace} in args (case YAMLs use
    //    these so the real tempdir path can be injected at runtime).
    //    Expected JSON does NOT get this treatment — it's compared
    //    against the normalised response which already has the
    //    placeholders restored.
    let fixture_str = fixture_in_workspace.to_string_lossy().to_string();
    let workspace_str = workspace_path.to_string_lossy().to_string();
    let args = crate::compare::substitute_placeholders(
        &case.invoke.args,
        &fixture_str,
        &workspace_str,
    );

    // 4. Call tool. If `retry_on_warmup` is set, retry every 2s
    //    for up to 30s while the response shape indicates the
    //    embedding pipeline is still warming up. Stabilises
    //    similarity-family cases that race against ONNX startup.
    let timeout = Duration::from_millis(case.invoke.timeout_ms);
    let mut response = client
        .call_tool(&case.invoke.tool, &args, timeout)
        .with_context(|| format!("call_tool({})", case.invoke.tool))?;
    if case.invoke.retry_on_warmup {
        let started = std::time::Instant::now();
        while is_warmup_response(&response) && started.elapsed() < Duration::from_secs(30) {
            std::thread::sleep(Duration::from_secs(2));
            response = client
                .call_tool(&case.invoke.tool, &args, timeout)
                .with_context(|| format!("retry call_tool({})", case.invoke.tool))?;
        }
    }

    // 5. Shutdown
    let _ = client.shutdown();

    // 6. Unwrap the MCP envelope. Many tools wrap their payload as
    //    `{ "content": [{ "type": "text", "text": "<json>" }] }`.
    //    Parse the inner JSON if present; otherwise use the raw response.
    let raw = unwrap_mcp_content(&response).unwrap_or(response);

    // 7. Normalise — strip volatile fields, substitute paths back to
    //    `${fixture}` / `${workspace}` placeholders, then per-case
    //    passes (sort, float-round, extra strips). Apply the same
    //    safe subset to expected so authoring is forgiving (no need
    //    to pre-sort or pre-round). Uses the resolved profile+case
    //    merge from step 0a so authoring is also profile-forgiving.
    let normalised = normalize(&raw, &workspace_str, &fixture_str, &resolved_opts);

    // Bless mode short-circuits: skip comparison, hand the normalized
    // actual back to main to splice into the YAML's `expect.data`.
    if bless {
        return Ok(InnerResult {
            passed: true,
            diff: String::new(),
            blessed_actual: Some(normalised),
        });
    }

    let expected = normalize_expected(&case.expect.data, &resolved_opts);

    // 8. Compare
    let comp = compare(&normalised, &expected, case.expect.r#match)?;

    Ok(InnerResult {
        passed: comp.passed,
        diff: comp.diff,
        blessed_actual: None,
    })
}

/// Copy the fixture (single file or directory) into a fresh tempdir
/// and return a handle to it. The harness owns the tempdir and drops
/// it when the case finishes.
fn make_workspace(setup: &Setup, fixtures_root: &Path) -> Result<tempfile::TempDir> {
    let src = fixtures_root.join(&setup.fixture);
    if !src.exists() {
        return Err(anyhow!(
            "fixture not found: {} (relative to {})",
            setup.fixture,
            fixtures_root.display()
        ));
    }
    let workspace = tempfile::Builder::new()
        .prefix("codegraph-harness-")
        .tempdir()
        .context("create tempdir for workspace")?;

    match setup.workspace_layout {
        WorkspaceLayout::SingleFile => {
            let dest_name = src
                .file_name()
                .ok_or_else(|| anyhow!("fixture has no filename: {}", src.display()))?;
            let dest = workspace.path().join(dest_name);
            std::fs::copy(&src, &dest)
                .with_context(|| format!("copy {} -> {}", src.display(), dest.display()))?;
        }
        WorkspaceLayout::MultiFile => {
            let dir = if src.is_dir() { src.clone() } else { src.parent().unwrap().to_path_buf() };
            copy_dir_recursive(&dir, workspace.path())?;
        }
    }
    Ok(workspace)
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    for entry in walkdir::WalkDir::new(src) {
        let entry = entry?;
        let rel = entry.path().strip_prefix(src)?;
        let target = dst.join(rel);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&target)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// True if the response indicates the embedding pipeline is still
/// warming up. The MCP envelope wraps the payload as
/// `{ content: [{ type: "text", text: "<json>" }] }` and tools emit
/// either `{status: "embeddings_in_progress", message: "..."}` or a
/// `message` containing the canonical "Embeddings are building"
/// substring. Either signal causes a retry under `retry_on_warmup`.
fn is_warmup_response(response: &serde_json::Value) -> bool {
    let candidates = [
        response.clone(),
        unwrap_mcp_content(response).unwrap_or(serde_json::Value::Null),
    ];
    for v in &candidates {
        if let Some(obj) = v.as_object() {
            if obj.get("status").and_then(|s| s.as_str()) == Some("embeddings_in_progress") {
                return true;
            }
            if let Some(msg) = obj.get("message").and_then(|s| s.as_str()) {
                if msg.contains("Embeddings are building") {
                    return true;
                }
            }
        }
    }
    false
}

/// Initialise a git repo in `workspace` with one deterministic commit
/// containing the fixture contents. Used by cases that flip
/// `setup.init_git: true` to exercise git-history tools. Fails loudly
/// if `git` is not on PATH.
fn init_git_repo(workspace: &Path) -> Result<()> {
    fn run(cmd: &mut std::process::Command) -> Result<()> {
        let out = cmd.output().context("spawn git subprocess")?;
        if !out.status.success() {
            return Err(anyhow!(
                "git command failed ({}): {}",
                out.status,
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        Ok(())
    }
    run(std::process::Command::new("git").arg("init").arg("-q").current_dir(workspace))?;
    run(std::process::Command::new("git").args(["add", "."]).current_dir(workspace))?;
    // Pin author + committer dates so the commit content (and the
    // SHA derived from it) is byte-stable across runs. Without these,
    // git uses wallclock and every run produces a different hash.
    run(std::process::Command::new("git")
        .args([
            "-c", "user.email=harness@codegraph.test",
            "-c", "user.name=Harness",
            "-c", "commit.gpgsign=false",
            "commit", "-q", "-m", "harness fixture",
        ])
        .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00+0000")
        .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00+0000")
        .current_dir(workspace))?;
    Ok(())
}

fn resolve_fixture_path(setup: &Setup, workspace: &Path) -> Result<PathBuf> {
    let fixture_relative = Path::new(&setup.fixture)
        .file_name()
        .ok_or_else(|| anyhow!("fixture has no filename: {}", setup.fixture))?;
    Ok(workspace.join(fixture_relative))
}

/// Many MCP tools wrap their payload as
/// `{ "content": [ { "type": "text", "text": "<json>" } ] }`. Unwrap
/// the inner JSON if present so case YAML can describe the *tool's*
/// output, not the MCP envelope. Returns None if the wrapper isn't
/// present (caller falls back to comparing the raw response).
fn unwrap_mcp_content(response: &serde_json::Value) -> Option<serde_json::Value> {
    let content = response.get("content")?.as_array()?;
    let first = content.first()?;
    let text = first.get("text")?.as_str()?;
    serde_json::from_str(text).ok()
}
