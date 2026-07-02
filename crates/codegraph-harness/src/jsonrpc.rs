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
            format!(
                "spawn {} --mcp --workspace {}",
                binary.display(),
                workspace.display()
            )
        })?;
        let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
        let stdout = BufReader::new(child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?);
        let mut client = McpClient {
            child,
            stdin,
            stdout,
            next_id: 1,
        };
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
        let body = encode_request(value)?;
        self.stdin.write_all(&body)?;
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
            match parse_response_line(&line)? {
                Some(value) => return Ok(value),
                None => continue,
            }
        }
    }
}

/// Serialize a JSON-RPC message into a line-delimited frame: the compact
/// JSON body followed by a single `\n`. Shared by `send` so the framing is
/// testable without a live child process.
fn encode_request(value: &Value) -> anyhow::Result<Vec<u8>> {
    let mut body = serde_json::to_vec(value)?;
    body.push(b'\n');
    Ok(body)
}

/// Parse one line of the server's line-delimited JSON-RPC output.
/// Returns `Ok(None)` for a blank/whitespace-only line (skipped by the
/// caller), `Ok(Some(value))` for a well-formed JSON document, or `Err`
/// for malformed JSON. Extracted from `recv` to create a stdout-free seam.
fn parse_response_line(line: &str) -> anyhow::Result<Option<Value>> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let value: Value =
        serde_json::from_str(trimmed).with_context(|| format!("parse JSON line: {:?}", trimmed))?;
    Ok(Some(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_request_appends_single_newline() {
        let msg = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "ping"});
        let bytes = encode_request(&msg).expect("encode");
        assert_eq!(*bytes.last().unwrap(), b'\n');
        // Exactly one trailing newline, none embedded in the compact body.
        assert_eq!(bytes.iter().filter(|&&b| b == b'\n').count(), 1);
        // The body (minus the newline) round-trips back to the same value.
        let parsed: Value = serde_json::from_slice(&bytes[..bytes.len() - 1]).expect("parse");
        assert_eq!(parsed, msg);
    }

    #[test]
    fn parse_response_line_valid_json() {
        let line = "{\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{\"ok\":true}}\n";
        let value = parse_response_line(line).expect("ok").expect("some");
        assert_eq!(value.get("id").and_then(Value::as_i64), Some(7));
        assert_eq!(
            value.pointer("/result/ok").and_then(Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn parse_response_line_trims_surrounding_whitespace() {
        let line = "   \t {\"jsonrpc\":\"2.0\",\"id\":3}  \r\n";
        let value = parse_response_line(line).expect("ok").expect("some");
        assert_eq!(value.get("id").and_then(Value::as_i64), Some(3));
    }

    #[test]
    fn parse_response_line_blank_is_none() {
        assert!(parse_response_line("").expect("ok").is_none());
        assert!(parse_response_line("   \t\r\n").expect("ok").is_none());
    }

    #[test]
    fn parse_response_line_malformed_is_err() {
        let err = parse_response_line("{not valid json").unwrap_err();
        assert!(err.to_string().contains("parse JSON line"));
    }
}
