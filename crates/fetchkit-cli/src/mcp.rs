//! MCP (Model Context Protocol) server implementation

use fetchkit::{FetchRequest, Tool, TOOL_DESCRIPTION};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

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

        // Simplified schema for fetchkit_md (no as_markdown/as_text options)
        let md_input_schema = json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to fetch (required, must be http:// or https://)"
                }
            },
            "required": ["url"]
        });

        JsonRpcResponse::success(
            id,
            json!({
                "tools": [{
                    "name": "fetchkit",
                    "description": TOOL_DESCRIPTION,
                    "inputSchema": input_schema
                }, {
                    "name": "fetchkit_md",
                    "description": "Fetch URL and return markdown with metadata frontmatter. Optimized for LLM consumption.",
                    "inputSchema": md_input_schema
                }]
            }),
        )
    }

    async fn handle_tools_call(&self, id: Option<Value>, params: Value) -> JsonRpcResponse {
        let tool_name = params
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        match tool_name {
            "fetchkit" => self.handle_fetchkit_call(id, params).await,
            "fetchkit_md" => self.handle_fetchkit_md_call(id, params).await,
            _ => JsonRpcResponse::error(id, -32602, format!("Unknown tool: {}", tool_name)),
        }
    }

    async fn handle_fetchkit_call(&self, id: Option<Value>, params: Value) -> JsonRpcResponse {
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

    async fn handle_fetchkit_md_call(&self, id: Option<Value>, params: Value) -> JsonRpcResponse {
        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

        // Extract URL from arguments
        let url = match arguments.get("url").and_then(|v| v.as_str()) {
            Some(u) => u.to_string(),
            None => {
                return JsonRpcResponse::error(id, -32602, "Missing required argument: url");
            }
        };

        // Build request with markdown conversion
        let request = FetchRequest::new(url).as_markdown();

        // Execute tool
        match self.tool.execute(request).await {
            Ok(response) => {
                let output = format_md_with_frontmatter(&response);
                JsonRpcResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": output
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

fn format_md_with_frontmatter(response: &fetchkit::FetchResponse) -> String {
    let mut output = String::new();

    // Build frontmatter
    output.push_str("---\n");
    output.push_str(&format!("url: {}\n", response.url));
    output.push_str(&format!("status_code: {}\n", response.status_code));
    if let Some(ref ct) = response.content_type {
        output.push_str(&format!("content_type: {}\n", ct));
    }
    if let Some(size) = response.size {
        output.push_str(&format!("size: {}\n", size));
    }
    if let Some(ref lm) = response.last_modified {
        output.push_str(&format!("last_modified: {}\n", lm));
    }
    if let Some(ref filename) = response.filename {
        output.push_str(&format!("filename: {}\n", filename));
    }
    if let Some(truncated) = response.truncated {
        if truncated {
            output.push_str("truncated: true\n");
        }
    }
    if let Some(ref err) = response.error {
        output.push_str(&format!("error: {}\n", err));
    }
    output.push_str("---\n");

    // Append content
    if let Some(ref content) = response.content {
        output.push_str(content);
    }

    output
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
