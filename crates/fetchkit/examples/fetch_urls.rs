//! Example: Fetch various URLs and display results
//!
//! Run with: cargo run -p fetchkit --example fetch_urls
//!
//! This example demonstrates the fetcher system with different URL types.

use fetchkit::{fetch, FetchRequest, FetchResponse};

/// Test case definition
struct TestCase {
    url: &'static str,
    description: &'static str,
    expect_format: Option<&'static str>,
    expect_contains: Option<&'static str>,
}

/// Define test cases here
const TEST_CASES: &[TestCase] = &[
    TestCase {
        url: "https://example.com",
        description: "Simple HTML page",
        expect_format: Some("markdown"),
        expect_contains: Some("Example Domain"),
    },
    TestCase {
        url: "https://httpbin.org/json",
        description: "JSON endpoint",
        expect_format: Some("raw"),
        expect_contains: Some("slideshow"),
    },
    TestCase {
        url: "https://httpbin.org/html",
        description: "HTML endpoint",
        expect_format: Some("markdown"),
        expect_contains: Some("Herman Melville"),
    },
    TestCase {
        url: "https://github.com/rust-lang/rust",
        description: "GitHub repository (uses GitHubRepoFetcher)",
        expect_format: Some("github_repo"),
        expect_contains: Some("rust-lang/rust"),
    },
    TestCase {
        url: "https://raw.githubusercontent.com/rust-lang/rust/master/README.md",
        description: "Raw markdown file",
        expect_format: Some("raw"),
        expect_contains: Some("Rust"),
    },
];

#[tokio::main]
async fn main() {
    println!("FetchKit URL Examples");
    println!("=====================\n");

    let mut passed = 0;
    let mut failed = 0;

    for (i, case) in TEST_CASES.iter().enumerate() {
        println!("{}. {}", i + 1, case.description);
        println!("   URL: {}", case.url);

        let request = FetchRequest::new(case.url).as_markdown();

        match fetch(request).await {
            Ok(response) => {
                let check_result = check_expectations(case, &response);
                print_response_summary(&response);

                if check_result {
                    println!("   ✓ PASS\n");
                    passed += 1;
                } else {
                    println!("   ✗ FAIL (expectations not met)\n");
                    failed += 1;
                }
            }
            Err(e) => {
                println!("   Error: {}", e);
                println!("   ✗ FAIL\n");
                failed += 1;
            }
        }
    }

    println!("=====================");
    println!("Results: {} passed, {} failed", passed, failed);

    if failed > 0 {
        std::process::exit(1);
    }
}

fn print_response_summary(response: &FetchResponse) {
    println!("   Status: {}", response.status_code);

    if let Some(ref format) = response.format {
        println!("   Format: {}", format);
    }

    if let Some(ref ct) = response.content_type {
        println!("   Content-Type: {}", ct);
    }

    if let Some(size) = response.size {
        println!("   Size: {} bytes", size);
    }

    if let Some(ref content) = response.content {
        let preview = content.chars().take(100).collect::<String>();
        let preview = preview.replace('\n', " ");
        println!(
            "   Preview: {}{}",
            preview,
            if content.len() > 100 { "..." } else { "" }
        );
    }

    if let Some(ref error) = response.error {
        println!("   Error: {}", error);
    }
}

fn check_expectations(case: &TestCase, response: &FetchResponse) -> bool {
    // Check format
    if let Some(expected_format) = case.expect_format {
        if response.format.as_deref() != Some(expected_format) {
            println!(
                "   Expected format '{}', got '{:?}'",
                expected_format, response.format
            );
            return false;
        }
    }

    // Check content contains
    if let Some(expected_text) = case.expect_contains {
        let content = response.content.as_deref().unwrap_or("");
        if !content.contains(expected_text) {
            println!("   Expected content to contain '{}'", expected_text);
            return false;
        }
    }

    true
}
