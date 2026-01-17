//! FetchKit CLI - Command-line interface for fetching web content

mod mcp;

use clap::{Parser, Subcommand};
use fetchkit::{FetchRequest, HttpMethod, Tool, TOOL_LLMTXT};

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
        #[arg(long)]
        url: String,

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
        println!("{}", TOOL_LLMTXT);
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
        Some(Commands::Md { url, user_agent }) => {
            run_md(&url, user_agent).await;
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
            println!("{}", json);
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

async fn run_md(url: &str, user_agent: Option<String>) {
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
        Ok(response) => {
            print_md_with_frontmatter(&response);
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

fn print_md_with_frontmatter(response: &fetchkit::FetchResponse) {
    // Build frontmatter
    println!("---");
    println!("url: {}", response.url);
    println!("status_code: {}", response.status_code);
    if let Some(ref ct) = response.content_type {
        println!("content_type: {}", ct);
    }
    if let Some(size) = response.size {
        println!("size: {}", size);
    }
    if let Some(ref lm) = response.last_modified {
        println!("last_modified: {}", lm);
    }
    if let Some(ref filename) = response.filename {
        println!("filename: {}", filename);
    }
    if let Some(truncated) = response.truncated {
        if truncated {
            println!("truncated: true");
        }
    }
    if let Some(ref err) = response.error {
        println!("error: {}", err);
    }
    println!("---");

    // Print content
    if let Some(ref content) = response.content {
        println!("{}", content);
    }
}
