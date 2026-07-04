//! MCP (Model Context Protocol) server implementation

use fetchkit::{CrawlPage, Tool};
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
    if let Some(ref quality) = response.quality {
        output.push_str(&format!("quality_score: {:.2}\n", quality.score));
        if !quality.warnings.is_empty() {
            let warnings =
                serde_json::to_string(&quality.warnings).unwrap_or_else(|_| "[]".to_string());
            output.push_str(&format!("quality_warnings: {}\n", warnings));
        }
        if let Some(ref method) = quality.extraction_method {
            output.push_str(&format!("extraction_method: {}\n", yaml_quote(method)));
        }
        if let Some(ref action) = quality.suggested_next_action {
            output.push_str(&format!("suggested_next_action: {}\n", yaml_quote(action)));
        }
    }
    if let Some(ref crawl) = response.crawl {
        output.push_str(&format!("crawl_pages: {}\n", crawl.pages.len()));
        if crawl.truncated.unwrap_or(false) {
            output.push_str("crawl_truncated: true\n");
        }
    }
    output.push_str("---\n");

    // Append content, or error as body for unsupported content
    if let Some(ref content) = response.content {
        output.push_str(content);
    } else if let Some(ref err) = response.error {
        output.push_str(err);
    }
    append_crawl_summary(&mut output, response);

    output
}

fn append_crawl_summary(output: &mut String, response: &fetchkit::FetchResponse) {
    let Some(ref crawl) = response.crawl else {
        return;
    };
    if crawl.pages.is_empty() {
        return;
    }

    output.push_str("\n\n## Crawl Discovery\n\n");
    for page in &crawl.pages {
        output.push_str(&format!("- {}\n", format_crawl_page(page)));
    }
}

fn format_crawl_page(page: &CrawlPage) -> String {
    let title = page.title.as_deref().unwrap_or(page.url.as_str());
    let mut summary = format!("[{}]({})", title.replace(['[', ']'], ""), page.url);
    if let Some(status_code) = page.status_code {
        summary.push_str(&format!(" - status {status_code}"));
    }
    if let Some(score) = page.quality_score {
        summary.push_str(&format!(", quality {score:.2}"));
    }
    if let Some(word_count) = page.word_count {
        summary.push_str(&format!(", {word_count} words"));
    }
    if let Some(ref error) = page.error {
        summary.push_str(&format!(", error: {error}"));
    }
    summary
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_md_includes_quality_frontmatter() {
        let response = fetchkit::FetchResponse {
            url: "https://example.com".to_string(),
            status_code: 200,
            quality: Some(fetchkit::PageQuality {
                score: 0.4,
                warnings: vec!["low_content".to_string()],
                extraction_method: Some("agent_main".to_string()),
                suggested_next_action: Some("retry_with_agent_focus_or_crawl".to_string()),
                ..Default::default()
            }),
            content: Some("short".to_string()),
            ..Default::default()
        };

        let output = format_md_with_frontmatter(&response);

        assert!(output.contains("quality_score: 0.40\n"));
        assert!(output.contains("quality_warnings: [\"low_content\"]\n"));
        assert!(output.contains("extraction_method: \"agent_main\"\n"));
        assert!(output.contains("suggested_next_action: \"retry_with_agent_focus_or_crawl\"\n"));
    }

    #[test]
    fn test_format_md_includes_crawl_summary() {
        let response = fetchkit::FetchResponse {
            url: "https://example.com".to_string(),
            status_code: 200,
            content: Some("# Home".to_string()),
            crawl: Some(fetchkit::CrawlResult {
                seed_url: "https://example.com".to_string(),
                max_pages: 2,
                pages: vec![fetchkit::CrawlPage {
                    url: "https://example.com/docs".to_string(),
                    status_code: Some(200),
                    title: Some("Docs".to_string()),
                    word_count: Some(42),
                    quality_score: Some(0.91),
                    ..Default::default()
                }],
                truncated: Some(true),
            }),
            ..Default::default()
        };

        let output = format_md_with_frontmatter(&response);

        assert!(output.contains("crawl_pages: 1\n"));
        assert!(output.contains("crawl_truncated: true\n"));
        assert!(output.contains("## Crawl Discovery"));
        assert!(output
            .contains("[Docs](https://example.com/docs) - status 200, quality 0.91, 42 words"));
    }
}
