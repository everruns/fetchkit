//! MCP (Model Context Protocol) server implementation

use fetchkit::Tool;
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
    fn new(tool: Tool) -> Self {
        Self { tool }
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
                    "name": self.tool.name(),
                    "description": self.tool.description(),
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

        if tool_name != self.tool.name() {
            return JsonRpcResponse::error(id, -32602, format!("Unknown tool: {}", tool_name));
        }

        self.handle_tool_call(id, params).await
    }

    async fn handle_tool_call(&self, id: Option<Value>, params: Value) -> JsonRpcResponse {
        let mut arguments = params.get("arguments").cloned().unwrap_or(json!({}));
        if let Some(object) = arguments.as_object_mut() {
            let wants_head = object
                .get("method")
                .and_then(|value| value.as_str())
                .is_some_and(|method| method.eq_ignore_ascii_case("HEAD"));
            let has_output_mode = object.contains_key("as_markdown")
                || object.contains_key("as_text")
                || object.contains_key("save_to_file");

            if !wants_head && !has_output_mode {
                object.insert("as_markdown".to_string(), json!(true));
            }
        }

        let execution = match self.tool.execution(arguments) {
            Ok(execution) => execution,
            Err(err) => {
                return JsonRpcResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": format!("Error: {}", err)
                        }],
                        "isError": true
                    }),
                );
            }
        };

        match execution.execute().await {
            Ok(output) => {
                let response = serde_json::from_value(output.result).unwrap_or_default();
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
    output.push_str(&format!("url: {}\n", yaml_quote(&response.url)));
    output.push_str(&format!("status_code: {}\n", response.status_code));
    if let Some(ref ct) = response.content_type {
        output.push_str(&format!("source_content_type: {}\n", yaml_quote(ct)));
    }
    if let Some(size) = response.size {
        output.push_str(&format!("source_size: {}\n", size));
    }
    if let Some(ref lm) = response.last_modified {
        output.push_str(&format!("last_modified: {}\n", yaml_quote(lm)));
    }
    if let Some(ref filename) = response.filename {
        output.push_str(&format!("filename: {}\n", yaml_quote(filename)));
    }
    if let Some(truncated) = response.truncated {
        if truncated {
            output.push_str("truncated: true\n");
        }
    }
    output.push_str("---\n");

    // Append content, or error as body for unsupported content
    if let Some(ref content) = response.content {
        output.push_str(content);
    } else if let Some(ref err) = response.error {
        output.push_str(err);
    }

    output
}

fn yaml_quote(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

/// Run the MCP server over stdio
pub async fn run_server(tool: Tool) {
    let server = McpServer::new(tool);
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
