//! Fetchkit CLI as a library.
//!
//! The `fetchkit` binary is a thin wrapper around [`run`]. Embedders (e.g. the
//! bashkit `fetchkit` builtin) call the same [`run`] with their own [`CliHost`],
//! so the command line interface (flags, help, version, errors, output) is the
//! same code everywhere.
//!
//! # Usage
//!
//! ```text
//! fetchkit fetch <URL> [--output md|json] [--user-agent <UA>]
//! fetchkit mcp
//! fetchkit --llmtxt
//! ```

// Decisions:
// - All process-level effects go through `CliHost`: stdout/stderr, environment
//   lookups, MCP stdio serving, and outbound policy (`configure_tool`). `run`
//   never calls `std::process::exit`, `println!`, or `std::env`; it returns an
//   exit code. This is what lets a sandbox (bashkit VFS + network allowlist)
//   embed the CLI without the CLI touching the host process.
// - Flags a host cannot honor stay in the parser (help text is identical in
//   every host) and are refused at run time via `CliHost::refuse`, with exit
//   code 1 and a one-line error.
// - clap errors are rendered to a string and routed through the host. Exit
//   codes match `clap::Error::exit`: 0 for --help/--version, 2 for usage errors.

pub mod mcp;

use async_trait::async_trait;
use clap::{Args, Parser, Subcommand, ValueEnum};
use fetchkit::{
    CrawlPage, DocumentConversionSettings, FetchRequest, Tool, ToolBuilder, TOOL_LLMTXT,
};
use std::ffi::OsString;
use std::io::{self, Write};

/// Optional behavior a host may refuse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum HostFeature {
    /// `--allow-env-proxy`: honor HTTP(S)_PROXY / NO_PROXY.
    EnvProxy,
    /// `--render-rakers`: rendered HTML via the rakers JS engine.
    RenderRakers,
    /// `--bot-auth-key` / `--bot-auth-agent`: request signing inside fetchkit.
    BotAuth,
    /// `--allow-remote-conversion`: upload documents to a third-party converter.
    RemoteConversion,
}

impl HostFeature {
    /// The command line flag that requested this feature.
    pub fn flag(self) -> &'static str {
        match self {
            HostFeature::EnvProxy => "--allow-env-proxy",
            HostFeature::RenderRakers => "--render-rakers",
            HostFeature::BotAuth => "--bot-auth-key",
            HostFeature::RemoteConversion => "--allow-remote-conversion",
        }
    }
}

/// Environment the CLI runs in.
///
/// [`NativeHost`] maps this to the real process. Sandboxed hosts map it to
/// their own output buffers, environment, and egress policy.
#[async_trait]
pub trait CliHost: Send + Sync {
    /// Write to standard output.
    fn write_stdout(&self, text: &str) -> io::Result<()>;

    /// Write to standard error.
    fn write_stderr(&self, text: &str);

    /// Look up an environment variable.
    fn env_var(&self, name: &str) -> Option<String>;

    /// Apply host policy to the tool after the CLI flags were applied, e.g. a
    /// custom [`fetchkit::HttpTransport`] or DNS policy.
    fn configure_tool(&self, builder: ToolBuilder) -> ToolBuilder {
        builder
    }

    /// Return `Some(reason)` to refuse a feature requested by a flag.
    fn refuse(&self, feature: HostFeature) -> Option<String> {
        let _ = feature;
        None
    }

    /// Serve the tool over MCP. Hosts without a process stdio refuse.
    async fn serve_mcp(&self, tool: Tool) -> Result<(), String> {
        let _ = tool;
        Err("mcp is not available in this environment".to_string())
    }
}

/// [`CliHost`] for the standalone `fetchkit` binary: real stdio and env.
#[derive(Debug, Default, Clone, Copy)]
pub struct NativeHost;

#[async_trait]
impl CliHost for NativeHost {
    fn write_stdout(&self, text: &str) -> io::Result<()> {
        let stdout = io::stdout();
        let mut handle = stdout.lock();
        handle.write_all(text.as_bytes())?;
        handle.flush()
    }

    fn write_stderr(&self, text: &str) {
        let _ = io::stderr().lock().write_all(text.as_bytes());
    }

    fn env_var(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }

    async fn serve_mcp(&self, tool: Tool) -> Result<(), String> {
        mcp::run_server(tool).await;
        Ok(())
    }
}

/// The clap command tree, for help/argument parity checks in embedders.
pub fn command() -> clap::Command {
    <Cli as clap::CommandFactory>::command()
}

/// Parse `args` (including the program name) and run the command.
///
/// Returns the process exit code.
pub async fn run<I, T>(args: I, host: &dyn CliHost) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(e) => {
            let text = e.render().to_string();
            if e.use_stderr() {
                host.write_stderr(&text);
            } else if let Some(code) = write_out(host, &text) {
                return code;
            }
            return e.exit_code();
        }
    };
    match execute(cli, host).await {
        Ok(()) => 0,
        Err(Exit(code)) => code,
    }
}

/// Early exit carrying an exit code; the message was already written.
struct Exit(i32);

fn fail(host: &dyn CliHost, message: impl std::fmt::Display) -> Exit {
    host.write_stderr(&format!("Error: {message}\n"));
    Exit(1)
}

fn check(host: &dyn CliHost, feature: HostFeature, requested: bool) -> Result<(), Exit> {
    if !requested {
        return Ok(());
    }
    match host.refuse(feature) {
        Some(reason) => Err(fail(host, format!("{} {reason}", feature.flag()))),
        None => Ok(()),
    }
}

/// Write to stdout. Broken pipe ends quietly with 0; other errors with 1.
fn write_out(host: &dyn CliHost, text: &str) -> Option<i32> {
    match host.write_stdout(text) {
        Ok(()) => None,
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Some(0),
        Err(e) => {
            host.write_stderr(&format!("Error writing to stdout: {e}\n"));
            Some(1)
        }
    }
}

fn writeln_out(host: &dyn CliHost, text: &str) -> Result<(), Exit> {
    match write_out(host, &format!("{text}\n")) {
        None => Ok(()),
        Some(code) => Err(Exit(code)),
    }
}

async fn execute(cli: Cli, host: &dyn CliHost) -> Result<(), Exit> {
    if cli.llmtxt {
        return writeln_out(host, &TOOL_LLMTXT);
    }

    match cli.command {
        Some(Commands::Mcp {
            hardened,
            allow_env_proxy,
            bot_auth_key,
            bot_auth_agent,
            documents,
        }) => {
            let tool = build_tool(
                host,
                ToolFlags {
                    user_agent: None,
                    hardened,
                    allow_env_proxy,
                    bot_auth_key,
                    bot_auth_agent,
                    render_rakers: false,
                },
                &documents,
            )?;
            host.serve_mcp(tool).await.map_err(|e| fail(host, e))
        }
        Some(Commands::Fetch {
            url,
            output,
            user_agent,
            hardened,
            allow_env_proxy,
            bot_auth_key,
            bot_auth_agent,
            content_focus,
            crawl,
            max_pages,
            render_rakers,
            documents,
        }) => {
            let flags = ToolFlags {
                user_agent,
                hardened,
                allow_env_proxy,
                bot_auth_key,
                bot_auth_agent,
                render_rakers,
            };
            run_fetch(
                host,
                &url,
                flags,
                &documents,
                output,
                content_focus,
                crawl,
                max_pages,
            )
            .await
        }
        None => {
            host.write_stderr(
                "Usage: fetchkit fetch <URL>\n   or: fetchkit mcp\n   or: fetchkit --help\n",
            );
            Err(Exit(1))
        }
    }
}

/// Tool-shaping flags shared by `fetch` and `mcp`.
struct ToolFlags {
    user_agent: Option<String>,
    hardened: bool,
    allow_env_proxy: bool,
    bot_auth_key: Option<String>,
    bot_auth_agent: Option<String>,
    render_rakers: bool,
}

fn build_tool(
    host: &dyn CliHost,
    flags: ToolFlags,
    documents: &DocumentArgs,
) -> Result<Tool, Exit> {
    check(host, HostFeature::EnvProxy, flags.allow_env_proxy)?;
    check(host, HostFeature::RenderRakers, flags.render_rakers)?;
    check(
        host,
        HostFeature::BotAuth,
        flags.bot_auth_key.is_some() || flags.bot_auth_agent.is_some(),
    )?;
    check(
        host,
        HostFeature::RemoteConversion,
        documents.allow_remote_conversion,
    )?;

    let mut builder = Tool::builder().enable_markdown(true);

    let settings = documents.settings(flags.allow_env_proxy, host);
    if !settings.is_default() {
        let conversion = settings.build().map_err(|e| fail(host, e))?;
        builder = builder.document_conversion(conversion);
    }

    if flags.hardened {
        builder = builder.hardened();
    }

    if flags.allow_env_proxy {
        builder = builder.use_env_proxy(true);
    }

    if let Some(ua) = flags.user_agent {
        builder = builder.user_agent(ua);
    }

    #[cfg(feature = "bot-auth")]
    if let Some(ref key) = flags.bot_auth_key {
        let config = fetchkit::BotAuthConfig::from_base64_seed(key).map_err(|e| fail(host, e))?;
        let config = if let Some(ref fqdn) = flags.bot_auth_agent {
            config.with_agent_fqdn(fqdn)
        } else {
            config
        };
        builder = builder.bot_auth(config);
    }

    #[cfg(not(feature = "bot-auth"))]
    if flags.bot_auth_key.is_some() {
        return Err(fail(
            host,
            "--bot-auth-key requires the bot-auth feature (rebuild with --features bot-auth)",
        ));
    }

    #[cfg(feature = "render-rakers")]
    if flags.render_rakers {
        builder = builder.enable_render_rakers(true);
    }

    #[cfg(not(feature = "render-rakers"))]
    if flags.render_rakers {
        return Err(fail(
            host,
            "--render-rakers requires the render-rakers feature (rebuild with --features render-rakers)",
        ));
    }

    Ok(host.configure_tool(builder).build())
}

#[allow(clippy::too_many_arguments)]
async fn run_fetch(
    host: &dyn CliHost,
    url: &str,
    flags: ToolFlags,
    documents: &DocumentArgs,
    output: OutputFormat,
    content_focus: Option<String>,
    crawl: bool,
    max_pages: usize,
) -> Result<(), Exit> {
    // Build request with markdown conversion
    let mut request = FetchRequest::new(url).as_markdown();
    if let Some(focus) = content_focus {
        request = request.content_focus(focus);
    }
    if crawl {
        request = request.crawl(true).max_pages(max_pages);
    }
    if flags.render_rakers {
        request = request.render_rakers();
    }

    let tool = build_tool(host, flags, documents)?;

    match tool.execute(request).await {
        Ok(response) => match output {
            OutputFormat::Md => writeln_out(host, &format_md_with_frontmatter(&response)),
            OutputFormat::Json => {
                let json = serde_json::to_string_pretty(&response).map_err(|e| {
                    host.write_stderr(&format!("Error serializing response: {e}\n"));
                    Exit(1)
                })?;
                writeln_out(host, &json)
            }
        },
        Err(e) => Err(fail(host, e)),
    }
}

/// Output format for fetch subcommand
#[derive(Debug, Clone, Copy, Default, ValueEnum)]
enum OutputFormat {
    /// Markdown with YAML frontmatter
    #[default]
    Md,
    /// JSON format
    Json,
}

/// Document (PDF, Office) conversion options shared by `fetch` and `mcp`.
#[derive(Args, Debug, Default, Clone)]
struct DocumentArgs {
    /// Document formats to convert, comma-separated (e.g. pdf,docx,xlsx)
    #[arg(long = "convert", value_delimiter = ',')]
    formats: Option<Vec<String>>,

    /// Converter order, comma-separated (pdf-inspector, markkit, office, cloudflare). Cloudflare
    /// reads CLOUDFLARE_ACCOUNT_ID and CLOUDFLARE_API_TOKEN.
    #[arg(long = "converter", value_delimiter = ',')]
    converters: Option<Vec<String>>,

    /// Allow converters that upload documents to a third-party service
    #[arg(long)]
    allow_remote_conversion: bool,

    /// Maximum document download size in bytes (default 25 MB)
    #[arg(long)]
    max_document_bytes: Option<usize>,
}

impl DocumentArgs {
    fn settings(&self, respect_proxy_env: bool, host: &dyn CliHost) -> DocumentConversionSettings {
        DocumentConversionSettings {
            formats: self.formats.clone(),
            converters: self.converters.clone(),
            allow_remote: self.allow_remote_conversion,
            max_document_bytes: self.max_document_bytes,
            cloudflare_account_id: host.env_var("CLOUDFLARE_ACCOUNT_ID"),
            cloudflare_api_token: host.env_var("CLOUDFLARE_API_TOKEN"),
            respect_proxy_env,
        }
    }
}

/// Fetchkit - AI-friendly web content fetching tool
#[derive(Parser, Debug)]
#[command(name = "fetchkit")]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Print full help with examples (llmtxt)
    #[arg(long)]
    llmtxt: bool,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Run as MCP (Model Context Protocol) server over stdio
    Mcp {
        /// Apply the hardened outbound policy profile
        #[arg(long)]
        hardened: bool,

        /// Allow HTTP_PROXY/HTTPS_PROXY/NO_PROXY from the environment
        #[arg(long)]
        allow_env_proxy: bool,

        /// Ed25519 secret key seed (base64url, 32 bytes) for Web Bot Auth signing
        #[arg(long)]
        bot_auth_key: Option<String>,

        /// Agent FQDN for Signature-Agent header (requires --bot-auth-key)
        #[arg(long)]
        bot_auth_agent: Option<String>,

        #[command(flatten)]
        documents: DocumentArgs,
    },
    /// Fetch URL and output as markdown with metadata frontmatter
    Fetch {
        /// URL to fetch
        url: String,

        /// Output format
        #[arg(long, short, default_value = "md")]
        output: OutputFormat,

        /// Custom User-Agent
        #[arg(long)]
        user_agent: Option<String>,

        /// Apply the hardened outbound policy profile
        #[arg(long)]
        hardened: bool,

        /// Allow HTTP_PROXY/HTTPS_PROXY/NO_PROXY from the environment
        #[arg(long)]
        allow_env_proxy: bool,

        /// Ed25519 secret key seed (base64url, 32 bytes) for Web Bot Auth signing
        #[arg(long)]
        bot_auth_key: Option<String>,

        /// Agent FQDN for Signature-Agent header (requires --bot-auth-key)
        #[arg(long)]
        bot_auth_agent: Option<String>,

        /// Extraction focus: full, main, readable, or agent
        #[arg(long)]
        content_focus: Option<String>,

        /// Discover and fetch a bounded set of same-origin pages
        #[arg(long)]
        crawl: bool,

        /// Maximum crawl pages, including the seed
        #[arg(long, default_value_t = 5)]
        max_pages: usize,

        /// Render HTML with the rakers backend before conversion
        #[arg(long)]
        render_rakers: bool,

        #[command(flatten)]
        documents: DocumentArgs,
    },
}

fn yaml_quote(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

/// Format response as markdown with YAML frontmatter
pub fn format_md_with_frontmatter(response: &fetchkit::FetchResponse) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use fetchkit::{CrawlPage, CrawlResult, FetchResponse, PageQuality};

    #[test]
    fn test_format_md_basic() {
        let response = FetchResponse {
            url: "https://example.com".to_string(),
            status_code: 200,
            content_type: Some("text/html".to_string()),
            content: Some("# Hello World".to_string()),
            ..Default::default()
        };

        let output = format_md_with_frontmatter(&response);

        assert!(output.starts_with("---\n"));
        assert!(output.contains("url: \"https://example.com\"\n"));
        assert!(output.contains("status_code: 200\n"));
        assert!(output.contains("source_content_type: \"text/html\"\n"));
        assert!(output.contains("---\n# Hello World"));
    }

    #[test]
    fn test_format_md_with_all_fields() {
        let response = FetchResponse {
            url: "https://example.com/page".to_string(),
            status_code: 200,
            content_type: Some("text/html".to_string()),
            size: Some(1234),
            last_modified: Some("Wed, 01 Jan 2025 00:00:00 GMT".to_string()),
            filename: Some("page.html".to_string()),
            truncated: Some(true),
            quality: Some(PageQuality {
                score: 0.72,
                warnings: vec!["low_content".to_string()],
                extraction_method: Some("agent_main".to_string()),
                suggested_next_action: Some("retry_with_agent_focus_or_crawl".to_string()),
                ..Default::default()
            }),
            content: Some("Content here".to_string()),
            ..Default::default()
        };

        let output = format_md_with_frontmatter(&response);

        assert!(output.contains("source_size: 1234\n"));
        assert!(output.contains("last_modified: \"Wed, 01 Jan 2025 00:00:00 GMT\"\n"));
        assert!(output.contains("filename: \"page.html\"\n"));
        assert!(output.contains("truncated: true\n"));
        assert!(output.contains("quality_score: 0.72\n"));
        assert!(output.contains("quality_warnings: [\"low_content\"]\n"));
        assert!(output.contains("extraction_method: \"agent_main\"\n"));
        assert!(output.contains("suggested_next_action: \"retry_with_agent_focus_or_crawl\"\n"));
    }

    #[test]
    fn test_format_md_error_as_body() {
        let response = FetchResponse {
            url: "https://example.com/file.pdf".to_string(),
            status_code: 200,
            content_type: Some("application/pdf".to_string()),
            error: Some("Binary content not supported".to_string()),
            ..Default::default()
        };

        let output = format_md_with_frontmatter(&response);

        // Error should appear as body, not in frontmatter
        assert!(!output.contains("error:"));
        assert!(output.ends_with("---\nBinary content not supported"));
    }

    #[test]
    fn test_format_md_truncated_false_omitted() {
        let response = FetchResponse {
            url: "https://example.com".to_string(),
            status_code: 200,
            truncated: Some(false),
            content: Some("Content".to_string()),
            ..Default::default()
        };

        let output = format_md_with_frontmatter(&response);

        // truncated: false should not appear
        assert!(!output.contains("truncated"));
    }

    #[test]
    fn test_format_md_quotes_untrusted_scalars() {
        let response = FetchResponse {
            url: "https://example.com/a\nforged: true".to_string(),
            status_code: 200,
            filename: Some("*alias".to_string()),
            content: Some("ok".to_string()),
            ..Default::default()
        };

        let output = format_md_with_frontmatter(&response);

        assert!(output.contains("url: \"https://example.com/a\\nforged: true\"\n"));
        assert!(output.contains("filename: \"*alias\"\n"));
        assert!(!output.contains("\nforged: true\n"));
    }

    #[test]
    fn test_format_md_includes_crawl_summary() {
        let response = FetchResponse {
            url: "https://example.com".to_string(),
            status_code: 200,
            content: Some("# Home".to_string()),
            crawl: Some(CrawlResult {
                seed_url: "https://example.com".to_string(),
                max_pages: 2,
                pages: vec![CrawlPage {
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
