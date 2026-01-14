//! Tool builder and contract for WebFetch

use crate::client::{fetch_with_options, FetchOptions};
use crate::error::FetchError;
use crate::types::{WebFetchRequest, WebFetchResponse};
use crate::{TOOL_DESCRIPTION, TOOL_LLMTXT};
use schemars::schema_for;
use serde::{Deserialize, Serialize};

/// Status update during tool execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolStatus {
    /// Current phase (e.g., "validate", "connect", "fetch", "convert")
    pub phase: String,
    /// Optional message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Estimated completion percentage (0-100)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent_complete: Option<f32>,
    /// Estimated time remaining in milliseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eta_ms: Option<u64>,
}

impl ToolStatus {
    /// Create a new status with phase
    pub fn new(phase: impl Into<String>) -> Self {
        Self {
            phase: phase.into(),
            message: None,
            percent_complete: None,
            eta_ms: None,
        }
    }

    /// Set message
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }

    /// Set completion percentage
    pub fn with_percent(mut self, percent: f32) -> Self {
        self.percent_complete = Some(percent);
        self
    }

    /// Set ETA
    pub fn with_eta(mut self, eta_ms: u64) -> Self {
        self.eta_ms = Some(eta_ms);
        self
    }
}

/// Builder for configuring the WebFetch tool
#[derive(Debug, Clone, Default)]
pub struct ToolBuilder {
    /// Enable as_markdown option
    enable_markdown: bool,
    /// Enable as_text option
    enable_text: bool,
    /// Custom User-Agent
    user_agent: Option<String>,
    /// Allow list of URL prefixes
    allow_prefixes: Vec<String>,
    /// Block list of URL prefixes
    block_prefixes: Vec<String>,
}

impl ToolBuilder {
    /// Create a new tool builder with all options enabled
    pub fn new() -> Self {
        Self {
            enable_markdown: true,
            enable_text: true,
            ..Default::default()
        }
    }

    /// Enable as_markdown option
    pub fn enable_markdown(mut self, enable: bool) -> Self {
        self.enable_markdown = enable;
        self
    }

    /// Enable as_text option
    pub fn enable_text(mut self, enable: bool) -> Self {
        self.enable_text = enable;
        self
    }

    /// Set custom User-Agent
    pub fn user_agent(mut self, ua: impl Into<String>) -> Self {
        self.user_agent = Some(ua.into());
        self
    }

    /// Add URL prefix to allow list
    pub fn allow_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.allow_prefixes.push(prefix.into());
        self
    }

    /// Add URL prefix to block list
    pub fn block_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.block_prefixes.push(prefix.into());
        self
    }

    /// Build the tool
    pub fn build(self) -> Tool {
        Tool {
            enable_markdown: self.enable_markdown,
            enable_text: self.enable_text,
            user_agent: self.user_agent,
            allow_prefixes: self.allow_prefixes,
            block_prefixes: self.block_prefixes,
        }
    }
}

/// Configured WebFetch tool
#[derive(Debug, Clone)]
pub struct Tool {
    enable_markdown: bool,
    enable_text: bool,
    user_agent: Option<String>,
    allow_prefixes: Vec<String>,
    block_prefixes: Vec<String>,
}

impl Default for Tool {
    fn default() -> Self {
        ToolBuilder::new().build()
    }
}

impl Tool {
    /// Create a new tool builder
    pub fn builder() -> ToolBuilder {
        ToolBuilder::new()
    }

    /// Get tool description
    pub fn description(&self) -> &'static str {
        TOOL_DESCRIPTION
    }

    /// Get system prompt (empty for this tool)
    pub fn system_prompt(&self) -> &'static str {
        ""
    }

    /// Get full documentation (llmtxt)
    pub fn llmtxt(&self) -> &'static str {
        TOOL_LLMTXT
    }

    /// Get input schema as JSON
    pub fn input_schema(&self) -> serde_json::Value {
        let schema = schema_for!(WebFetchRequest);
        let mut value = serde_json::to_value(schema).unwrap_or_default();

        // Remove disabled options from schema
        if let Some(props) = value.get_mut("properties").and_then(|p| p.as_object_mut()) {
            if !self.enable_markdown {
                props.remove("as_markdown");
            }
            if !self.enable_text {
                props.remove("as_text");
            }
        }

        value
    }

    /// Get output schema as JSON
    pub fn output_schema(&self) -> serde_json::Value {
        let schema = schema_for!(WebFetchResponse);
        serde_json::to_value(schema).unwrap_or_default()
    }

    /// Execute the tool with the given request
    pub async fn execute(&self, req: WebFetchRequest) -> Result<WebFetchResponse, FetchError> {
        let options = FetchOptions {
            user_agent: self.user_agent.clone(),
            allow_prefixes: self.allow_prefixes.clone(),
            block_prefixes: self.block_prefixes.clone(),
            enable_markdown: self.enable_markdown,
            enable_text: self.enable_text,
        };

        fetch_with_options(req, options).await
    }

    /// Execute the tool with status updates
    pub async fn execute_with_status<F>(
        &self,
        req: WebFetchRequest,
        mut status_callback: F,
    ) -> Result<WebFetchResponse, FetchError>
    where
        F: FnMut(ToolStatus),
    {
        status_callback(ToolStatus::new("validate").with_percent(0.0));

        // Validate request
        if req.url.is_empty() {
            return Err(FetchError::MissingUrl);
        }

        if !req.url.starts_with("http://") && !req.url.starts_with("https://") {
            return Err(FetchError::InvalidUrlScheme);
        }

        status_callback(ToolStatus::new("connect").with_percent(10.0));

        let options = FetchOptions {
            user_agent: self.user_agent.clone(),
            allow_prefixes: self.allow_prefixes.clone(),
            block_prefixes: self.block_prefixes.clone(),
            enable_markdown: self.enable_markdown,
            enable_text: self.enable_text,
        };

        status_callback(ToolStatus::new("fetch").with_percent(20.0));

        let result = fetch_with_options(req, options).await;

        status_callback(ToolStatus::new("complete").with_percent(100.0));

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_builder() {
        let tool = Tool::builder()
            .enable_markdown(false)
            .enable_text(true)
            .user_agent("TestAgent/1.0")
            .allow_prefix("https://allowed.com")
            .block_prefix("https://blocked.com")
            .build();

        assert!(!tool.enable_markdown);
        assert!(tool.enable_text);
        assert_eq!(tool.user_agent, Some("TestAgent/1.0".to_string()));
        assert_eq!(tool.allow_prefixes, vec!["https://allowed.com"]);
        assert_eq!(tool.block_prefixes, vec!["https://blocked.com"]);
    }

    #[test]
    fn test_tool_description() {
        let tool = Tool::default();
        assert!(!tool.description().is_empty());
        assert!(tool.system_prompt().is_empty());
        assert!(!tool.llmtxt().is_empty());
    }

    #[test]
    fn test_tool_schemas() {
        let tool = Tool::default();
        let input_schema = tool.input_schema();
        let output_schema = tool.output_schema();

        // Input schema should have url property
        assert!(input_schema["properties"]["url"].is_object());

        // Output schema should have url and status_code
        assert!(output_schema["properties"]["url"].is_object());
        assert!(output_schema["properties"]["status_code"].is_object());
    }

    #[test]
    fn test_tool_schema_feature_gating() {
        let tool = Tool::builder()
            .enable_markdown(false)
            .enable_text(false)
            .build();

        let schema = tool.input_schema();

        // Disabled options should be removed from schema
        if let Some(props) = schema.get("properties").and_then(|p| p.as_object()) {
            assert!(!props.contains_key("as_markdown"));
            assert!(!props.contains_key("as_text"));
        }
    }

    #[test]
    fn test_tool_status() {
        let status = ToolStatus::new("fetch")
            .with_message("Fetching URL")
            .with_percent(50.0)
            .with_eta(5000);

        assert_eq!(status.phase, "fetch");
        assert_eq!(status.message, Some("Fetching URL".to_string()));
        assert_eq!(status.percent_complete, Some(50.0));
        assert_eq!(status.eta_ms, Some(5000));
    }
}
