// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

fn main() {
    // On Windows, we use ort-load-dynamic (fastembed feature) to load
    // ONNX Runtime as a shared library at runtime. This avoids the
    // CRT mismatch between ort-sys (/MT) and rocksdb-sys (/MD).
    //
    // On macOS/Linux, ort-download-binaries statically links ONNX Runtime.

    // Generate build-time provenance constants
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("build_info.rs");

    let git_hash = run_cmd("git", &["rev-parse", "HEAD"]);
    let git_hash_short = run_cmd("git", &["rev-parse", "--short", "HEAD"]);

    let git_dirty = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);

    let dirty_suffix = if git_dirty { "-dirty" } else { "" };
    let rustc_version = run_cmd("rustc", &["--version"]);

    // Cross-platform timestamp (macOS date doesn't support --utc)
    let build_timestamp = Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let contents = format!(
        r#"/// The full git commit hash at build time.
pub const GIT_HASH: &str = "{git_hash}";

/// The short git commit hash at build time.
pub const GIT_HASH_SHORT: &str = "{git_hash_short}{dirty_suffix}";

/// The rustc version used to compile this build.
pub const RUSTC_VERSION: &str = "{rustc_version}";

/// The UTC timestamp when this build was compiled.
pub const BUILD_TIMESTAMP: &str = "{build_timestamp}";
"#
    );

    fs::write(&dest_path, contents).unwrap();

    // Re-run if git HEAD changes (new commit)
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/refs/");
}

fn run_cmd(cmd: &str, args: &[&str]) -> String {
    Command::new(cmd)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}
