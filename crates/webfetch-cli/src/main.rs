//! WebFetch CLI - Command-line interface for fetching web content

use clap::Parser;
use webfetch::{HttpMethod, Tool, WebFetchRequest, TOOL_LLMTXT};

/// WebFetch - AI-friendly web content fetching tool
#[derive(Parser, Debug)]
#[command(name = "webfetch")]
#[command(author, version, about, long_about = None)]
struct Args {
    /// URL to fetch (required)
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

#[tokio::main]
async fn main() {
    let args = Args::parse();

    // Handle --llmtxt flag
    if args.llmtxt {
        println!("{}", TOOL_LLMTXT);
        std::process::exit(0);
    }

    // Require URL
    let url = match args.url {
        Some(url) => url,
        None => {
            eprintln!("Error: Missing required parameter: url");
            eprintln!("Usage: webfetch --url <URL>");
            std::process::exit(1);
        }
    };

    // Parse method
    let method = match args.method.to_uppercase().as_str() {
        "GET" => HttpMethod::Get,
        "HEAD" => HttpMethod::Head,
        _ => {
            eprintln!("Error: Invalid method: must be GET or HEAD");
            std::process::exit(1);
        }
    };

    // Build request
    let mut request = WebFetchRequest::new(&url).method(method);

    if args.as_markdown {
        request = request.as_markdown();
    }
    if args.as_text {
        request = request.as_text();
    }

    // Build tool
    let mut builder = Tool::builder().enable_markdown(true).enable_text(true);

    if let Some(ua) = args.user_agent {
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
