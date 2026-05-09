// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Minimal JSON-RPC stdio client for driving codegraph-server in MCP
//! mode. P1 supports `initialize`, `tools/call`, and graceful shutdown.
//!
//! codegraph-server uses **line-delimited JSON** on stdio — each
//! message is a single JSON document terminated by `\n`. (NOT
//! Content-Length-framed LSP-style — verified against
//! `crates/codegraph-server/src/mcp/transport.rs`.)

use anyhow::{anyhow, Context};
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

pub struct McpClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: i64,
}

impl McpClient {
    /// Spawn `binary --mcp --workspace <workspace>` and complete the
    /// `initialize` handshake. Returns once the server is ready to
    /// accept tools/call.
    pub fn spawn(binary: &std::path::Path, workspace: &std::path::Path) -> anyhow::Result<Self> {
        let mut cmd = Command::new(binary);
        cmd.arg("--mcp")
            .arg("--workspace")
            .arg(workspace)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = cmd.spawn().with_context(|| {
            format!("spawn {} --mcp --workspace {}", binary.display(), workspace.display())
        })?;
        let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
        let stdout = BufReader::new(child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?);
        let mut client = McpClient { child, stdin, stdout, next_id: 1 };
        client.handshake()?;
        Ok(client)
    }

    fn handshake(&mut self) -> anyhow::Result<()> {
        // MCP `initialize` request.
        let init_req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": self.next_id,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "codegraph-harness", "version": "0.1.0" }
            }
        });
        self.next_id += 1;
        self.send(&init_req)?;
        let _resp = self.recv(Duration::from_secs(60))?;
        // Send the `initialized` notification (no response expected).
        let init_notif = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        });
        self.send(&init_notif)?;
        Ok(())
    }

    /// Issue a `tools/call` request and return the parsed response.
    /// Returns the `result` field on success, or an error containing
    /// the JSON-RPC error payload.
    pub fn call_tool(
        &mut self,
        name: &str,
        args: &Value,
        timeout: Duration,
    ) -> anyhow::Result<Value> {
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": self.next_id,
            "method": "tools/call",
            "params": { "name": name, "arguments": args }
        });
        let req_id = self.next_id;
        self.next_id += 1;
        self.send(&req)?;
        loop {
            let resp = self.recv(timeout)?;
            // Skip notifications (no `id` field) and responses for
            // earlier requests that arrive late.
            let id = resp.get("id").and_then(|v| v.as_i64());
            if id != Some(req_id) {
                continue;
            }
            if let Some(err) = resp.get("error") {
                return Err(anyhow!("tool call error: {}", err));
            }
            return resp
                .get("result")
                .cloned()
                .ok_or_else(|| anyhow!("response had no `result` and no `error`: {}", resp));
        }
    }

    /// Send `shutdown` then `exit`, then wait for the child.
    pub fn shutdown(mut self) -> anyhow::Result<()> {
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": self.next_id,
            "method": "shutdown",
            "params": {}
        });
        let _ = self.send(&req); // best-effort
        let exit = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "exit"
        });
        let _ = self.send(&exit);
        // Give the server a moment to close cleanly, then kill if needed.
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match self.child.try_wait()? {
                Some(_) => return Ok(()),
                None if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                None => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    return Ok(());
                }
            }
        }
    }

    fn send(&mut self, value: &Value) -> anyhow::Result<()> {
        let body = serde_json::to_vec(value)?;
        self.stdin.write_all(&body)?;
        self.stdin.write_all(b"\n")?;
        self.stdin.flush()?;
        Ok(())
    }

    fn recv(&mut self, timeout: Duration) -> anyhow::Result<Value> {
        // Line-delimited JSON: read up to `\n`, parse the body. Skip
        // blank lines (defensive — shouldn't occur but cheap to handle).
        let _deadline = Instant::now() + timeout;
        // NOTE: BufRead::read_line is blocking; we don't enforce the
        // per-call timeout here. Server hangs surface via the parent's
        // case-level timeout (the run is bounded by the outer loop).
        loop {
            let mut line = String::new();
            let n = self.stdout.read_line(&mut line)?;
            if n == 0 {
                return Err(anyhow!("server closed stdout"));
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let value: Value = serde_json::from_str(trimmed)
                .with_context(|| format!("parse JSON line: {:?}", trimmed))?;
            return Ok(value);
        }
    }
}
