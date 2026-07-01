// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! MCP Protocol Types
//!
//! JSON-RPC 2.0 message types for the Model Context Protocol.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// JSON-RPC 2.0 request
#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}

/// JSON-RPC 2.0 response
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    pub fn success(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: Option<Value>, error: JsonRpcError) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(error),
        }
    }
}

/// JSON-RPC 2.0 error
#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JsonRpcError {
    pub fn parse_error(message: impl Into<String>) -> Self {
        Self {
            code: -32700,
            message: message.into(),
            data: None,
        }
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self {
            code: -32600,
            message: message.into(),
            data: None,
        }
    }

    pub fn method_not_found(method: impl Into<String>) -> Self {
        Self {
            code: -32601,
            message: format!("Method not found: {}", method.into()),
            data: None,
        }
    }

    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: message.into(),
            data: None,
        }
    }

    pub fn internal_error(message: impl Into<String>) -> Self {
        Self {
            code: -32603,
            message: message.into(),
            data: None,
        }
    }
}

/// MCP Initialize Request params
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    #[serde(default)]
    pub protocol_version: Option<String>,
    #[serde(default)]
    pub capabilities: ClientCapabilities,
    #[serde(default)]
    pub client_info: Option<ClientInfo>,
    /// Workspace roots provided by the client (MCP 2024-11-05+).
    #[serde(default)]
    pub roots: Option<Vec<Root>>,
}

/// A workspace root provided by the MCP client.
#[derive(Debug, Clone, Deserialize)]
pub struct Root {
    /// Root URI (typically file:///path/to/dir)
    pub uri: String,
    /// Optional human-readable name
    #[serde(default)]
    pub name: Option<String>,
}

/// MCP Client capabilities
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClientCapabilities {
    #[serde(default)]
    pub experimental: Option<HashMap<String, Value>>,
    #[serde(default)]
    pub sampling: Option<SamplingCapability>,
    #[serde(default)]
    pub roots: Option<RootsCapability>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct SamplingCapability {}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RootsCapability {
    #[serde(default)]
    pub list_changed: Option<bool>,
}

/// Client info
#[derive(Debug, Clone, Deserialize)]
pub struct ClientInfo {
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
}

/// MCP Initialize response
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub protocol_version: String,
    pub capabilities: ServerCapabilities,
    pub server_info: ServerInfo,
}

/// MCP Server capabilities
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental: Option<HashMap<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logging: Option<LoggingCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompts: Option<PromptsCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourcesCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolsCapability>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct LoggingCapability {}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PromptsCapability {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ResourcesCapability {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscribe: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ToolsCapability {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

/// Server info
#[derive(Debug, Clone, Serialize)]
pub struct ServerInfo {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// MCP Tool definition
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub input_schema: ToolInputSchema,
}

/// Tool input schema (JSON Schema subset)
#[derive(Debug, Clone, Serialize)]
pub struct ToolInputSchema {
    #[serde(rename = "type")]
    pub schema_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, PropertySchema>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,
}

/// Property schema for tool inputs
#[derive(Debug, Clone, Serialize)]
pub struct PropertySchema {
    #[serde(rename = "type")]
    pub property_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
    pub enum_values: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Box<PropertySchema>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum: Option<f64>,
}

/// Tools list response
#[derive(Debug, Clone, Serialize)]
pub struct ToolsListResult {
    pub tools: Vec<Tool>,
}

/// Tool call request params
#[derive(Debug, Clone, Deserialize)]
pub struct ToolCallParams {
    pub name: String,
    #[serde(default)]
    pub arguments: Option<Value>,
}

/// Tool call response
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallResult {
    pub content: Vec<ToolResultContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

/// Tool result content
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ToolResultContent {
    Text { text: String },
    Image { data: String, mime_type: String },
    Resource { resource: ResourceReference },
}

/// Reference to a resource
#[derive(Debug, Clone, Serialize)]
pub struct ResourceReference {
    pub uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// MCP Resource definition
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Resource {
    pub uri: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// Resources list response
#[derive(Debug, Clone, Serialize)]
pub struct ResourcesListResult {
    pub resources: Vec<Resource>,
}

/// Resource read request params
#[derive(Debug, Clone, Deserialize)]
pub struct ResourceReadParams {
    pub uri: String,
}

/// Resource read response
#[derive(Debug, Clone, Serialize)]
pub struct ResourceReadResult {
    pub contents: Vec<ResourceContent>,
}

/// Resource content
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceContent {
    pub uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blob: Option<String>,
}

/// Ping response
#[derive(Debug, Clone, Serialize)]
pub struct PingResult {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_request() {
        let json = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
        let request: JsonRpcRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.method, "initialize");
        assert_eq!(request.id, Some(Value::Number(1.into())));
    }

    #[test]
    fn test_serialize_response() {
        let response = JsonRpcResponse::success(
            Some(Value::Number(1.into())),
            serde_json::json!({"status": "ok"}),
        );
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"result\""));
    }

    #[test]
    fn test_error_response() {
        let error = JsonRpcError::method_not_found("unknown");
        let response = JsonRpcResponse::error(Some(Value::Number(1.into())), error);
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("-32601"));
    }

    #[test]
    fn test_error_constructor_codes() {
        // Each constructor pins the standard JSON-RPC 2.0 error code.
        assert_eq!(JsonRpcError::parse_error("x").code, -32700);
        assert_eq!(JsonRpcError::invalid_request("x").code, -32600);
        assert_eq!(JsonRpcError::method_not_found("x").code, -32601);
        assert_eq!(JsonRpcError::invalid_params("x").code, -32602);
        assert_eq!(JsonRpcError::internal_error("x").code, -32603);
    }

    #[test]
    fn test_error_constructor_messages_and_data() {
        // parse_error/invalid_request/invalid_params/internal_error pass the
        // message through verbatim; only method_not_found reformats it.
        let e = JsonRpcError::parse_error("bad json");
        assert_eq!(e.message, "bad json");
        assert!(e.data.is_none());
        assert_eq!(JsonRpcError::invalid_request("nope").message, "nope");
        assert_eq!(JsonRpcError::invalid_params("nope").message, "nope");
        assert_eq!(JsonRpcError::internal_error("boom").message, "boom");
        assert_eq!(
            JsonRpcError::method_not_found("foo/bar").message,
            "Method not found: foo/bar"
        );
    }

    #[test]
    fn test_success_response_shape() {
        // success populates result and leaves error None.
        let r = JsonRpcResponse::success(Some(Value::Number(7.into())), serde_json::json!(42));
        assert_eq!(r.jsonrpc, "2.0");
        assert!(r.result.is_some());
        assert!(r.error.is_none());
        assert_eq!(r.id, Some(Value::Number(7.into())));
    }

    #[test]
    fn test_error_response_shape() {
        // error populates error and leaves result None.
        let r = JsonRpcResponse::error(None, JsonRpcError::internal_error("x"));
        assert!(r.result.is_none());
        assert!(r.error.is_some());
    }

    #[test]
    fn test_response_skips_none_fields() {
        // id/result/error all carry skip_serializing_if = Option::is_none.
        let r = JsonRpcResponse::success(None, serde_json::json!({"ok": true}));
        let json = serde_json::to_string(&r).unwrap();
        assert!(!json.contains("\"id\""));
        assert!(!json.contains("\"error\""));
        assert!(json.contains("\"result\""));
    }

    #[test]
    fn test_error_serializes_without_data_when_none() {
        let json = serde_json::to_string(&JsonRpcError::parse_error("x")).unwrap();
        assert!(!json.contains("\"data\""));
        assert!(json.contains("\"code\":-32700"));
    }

    #[test]
    fn test_request_params_default_to_none() {
        // params has #[serde(default)] so a request may omit it entirely.
        let json = r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#;
        let req: JsonRpcRequest = serde_json::from_str(json).unwrap();
        assert!(req.params.is_none());
        assert_eq!(req.method, "ping");
    }

    #[test]
    fn test_request_allows_null_id() {
        // Notification-style id absence deserializes to None.
        let json = r#"{"jsonrpc":"2.0","id":null,"method":"notify"}"#;
        let req: JsonRpcRequest = serde_json::from_str(json).unwrap();
        assert!(req.id.is_none());
    }

    #[test]
    fn test_tool_call_params_arguments_default_none() {
        let params: ToolCallParams =
            serde_json::from_str(r#"{"name":"codegraph_symbol_search"}"#).unwrap();
        assert_eq!(params.name, "codegraph_symbol_search");
        assert!(params.arguments.is_none());
    }

    #[test]
    fn test_initialize_params_all_defaults() {
        // Every field is #[serde(default)], so an empty object is valid.
        let params: InitializeParams = serde_json::from_str("{}").unwrap();
        assert!(params.protocol_version.is_none());
        assert!(params.client_info.is_none());
        assert!(params.roots.is_none());
    }

    #[test]
    fn test_tool_result_content_text_tag() {
        // The enum is internally tagged and lowercased.
        let content = ToolResultContent::Text {
            text: "hello".to_string(),
        };
        let json = serde_json::to_string(&content).unwrap();
        assert!(json.contains("\"type\":\"text\""));
        assert!(json.contains("\"text\":\"hello\""));
    }

    #[test]
    fn test_tool_result_content_image_tag_snake_case_mime() {
        let content = ToolResultContent::Image {
            data: "abc".to_string(),
            mime_type: "image/png".to_string(),
        };
        let json = serde_json::to_string(&content).unwrap();
        assert!(json.contains("\"type\":\"image\""));
        assert!(json.contains("\"mime_type\":\"image/png\""));
    }

    #[test]
    fn test_property_schema_renames_enum_and_type() {
        let schema = PropertySchema {
            property_type: "string".to_string(),
            description: None,
            default: None,
            enum_values: Some(vec!["a".to_string(), "b".to_string()]),
            items: None,
            minimum: None,
            maximum: None,
        };
        let json = serde_json::to_string(&schema).unwrap();
        assert!(json.contains("\"type\":\"string\""));
        assert!(json.contains("\"enum\":[\"a\",\"b\"]"));
        // None-valued optionals are skipped.
        assert!(!json.contains("\"description\""));
        assert!(!json.contains("\"items\""));
    }

    #[test]
    fn test_tool_input_schema_renames_type_and_skips_none() {
        let schema = ToolInputSchema {
            schema_type: "object".to_string(),
            properties: None,
            required: None,
        };
        let json = serde_json::to_string(&schema).unwrap();
        assert!(json.contains("\"type\":\"object\""));
        assert!(!json.contains("\"properties\""));
        assert!(!json.contains("\"required\""));
    }

    #[test]
    fn test_tool_call_result_camel_case_is_error() {
        let result = ToolCallResult {
            content: vec![],
            is_error: Some(true),
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"isError\":true"));
    }

    #[test]
    fn test_initialize_result_camel_case_fields() {
        let result = InitializeResult {
            protocol_version: "2024-11-05".to_string(),
            capabilities: ServerCapabilities {
                experimental: None,
                logging: None,
                prompts: None,
                resources: None,
                tools: Some(ToolsCapability {
                    list_changed: Some(false),
                }),
            },
            server_info: ServerInfo {
                name: "codegraph".to_string(),
                version: Some("1.0".to_string()),
            },
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"protocolVersion\":\"2024-11-05\""));
        assert!(json.contains("\"serverInfo\""));
        assert!(json.contains("\"listChanged\":false"));
    }
}
