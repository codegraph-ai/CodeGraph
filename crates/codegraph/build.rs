// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Build script for the codegraph crate.
//!
//! Embeds compile-time provenance metadata (git hash, build timestamp,
//! rustc version) into the binary via generated constants.

use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("build_info.rs");

    let git_hash = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let git_hash_short = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let git_dirty = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);

    let dirty_suffix = if git_dirty { "-dirty" } else { "" };

    let rustc_version = Command::new("rustc")
        .args(["--version"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let build_timestamp = Command::new("date")
        .args(["--utc", "+%Y-%m-%dT%H:%M:%SZ"])
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
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs/");
}
