//! FetchKit CLI - Command-line interface for fetching web content

mod mcp;

use clap::{Parser, Subcommand, ValueEnum};
use fetchkit::{FetchRequest, Tool, TOOL_LLMTXT};
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
    Mcp,
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
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // Handle --llmtxt flag
    if cli.llmtxt {
        writeln_safe(TOOL_LLMTXT);
        std::process::exit(0);
    }

    match cli.command {
        Some(Commands::Mcp) => {
            mcp::run_server().await;
        }
        Some(Commands::Fetch {
            url,
            output,
            user_agent,
        }) => {
            run_fetch(&url, output, user_agent).await;
        }
        None => {
            eprintln!("Usage: fetchkit fetch <URL>");
            eprintln!("   or: fetchkit mcp");
            eprintln!("   or: fetchkit --help");
            std::process::exit(1);
        }
    }
}

async fn run_fetch(url: &str, output: OutputFormat, user_agent: Option<String>) {
    // Build request with markdown conversion
    let request = FetchRequest::new(url).as_markdown();

    // Build tool
    let mut builder = Tool::builder().enable_markdown(true);

    if let Some(ua) = user_agent {
        builder = builder.user_agent(ua);
    }

    let tool = builder.build();

    // Execute request
    match tool.execute(request).await {
        Ok(response) => match output {
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

/// Format response as markdown with YAML frontmatter
fn format_md_with_frontmatter(response: &fetchkit::FetchResponse) -> String {
    let mut output = String::new();

    // Build frontmatter
    output.push_str("---\n");
    output.push_str(&format!("url: {}\n", response.url));
    output.push_str(&format!("status_code: {}\n", response.status_code));
    if let Some(ref ct) = response.content_type {
        output.push_str(&format!("source_content_type: {}\n", ct));
    }
    if let Some(size) = response.size {
        output.push_str(&format!("source_size: {}\n", size));
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
    output.push_str("---\n");

    // Append content, or error as body for unsupported content
    if let Some(ref content) = response.content {
        output.push_str(content);
    } else if let Some(ref err) = response.error {
        output.push_str(err);
    }

    output
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
    use fetchkit::FetchResponse;

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
        assert!(output.contains("url: https://example.com\n"));
        assert!(output.contains("status_code: 200\n"));
        assert!(output.contains("source_content_type: text/html\n"));
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
            content: Some("Content here".to_string()),
            ..Default::default()
        };

        let output = format_md_with_frontmatter(&response);

        assert!(output.contains("source_size: 1234\n"));
        assert!(output.contains("last_modified: Wed, 01 Jan 2025 00:00:00 GMT\n"));
        assert!(output.contains("filename: page.html\n"));
        assert!(output.contains("truncated: true\n"));
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
}
