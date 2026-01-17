//! MCP (Model Context Protocol) server implementation

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use fetchkit::{Tool, FetchRequest, TOOL_DESCRIPTION};

/// JSON-RPC 2.0 request
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

/// JSON-RPC 2.0 response
#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

/// JSON-RPC 2.0 error
#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

impl JsonRpcResponse {
    fn success(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    fn error(id: Option<Value>, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }
}

/// MCP Server implementation
struct McpServer {
    tool: Tool,
}

impl McpServer {
    fn new() -> Self {
        Self {
            tool: Tool::default(),
        }
    }

    async fn handle_request(&self, request: JsonRpcRequest) -> JsonRpcResponse {
        match request.method.as_str() {
            "initialize" => self.handle_initialize(request.id),
            "tools/list" => self.handle_tools_list(request.id),
            "tools/call" => self.handle_tools_call(request.id, request.params).await,
            "notifications/initialized" => {
                // This is a notification, no response needed
                JsonRpcResponse::success(request.id, json!(null))
            }
            _ => JsonRpcResponse::error(
                request.id,
                -32601,
                format!("Method not found: {}", request.method),
            ),
        }
    }

    fn handle_initialize(&self, id: Option<Value>) -> JsonRpcResponse {
        JsonRpcResponse::success(
            id,
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": "fetchkit",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }),
        )
    }

    fn handle_tools_list(&self, id: Option<Value>) -> JsonRpcResponse {
        let input_schema = self.tool.input_schema();

        JsonRpcResponse::success(
            id,
            json!({
                "tools": [{
                    "name": "fetchkit",
                    "description": TOOL_DESCRIPTION,
                    "inputSchema": input_schema
                }]
            }),
        )
    }

    async fn handle_tools_call(&self, id: Option<Value>, params: Value) -> JsonRpcResponse {
        let tool_name = params
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        if tool_name != "fetchkit" {
            return JsonRpcResponse::error(id, -32602, format!("Unknown tool: {}", tool_name));
        }

        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

        // Parse request
        let request: FetchRequest = match serde_json::from_value(arguments) {
            Ok(req) => req,
            Err(e) => {
                return JsonRpcResponse::error(id, -32602, format!("Invalid arguments: {}", e));
            }
        };

        // Execute tool
        match self.tool.execute(request).await {
            Ok(response) => {
                let content = serde_json::to_value(&response).unwrap_or(json!({}));
                JsonRpcResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": serde_json::to_string_pretty(&content).unwrap_or_default()
                        }]
                    }),
                )
            }
            Err(e) => JsonRpcResponse::success(
                id,
                json!({
                    "content": [{
                        "type": "text",
                        "text": format!("Error: {}", e)
                    }],
                    "isError": true
                }),
            ),
        }
    }
}

/// Run the MCP server over stdio
pub async fn run_server() {
    let server = McpServer::new();
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                eprintln!("Error reading stdin: {}", e);
                continue;
            }
        };

        if line.is_empty() {
            continue;
        }

        let request: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(req) => req,
            Err(e) => {
                let response = JsonRpcResponse::error(None, -32700, format!("Parse error: {}", e));
                let json = serde_json::to_string(&response).unwrap_or_default();
                let _ = writeln!(stdout, "{}", json);
                let _ = stdout.flush();
                continue;
            }
        };

        // Skip notifications (no id)
        if request.id.is_none() && request.method.starts_with("notifications/") {
            continue;
        }

        let response = server.handle_request(request).await;
        let json = serde_json::to_string(&response).unwrap_or_default();
        let _ = writeln!(stdout, "{}", json);
        let _ = stdout.flush();
    }
}
