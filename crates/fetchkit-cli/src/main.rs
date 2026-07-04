//! FetchKit CLI - Command-line interface for fetching web content
//!
//! Provides the `fetchkit` binary with subcommands for fetching URLs
//! and running an MCP server.
//!
//! # Usage
//!
//! ```text
//! fetchkit fetch <URL> [--output md|json] [--user-agent <UA>]
//! fetchkit mcp
//! fetchkit --llmtxt
//! ```

mod mcp;

use clap::{Parser, Subcommand, ValueEnum};
use fetchkit::{CrawlPage, FetchRequest, Tool, TOOL_LLMTXT};
use std::io::{self, Write};

/// Output format for fetch subcommand
#[derive(Debug, Clone, Copy, Default, ValueEnum)]
enum OutputFormat {
    /// Markdown with YAML frontmatter
    #[default]
    Md,
    /// JSON format
    Json,
}

/// FetchKit - AI-friendly web content fetching tool
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
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // Handle --llmtxt flag
    if cli.llmtxt {
        writeln_safe(&TOOL_LLMTXT);
        std::process::exit(0);
    }

    match cli.command {
        Some(Commands::Mcp {
            hardened,
            allow_env_proxy,
            bot_auth_key,
            bot_auth_agent,
        }) => {
            mcp::run_server(build_tool(
                None,
                hardened,
                allow_env_proxy,
                bot_auth_key,
                bot_auth_agent,
                false,
            ))
            .await;
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
        }) => {
            let options = FetchCommandOptions {
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
            };
            run_fetch(&url, options).await;
        }
        None => {
            eprintln!("Usage: fetchkit fetch <URL>");
            eprintln!("   or: fetchkit mcp");
            eprintln!("   or: fetchkit --help");
            std::process::exit(1);
        }
    }
}

fn build_tool(
    user_agent: Option<String>,
    hardened: bool,
    allow_env_proxy: bool,
    bot_auth_key: Option<String>,
    bot_auth_agent: Option<String>,
    render_rakers: bool,
) -> Tool {
    let mut builder = Tool::builder().enable_markdown(true);

    if hardened {
        builder = builder.hardened();
    }

    if allow_env_proxy {
        builder = builder.use_env_proxy(true);
    }

    if let Some(ua) = user_agent {
        builder = builder.user_agent(ua);
    }

    #[cfg(feature = "bot-auth")]
    if let Some(ref key) = bot_auth_key {
        let config = fetchkit::BotAuthConfig::from_base64_seed(key).unwrap_or_else(|e| {
            eprintln!("Error: {e}");
            std::process::exit(1);
        });
        let config = if let Some(ref fqdn) = bot_auth_agent {
            config.with_agent_fqdn(fqdn)
        } else {
            config
        };
        builder = builder.bot_auth(config);
    }

    #[cfg(not(feature = "bot-auth"))]
    if bot_auth_key.is_some() {
        eprintln!("Error: --bot-auth-key requires the bot-auth feature (rebuild with --features bot-auth)");
        std::process::exit(1);
    }

    let _ = bot_auth_agent; // suppress unused warning without feature

    #[cfg(feature = "render-rakers")]
    if render_rakers {
        builder = builder.enable_render_rakers(true);
    }

    #[cfg(not(feature = "render-rakers"))]
    if render_rakers {
        eprintln!("Error: --render-rakers requires the render-rakers feature (rebuild with --features render-rakers)");
        std::process::exit(1);
    }

    builder.build()
}

struct FetchCommandOptions {
    output: OutputFormat,
    user_agent: Option<String>,
    hardened: bool,
    allow_env_proxy: bool,
    bot_auth_key: Option<String>,
    bot_auth_agent: Option<String>,
    content_focus: Option<String>,
    crawl: bool,
    max_pages: usize,
    render_rakers: bool,
}

async fn run_fetch(url: &str, options: FetchCommandOptions) {
    // Build request with markdown conversion
    let mut request = FetchRequest::new(url).as_markdown();
    if let Some(focus) = options.content_focus {
        request = request.content_focus(focus);
    }
    if options.crawl {
        request = request.crawl(true).max_pages(options.max_pages);
    }
    if options.render_rakers {
        request = request.render_rakers();
    }
    let tool = build_tool(
        options.user_agent,
        options.hardened,
        options.allow_env_proxy,
        options.bot_auth_key,
        options.bot_auth_agent,
        options.render_rakers,
    );

    // Execute request
    match tool.execute(request).await {
        Ok(response) => match options.output {
            OutputFormat::Md => print_md_with_frontmatter(&response),
            OutputFormat::Json => {
                let json = serde_json::to_string_pretty(&response).unwrap_or_else(|e| {
                    eprintln!("Error serializing response: {}", e);
                    std::process::exit(1);
                });
                writeln_safe(&json);
            }
        },
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

fn print_md_with_frontmatter(response: &fetchkit::FetchResponse) {
    writeln_safe(&format_md_with_frontmatter(response));
}

fn yaml_quote(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

/// Format response as markdown with YAML frontmatter
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

/// Write to stdout, exit silently on broken pipe
fn writeln_safe(s: &str) {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    if let Err(e) = writeln!(handle, "{}", s) {
        if e.kind() == io::ErrorKind::BrokenPipe {
            std::process::exit(0);
        }
        eprintln!("Error writing to stdout: {}", e);
        std::process::exit(1);
    }
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
