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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn emit_tel_respects_off_optout_and_emits_otherwise() {
        // This is the only reader of CODEGRAPH_TELEMETRY in the crate, so
        // mutating it here cannot race another test. Exercise both arms in one
        // test to avoid intra-module env races between parallel tests.
        let event = json!({ "event": "test_event", "n": 1 });

        // Opt-out arm: `off` (case-insensitive) returns early without emitting.
        std::env::set_var("CODEGRAPH_TELEMETRY", "OfF");
        emit_tel(event.clone());

        // Emit arm: any non-`off` value (or unset) reaches the eprintln! path.
        std::env::set_var("CODEGRAPH_TELEMETRY", "on");
        emit_tel(event.clone());
        std::env::remove_var("CODEGRAPH_TELEMETRY");
        emit_tel(event);
        // No panic and no return value: the contract is that emission never
        // blocks or fails the caller regardless of the opt-out state.
    }

    #[test]
    fn current_rss_mb_reports_megabytes() {
        // The running test process must have some resident memory; the value is
        // reported in MB, so it stays well under a 1 TB sanity bound (it would
        // blow past this if the fn accidentally returned raw bytes).
        let rss = current_rss_mb();
        assert!(
            rss < 1_000_000,
            "rss {rss} MB implausibly large - unit bug?"
        );
    }
}
