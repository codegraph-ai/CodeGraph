// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! MCP Transport Layer
//!
//! Stdio transport for JSON-RPC 2.0 communication.

use super::protocol::{JsonRpcRequest, JsonRpcResponse};
use std::io::{self, BufRead, Write};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

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
}
