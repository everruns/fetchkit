//! SSRF security tests for FetchKit
//!
//! Tests that validate the resolve-then-check DNS policy prevents
//! server-side request forgery attacks. These tests verify the threat
//! mitigations documented in specs/threat-model.md.
//!
//! Safe-by-default: Tool::default() and fetch() block private IPs.
//! Tests that need loopback (wiremock) must explicitly opt out.

use fetchkit::{FetchError, FetchRequest, Tool};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ============================================================================
// TM-SSRF-001: Private IP access via URL (blocked by default)
// ============================================================================

#[tokio::test]
async fn test_ssrf_001_loopback_ipv4_blocked() {
    let tool = Tool::default();
    let req = FetchRequest::new("http://127.0.0.1/");
    let result = tool.execute(req).await;
    assert!(matches!(result, Err(FetchError::BlockedUrl)));
}

#[tokio::test]
async fn test_ssrf_001_loopback_ipv4_alt_blocked() {
    let tool = Tool::default();
    let req = FetchRequest::new("http://127.0.0.2/");
    let result = tool.execute(req).await;
    assert!(matches!(result, Err(FetchError::BlockedUrl)));
}

#[tokio::test]
async fn test_ssrf_001_private_10_blocked() {
    let tool = Tool::default();
    let req = FetchRequest::new("http://10.0.0.1/");
    let result = tool.execute(req).await;
    assert!(matches!(result, Err(FetchError::BlockedUrl)));
}

#[tokio::test]
async fn test_ssrf_001_private_172_blocked() {
    let tool = Tool::default();
    let req = FetchRequest::new("http://172.16.0.1/");
    let result = tool.execute(req).await;
    assert!(matches!(result, Err(FetchError::BlockedUrl)));
}

#[tokio::test]
async fn test_ssrf_001_private_192_168_blocked() {
    let tool = Tool::default();
    let req = FetchRequest::new("http://192.168.1.1/");
    let result = tool.execute(req).await;
    assert!(matches!(result, Err(FetchError::BlockedUrl)));
}

// ============================================================================
// TM-SSRF-002: Loopback access
// ============================================================================

#[tokio::test]
async fn test_ssrf_002_localhost_blocked() {
    let tool = Tool::default();
    let req = FetchRequest::new("http://localhost/");
    let result = tool.execute(req).await;
    // localhost resolves to 127.0.0.1, which is blocked
    assert!(matches!(result, Err(FetchError::BlockedUrl)));
}

// ============================================================================
// TM-SSRF-003: Cloud metadata endpoint
// ============================================================================

#[tokio::test]
async fn test_ssrf_003_cloud_metadata_blocked() {
    let tool = Tool::default();
    let req = FetchRequest::new("http://169.254.169.254/latest/meta-data/");
    let result = tool.execute(req).await;
    assert!(matches!(result, Err(FetchError::BlockedUrl)));
}

#[tokio::test]
async fn test_ssrf_003_link_local_blocked() {
    let tool = Tool::default();
    let req = FetchRequest::new("http://169.254.0.1/");
    let result = tool.execute(req).await;
    assert!(matches!(result, Err(FetchError::BlockedUrl)));
}

// ============================================================================
// TM-SSRF-006: IPv6-mapped IPv4
// ============================================================================

#[tokio::test]
async fn test_ssrf_006_ipv6_loopback_blocked() {
    let tool = Tool::default();
    let req = FetchRequest::new("http://[::1]/");
    let result = tool.execute(req).await;
    assert!(matches!(result, Err(FetchError::BlockedUrl)));
}

// ============================================================================
// TM-INPUT-001: Non-HTTP scheme
// ============================================================================

#[tokio::test]
async fn test_input_001_file_scheme_blocked() {
    let tool = Tool::default();
    let req = FetchRequest::new("file:///etc/passwd");
    let result = tool.execute(req).await;
    assert!(matches!(result, Err(FetchError::InvalidUrlScheme)));
}

#[tokio::test]
async fn test_input_001_ftp_scheme_blocked() {
    let tool = Tool::default();
    let req = FetchRequest::new("ftp://internal-server/files");
    let result = tool.execute(req).await;
    assert!(matches!(result, Err(FetchError::InvalidUrlScheme)));
}

#[tokio::test]
async fn test_input_001_data_scheme_blocked() {
    let tool = Tool::default();
    let req = FetchRequest::new("data:text/html,<h1>XSS</h1>");
    let result = tool.execute(req).await;
    assert!(matches!(result, Err(FetchError::InvalidUrlScheme)));
}

#[tokio::test]
async fn test_input_001_gopher_scheme_blocked() {
    let tool = Tool::default();
    let req = FetchRequest::new("gopher://internal:70/");
    let result = tool.execute(req).await;
    assert!(matches!(result, Err(FetchError::InvalidUrlScheme)));
}

// ============================================================================
// Default blocks private IPs, explicit opt-out allows them
// ============================================================================

#[tokio::test]
async fn test_default_blocks_loopback_mock_server() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("Hello from loopback")
                .insert_header("content-type", "text/plain"),
        )
        .mount(&mock_server)
        .await;

    // Default tool blocks loopback — mock server is on 127.0.0.1
    let tool = Tool::default();
    let req = FetchRequest::new(format!("{}/", mock_server.uri()));
    let result = tool.execute(req).await;
    assert!(matches!(result, Err(FetchError::BlockedUrl)));
}

#[tokio::test]
async fn test_explicit_opt_out_allows_loopback() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("Hello")
                .insert_header("content-type", "text/plain"),
        )
        .mount(&mock_server)
        .await;

    // Explicit opt-out allows loopback
    let tool = Tool::builder().block_private_ips(false).build();
    let req = FetchRequest::new(format!("{}/", mock_server.uri()));
    let result = tool.execute(req).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().status_code, 200);
}

// ============================================================================
// Prefix-based blocking still works alongside DNS policy
// ============================================================================

#[tokio::test]
async fn test_prefix_block_and_dns_policy_combined() {
    let tool = Tool::builder()
        .block_prefix("https://blocked.example.com")
        .build();

    // URL prefix blocked
    let req = FetchRequest::new("https://blocked.example.com/secret");
    let result = tool.execute(req).await;
    assert!(matches!(result, Err(FetchError::BlockedUrl)));

    // Private IP blocked (by default dns policy)
    let req = FetchRequest::new("http://10.0.0.1/");
    let result = tool.execute(req).await;
    assert!(matches!(result, Err(FetchError::BlockedUrl)));
}

// ============================================================================
// TM-SSRF-004: Numeric IP variants
// ============================================================================

#[tokio::test]
async fn test_ssrf_004_zero_ip_blocked() {
    let tool = Tool::default();
    let req = FetchRequest::new("http://0.0.0.0/");
    let result = tool.execute(req).await;
    assert!(matches!(result, Err(FetchError::BlockedUrl)));
}

// ============================================================================
// TM-CONV-001: Script injection in converted content
// (Uses wiremock on loopback, so must opt out of private IP blocking)
// ============================================================================

#[tokio::test]
async fn test_conv_001_script_stripped_in_markdown() {
    let mock_server = MockServer::start().await;

    let html = r#"<html><body>
        <p>Hello</p>
        <script>alert('xss')</script>
        <p>World</p>
    </body></html>"#;

    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(html, "text/html"))
        .mount(&mock_server)
        .await;

    let tool = Tool::builder().block_private_ips(false).build();
    let req = FetchRequest::new(format!("{}/", mock_server.uri())).as_markdown();
    let resp = tool.execute(req).await.unwrap();

    let content = resp.content.unwrap();
    assert!(!content.contains("alert"));
    assert!(!content.contains("<script>"));
    assert!(content.contains("Hello"));
    assert!(content.contains("World"));
}

#[tokio::test]
async fn test_conv_001_script_stripped_in_text() {
    let mock_server = MockServer::start().await;

    let html = r#"<html><body>
        <p>Safe content</p>
        <script>document.cookie</script>
        <style>.hidden{display:none}</style>
    </body></html>"#;

    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(html, "text/html"))
        .mount(&mock_server)
        .await;

    let tool = Tool::builder().block_private_ips(false).build();
    let req = FetchRequest::new(format!("{}/", mock_server.uri())).as_text();
    let resp = tool.execute(req).await.unwrap();

    let content = resp.content.unwrap();
    assert!(!content.contains("document.cookie"));
    assert!(!content.contains("display:none"));
    assert!(content.contains("Safe content"));
}
