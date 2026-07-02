// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! MCP Transport Layer
//!
//! Stdio transport for JSON-RPC 2.0 communication.

use super::protocol::{JsonRpcRequest, JsonRpcResponse};
use std::io::{self, BufRead, Write};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Parse a single input line into a JSON-RPC request.
///
/// Returns `Ok(None)` for empty/whitespace-only lines (caller keeps reading)
/// and `Err(InvalidData)` for malformed JSON. Shared by the sync and async
/// stdio transports so both apply identical framing rules to a read line.
fn parse_request_line(line: &str) -> io::Result<Option<JsonRpcRequest>> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(None);
    }

    match serde_json::from_str(line) {
        Ok(request) => Ok(Some(request)),
        Err(e) => {
            tracing::error!("Failed to parse JSON-RPC request: {}", e);
            Err(io::Error::new(io::ErrorKind::InvalidData, e))
        }
    }
}

/// Synchronous stdio transport for MCP
pub struct StdioTransport {
    stdin: io::Stdin,
    stdout: io::Stdout,
}

impl StdioTransport {
    pub fn new() -> Self {
        Self {
            stdin: io::stdin(),
            stdout: io::stdout(),
        }
    }

    /// Read a JSON-RPC request from stdin
    ///
    /// Returns `Err(UnexpectedEof)` when stdin is closed (client disconnected).
    /// Returns `Ok(None)` for empty/whitespace-only lines (keep reading).
    pub fn read_request(&self) -> io::Result<Option<JsonRpcRequest>> {
        let mut line = String::new();
        let bytes_read = self.stdin.lock().read_line(&mut line)?;

        if bytes_read == 0 {
            // EOF — stdin closed, client disconnected
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "stdin closed"));
        }

        parse_request_line(&line)
    }

    /// Write a JSON-RPC response to stdout
    pub fn write_response(&mut self, response: &JsonRpcResponse) -> io::Result<()> {
        let json = serde_json::to_string(response)?;
        let mut stdout = self.stdout.lock();
        writeln!(stdout, "{}", json)?;
        stdout.flush()
    }
}

impl Default for StdioTransport {
    fn default() -> Self {
        Self::new()
    }
}

/// Async stdio transport for MCP
pub struct AsyncStdioTransport {
    stdin: BufReader<tokio::io::Stdin>,
    stdout: tokio::io::Stdout,
}

impl AsyncStdioTransport {
    pub fn new() -> Self {
        Self {
            stdin: BufReader::new(tokio::io::stdin()),
            stdout: tokio::io::stdout(),
        }
    }

    /// Read a JSON-RPC request from stdin asynchronously
    ///
    /// Returns `Err(UnexpectedEof)` when stdin is closed (client disconnected).
    /// Returns `Ok(None)` for empty/whitespace-only lines (keep reading).
    pub async fn read_request(&mut self) -> io::Result<Option<JsonRpcRequest>> {
        let mut line = String::new();
        let bytes_read = self.stdin.read_line(&mut line).await?;

        if bytes_read == 0 {
            // EOF — stdin closed, client disconnected
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "stdin closed"));
        }

        parse_request_line(&line)
    }

    /// Write a JSON-RPC response to stdout asynchronously
    pub async fn write_response(&mut self, response: &JsonRpcResponse) -> io::Result<()> {
        let json = serde_json::to_string(response)?;
        self.stdout.write_all(json.as_bytes()).await?;
        self.stdout.write_all(b"\n").await?;
        self.stdout.flush().await
    }
}

impl Default for AsyncStdioTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::protocol::JsonRpcError;

    #[test]
    fn test_response_serialization() {
        let response = JsonRpcResponse::success(
            Some(serde_json::json!(1)),
            serde_json::json!({"status": "ok"}),
        );
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"result\""));
    }

    #[test]
    fn test_error_response_serialization() {
        let error = JsonRpcError::method_not_found("unknown");
        let response = JsonRpcResponse::error(Some(serde_json::json!(1)), error);
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("-32601"));
    }

    #[test]
    fn parse_request_line_returns_request_for_valid_json() {
        // The happy path both stdio transports funnel through: a well-formed
        // JSON-RPC line deserializes into a JsonRpcRequest with fields intact.
        let line = r#"{"jsonrpc":"2.0","id":7,"method":"ping","params":{"x":1}}"#;
        let parsed = parse_request_line(line).unwrap().unwrap();
        assert_eq!(parsed.jsonrpc, "2.0");
        assert_eq!(parsed.id, Some(serde_json::json!(7)));
        assert_eq!(parsed.method, "ping");
        assert_eq!(parsed.params, Some(serde_json::json!({"x": 1})));
    }

    #[test]
    fn parse_request_line_trims_surrounding_whitespace() {
        // read_line hands over the trailing newline (and any leading indent);
        // the helper trims before parsing so a padded but valid line still
        // deserializes rather than being treated as malformed.
        let line = "  \t {\"jsonrpc\":\"2.0\",\"method\":\"m\"}\n";
        let parsed = parse_request_line(line).unwrap().unwrap();
        assert_eq!(parsed.method, "m");
        // `id` is optional and absent here, so it defaults to None.
        assert_eq!(parsed.id, None);
    }

    #[test]
    fn parse_request_line_returns_none_for_blank_line() {
        // Empty and whitespace-only lines are the "keep reading" signal:
        // Ok(None), not an error, so the read loop does not disconnect.
        assert!(parse_request_line("").unwrap().is_none());
        assert!(parse_request_line("   \t\n").unwrap().is_none());
    }

    #[test]
    fn parse_request_line_errors_on_malformed_json() {
        // A non-empty line that is not valid JSON-RPC surfaces as an
        // InvalidData io::Error (never a silent None), so the caller can
        // report a parse failure distinctly from EOF/blank lines.
        let err = parse_request_line("{not json").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }
}
