// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Process-level telemetry emission.
//!
//! Events are EMITTED here as `TEL:`-prefixed stderr lines; a JS wrapper (the
//! npm package or the VS Code extension) parses stderr and forwards them to
//! PostHog. No network calls happen in this binary by design — egress lives in
//! the JS layer. stdout stays reserved for the JSON-RPC channel.

use serde_json::Value;

/// Emit a structured telemetry event to stderr for the wrapper to forward.
///
/// Opt-out at the source via `CODEGRAPH_TELEMETRY=off` (the wrapper also gates
/// forwarding). Silently dropped if serialization fails — never blocks.
pub fn emit_tel(value: Value) {
    if std::env::var("CODEGRAPH_TELEMETRY")
        .map(|v| v.eq_ignore_ascii_case("off"))
        .unwrap_or(false)
    {
        return;
    }
    if let Ok(json) = serde_json::to_string(&value) {
        eprintln!("TEL: {json}");
    }
}

/// Resident set size of the current process in MB (0 if unavailable).
///
/// Used by the daemon to report its own footprint — the signal that informs
/// whether the shared-RocksDB model needs to upgrade to a single resident
/// process (see the daemon Model A/B trade-off).
pub fn current_rss_mb() -> u64 {
    use sysinfo::{Pid, System};
    let mut sys = System::new();
    let pid = Pid::from_u32(std::process::id());
    sys.refresh_process(pid);
    sys.process(pid)
        .map(|p| p.memory() / (1024 * 1024))
        .unwrap_or(0)
}
