//! Example: Fetch various URLs and display results
//!
//! Run with: cargo run -p fetchkit --example fetch_urls
//!
//! Demonstrates the fetcher system with different content types using a local
//! wiremock server (no external network required).

use fetchkit::{fetch_with_options, DnsPolicy, FetchOptions, FetchRequest, FetchResponse};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Test case definition
struct TestCase {
    path: &'static str,
    description: &'static str,
    expect_format: Option<&'static str>,
    expect_contains: Option<&'static str>,
}

#[tokio::main]
async fn main() {
    println!("FetchKit URL Examples");
    println!("=====================\n");

    let mock_server = MockServer::start().await;

    // Mount mock endpoints
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(
                    "<html><body><h1>Example Domain</h1>\
                     <p>This domain is for use in illustrative examples.</p></body></html>",
                )
                .insert_header("content-type", "text/html; charset=utf-8"),
        )
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/json"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(r#"{"slideshow": {"title": "Sample Slide Show", "slides": []}}"#)
                .insert_header("content-type", "application/json"),
        )
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/html"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(
                    "<html><body><h1>Herman Melville - Moby Dick</h1>\
                     <p>Call me Ishmael.</p></body></html>",
                )
                .insert_header("content-type", "text/html; charset=utf-8"),
        )
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/readme"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("# Rust Programming Language\n\nA systems programming language.")
                .insert_header("content-type", "text/plain; charset=utf-8"),
        )
        .mount(&mock_server)
        .await;

    let base = mock_server.uri();

    let test_cases: Vec<TestCase> = vec![
        TestCase {
            path: "/",
            description: "Simple HTML page",
            expect_format: Some("markdown"),
            expect_contains: Some("Example Domain"),
        },
        TestCase {
            path: "/json",
            description: "JSON endpoint",
            expect_format: Some("raw"),
            expect_contains: Some("slideshow"),
        },
        TestCase {
            path: "/html",
            description: "HTML endpoint",
            expect_format: Some("markdown"),
            expect_contains: Some("Herman Melville"),
        },
        TestCase {
            path: "/readme",
            description: "Raw text file",
            expect_format: Some("raw"),
            expect_contains: Some("Rust"),
        },
    ];

    let options = FetchOptions {
        enable_markdown: true,
        enable_text: true,
        dns_policy: DnsPolicy::allow_all(), // Allow loopback for mock server
        ..Default::default()
    };

    let mut passed = 0;
    let mut failed = 0;

    for (i, case) in test_cases.iter().enumerate() {
        let url = format!("{}{}", base, case.path);
        println!("{}. {}", i + 1, case.description);
        println!("   URL: {}", url);

        let request = FetchRequest::new(&url).as_markdown();

        match fetch_with_options(request, options.clone()).await {
            Ok(response) => {
                let check_result = check_expectations(case, &response);
                print_response_summary(&response);

                if check_result {
                    println!("   PASS\n");
                    passed += 1;
                } else {
                    println!("   FAIL (expectations not met)\n");
                    failed += 1;
                }
            }
            Err(e) => {
                println!("   Error: {}", e);
                println!("   FAIL\n");
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
