//! Tool builder and contract for FetchKit

use crate::client::{fetch_with_options, FetchOptions};
use crate::dns::DnsPolicy;
use crate::error::FetchError;
use crate::fetchers::FetcherRegistry;
use crate::file_saver::FileSaver;
use crate::types::{FetchRequest, FetchResponse};
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

/// Builder for configuring the FetchKit tool
///
/// # Examples
///
/// ```
/// use fetchkit::ToolBuilder;
///
/// let tool = ToolBuilder::new()
///     .enable_markdown(true)
///     .enable_text(false)
///     .user_agent("MyBot/1.0")
///     .allow_prefix("https://docs.example.com")
///     .block_prefix("https://internal.example.com")
///     .build();
///
/// assert!(!tool.description().is_empty());
/// ```
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
    /// DNS resolution policy for SSRF prevention
    dns_policy: DnsPolicy,
    /// Maximum response body size in bytes
    max_body_size: Option<usize>,
    /// Enable save_to_file parameter (opt-in)
    enable_save_to_file: bool,
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

    /// Set maximum response body size in bytes
    ///
    /// Limits the amount of data read from responses. Protects against
    /// memory exhaustion from large responses and compressed content bombs.
    /// Default: 10 MB if not set.
    pub fn max_body_size(mut self, size: usize) -> Self {
        self.max_body_size = Some(size);
        self
    }

    /// Enable file download (save_to_file parameter).
    /// Disabled by default — opt-in only.
    pub fn enable_save_to_file(mut self, enable: bool) -> Self {
        self.enable_save_to_file = enable;
        self
    }

    /// Control private/reserved IP range blocking (SSRF prevention)
    ///
    /// Enabled by default. When enabled, FetchKit resolves hostnames to IP
    /// addresses before connecting and validates that the resolved IP is not
    /// in a private or reserved range. DNS pinning prevents rebinding attacks.
    ///
    /// Pass `false` only for local development or testing against loopback
    /// servers. In production, always leave this enabled.
    pub fn block_private_ips(mut self, block: bool) -> Self {
        self.dns_policy = if block {
            DnsPolicy::block_private_ips()
        } else {
            DnsPolicy::allow_all()
        };
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
            dns_policy: self.dns_policy,
            max_body_size: self.max_body_size,
            enable_save_to_file: self.enable_save_to_file,
        }
    }
}

/// Configured FetchKit tool
///
/// Created via [`ToolBuilder`]. Provides methods for executing fetch requests,
/// retrieving schemas, and accessing tool metadata.
///
/// # Examples
///
/// ```no_run
/// use fetchkit::{FetchRequest, Tool};
///
/// # async fn example() -> Result<(), fetchkit::FetchError> {
/// let tool = Tool::default();
/// let response = tool.execute(FetchRequest::new("https://example.com")).await?;
/// println!("Status: {}", response.status_code);
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct Tool {
    enable_markdown: bool,
    enable_text: bool,
    user_agent: Option<String>,
    allow_prefixes: Vec<String>,
    block_prefixes: Vec<String>,
    dns_policy: DnsPolicy,
    max_body_size: Option<usize>,
    enable_save_to_file: bool,
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
        let schema = schema_for!(FetchRequest);
        let mut value = serde_json::to_value(schema).unwrap_or_default();

        // Remove disabled options from schema
        if let Some(props) = value.get_mut("properties").and_then(|p| p.as_object_mut()) {
            if !self.enable_markdown {
                props.remove("as_markdown");
            }
            if !self.enable_text {
                props.remove("as_text");
            }
            if !self.enable_save_to_file {
                props.remove("save_to_file");
            }
        }

        value
    }

    /// Get output schema as JSON
    pub fn output_schema(&self) -> serde_json::Value {
        let schema = schema_for!(FetchResponse);
        serde_json::to_value(schema).unwrap_or_default()
    }

    /// Execute the tool with the given request
    pub async fn execute(&self, req: FetchRequest) -> Result<FetchResponse, FetchError> {
        fetch_with_options(req, self.build_options()).await
    }

    /// Execute the tool with status updates
    pub async fn execute_with_status<F>(
        &self,
        req: FetchRequest,
        mut status_callback: F,
    ) -> Result<FetchResponse, FetchError>
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

        status_callback(ToolStatus::new("fetch").with_percent(20.0));

        let result = fetch_with_options(req, self.build_options()).await;

        status_callback(ToolStatus::new("complete").with_percent(100.0));

        result
    }

    /// Build FetchOptions from this Tool's configuration
    fn build_options(&self) -> FetchOptions {
        FetchOptions {
            user_agent: self.user_agent.clone(),
            allow_prefixes: self.allow_prefixes.clone(),
            block_prefixes: self.block_prefixes.clone(),
            enable_markdown: self.enable_markdown,
            enable_text: self.enable_text,
            dns_policy: self.dns_policy.clone(),
            max_body_size: self.max_body_size,
            enable_save_to_file: self.enable_save_to_file,
        }
    }

    /// Execute fetch with optional file saving.
    ///
    /// When `req.save_to_file` is set, validates the path via the saver,
    /// fetches content (including binary), and saves through the saver.
    /// Returns metadata without inline content.
    ///
    /// When `req.save_to_file` is `None`, behaves identically to [`execute`](Self::execute).
    pub async fn execute_with_saver(
        &self,
        req: FetchRequest,
        saver: Option<&dyn FileSaver>,
    ) -> Result<FetchResponse, FetchError> {
        if let Some(path) = &req.save_to_file {
            if !self.enable_save_to_file {
                return Err(FetchError::SaverNotAvailable);
            }

            let saver = saver.ok_or(FetchError::SaverNotAvailable)?;

            // Validate path before making HTTP request
            saver
                .validate_path(path)
                .await
                .map_err(|e| FetchError::SaveError(e.to_string()))?;

            let options = self.build_options();
            let registry = FetcherRegistry::with_defaults();
            registry.fetch_to_file(req, options, saver).await
        } else {
            self.execute(req).await
        }
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
        // Safe by default: private IPs blocked
        assert!(tool.dns_policy.block_private);
    }

    #[test]
    fn test_tool_builder_opt_out_private_ip_blocking() {
        let tool = Tool::builder().block_private_ips(false).build();
        assert!(!tool.dns_policy.block_private);
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
