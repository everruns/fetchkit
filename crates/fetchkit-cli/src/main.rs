//! FetchKit CLI - Command-line interface for fetching web content

mod mcp;

use clap::{Parser, Subcommand, ValueEnum};
use fetchkit::{FetchRequest, HttpMethod, Tool, TOOL_LLMTXT};
use std::io::{self, Write};

/// Output format for md subcommand
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

    /// URL to fetch (for direct fetch mode)
    #[arg(long)]
    url: Option<String>,

    /// HTTP method (GET or HEAD)
    #[arg(long, default_value = "GET")]
    method: String,

    /// Convert HTML to markdown
    #[arg(long)]
    as_markdown: bool,

    /// Convert HTML to plain text
    #[arg(long)]
    as_text: bool,

    /// Custom User-Agent
    #[arg(long)]
    user_agent: Option<String>,

    /// Print full help with examples (llmtxt)
    #[arg(long)]
    llmtxt: bool,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Run as MCP (Model Context Protocol) server over stdio
    Mcp,
    /// Fetch a URL (default command)
    Fetch {
        /// URL to fetch
        #[arg(long)]
        url: String,

        /// HTTP method (GET or HEAD)
        #[arg(long, default_value = "GET")]
        method: String,

        /// Convert HTML to markdown
        #[arg(long)]
        as_markdown: bool,

        /// Convert HTML to plain text
        #[arg(long)]
        as_text: bool,

        /// Custom User-Agent
        #[arg(long)]
        user_agent: Option<String>,
    },
    /// Fetch URL and output as markdown with metadata frontmatter
    Md {
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
            method,
            as_markdown,
            as_text,
            user_agent,
        }) => {
            run_fetch(&url, &method, as_markdown, as_text, user_agent).await;
        }
        Some(Commands::Md {
            url,
            output,
            user_agent,
        }) => {
            run_md(&url, output, user_agent).await;
        }
        None => {
            // Default: fetch mode if URL is provided
            if let Some(url) = cli.url {
                run_fetch(
                    &url,
                    &cli.method,
                    cli.as_markdown,
                    cli.as_text,
                    cli.user_agent,
                )
                .await;
            } else {
                eprintln!("Error: Missing required parameter: url");
                eprintln!("Usage: fetchkit --url <URL>");
                eprintln!("   or: fetchkit fetch --url <URL>");
                eprintln!("   or: fetchkit mcp");
                std::process::exit(1);
            }
        }
    }
}

async fn run_fetch(
    url: &str,
    method: &str,
    as_markdown: bool,
    as_text: bool,
    user_agent: Option<String>,
) {
    // Parse method
    let method = match method.to_uppercase().as_str() {
        "GET" => HttpMethod::Get,
        "HEAD" => HttpMethod::Head,
        _ => {
            eprintln!("Error: Invalid method: must be GET or HEAD");
            std::process::exit(1);
        }
    };

    // Build request
    let mut request = FetchRequest::new(url).method(method);

    if as_markdown {
        request = request.as_markdown();
    }
    if as_text {
        request = request.as_text();
    }

    // Build tool
    let mut builder = Tool::builder().enable_markdown(true).enable_text(true);

    if let Some(ua) = user_agent {
        builder = builder.user_agent(ua);
    }

    let tool = builder.build();

    // Execute request
    match tool.execute(request).await {
        Ok(response) => {
            let json = serde_json::to_string_pretty(&response).unwrap_or_else(|e| {
                eprintln!("Error serializing response: {}", e);
                std::process::exit(1);
            });
            writeln_safe(&json);
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

async fn run_md(url: &str, output: OutputFormat, user_agent: Option<String>) {
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
    // Build frontmatter
    writeln_safe("---");
    writeln_safe(&format!("url: {}", response.url));
    writeln_safe(&format!("status_code: {}", response.status_code));
    if let Some(ref ct) = response.content_type {
        writeln_safe(&format!("source_content_type: {}", ct));
    }
    if let Some(size) = response.size {
        writeln_safe(&format!("source_size: {}", size));
    }
    if let Some(ref lm) = response.last_modified {
        writeln_safe(&format!("last_modified: {}", lm));
    }
    if let Some(ref filename) = response.filename {
        writeln_safe(&format!("filename: {}", filename));
    }
    if let Some(truncated) = response.truncated {
        if truncated {
            writeln_safe("truncated: true");
        }
    }
    writeln_safe("---");

    // Print content, or error as body for unsupported content
    if let Some(ref content) = response.content {
        writeln_safe(content);
    } else if let Some(ref err) = response.error {
        writeln_safe(err);
    }
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
