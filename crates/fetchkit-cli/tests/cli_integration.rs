//! CLI integration tests for fetchkit-cli
//!
//! Tests the CLI binary end-to-end by running it as a subprocess.
//! Uses real HTTP requests to https://example.com.

use std::io::Write;
use std::process::{Command, Stdio};

/// Path to the built binary — `cargo test` puts it in target/debug
fn fetchkit_bin() -> String {
    // cargo sets this env var for integration tests
    env!("CARGO_BIN_EXE_fetchkit").to_string()
}

// ============================================================================
// fetch subcommand — markdown output
// ============================================================================

#[test]
#[ignore] // requires network
fn test_fetch_markdown_output() {
    let output = Command::new(fetchkit_bin())
        .args(["fetch", "https://example.com"])
        .output()
        .expect("failed to run fetchkit");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Should have YAML frontmatter
    assert!(
        stdout.starts_with("---\n"),
        "Expected YAML frontmatter, got: {}",
        &stdout[..80.min(stdout.len())]
    );
    assert!(stdout.contains("url: \"https://example.com"));
    assert!(stdout.contains("status_code: 200"));
    assert!(stdout.contains("source_content_type:"));

    // Should have markdown content after frontmatter
    let parts: Vec<&str> = stdout.splitn(3, "---\n").collect();
    assert!(parts.len() >= 3, "Expected frontmatter delimiters");
    let body = parts[2];
    assert!(!body.is_empty(), "Body should not be empty");
}

// ============================================================================
// fetch subcommand — JSON output
// ============================================================================

#[test]
#[ignore] // requires network
fn test_fetch_json_output() {
    let output = Command::new(fetchkit_bin())
        .args(["fetch", "https://example.com", "--output", "json"])
        .output()
        .expect("failed to run fetchkit");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Should be valid JSON
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("Output should be valid JSON");

    assert_eq!(parsed["status_code"], 200);
    assert_eq!(parsed["url"], "https://example.com/");
    assert!(parsed["content"].is_string());
    assert!(parsed["format"].is_string());
}

// ============================================================================
// fetch subcommand — invalid URL
// ============================================================================

#[test]
fn test_fetch_invalid_scheme() {
    let output = Command::new(fetchkit_bin())
        .args(["fetch", "ftp://example.com"])
        .output()
        .expect("failed to run fetchkit");

    assert!(!output.status.success(), "Should exit with error");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("http://") || stderr.contains("https://"),
        "Should mention valid schemes, got: {}",
        stderr
    );
}

// ============================================================================
// --llmtxt flag
// ============================================================================

#[test]
fn test_llmtxt_flag() {
    let output = Command::new(fetchkit_bin())
        .args(["--llmtxt"])
        .output()
        .expect("failed to run fetchkit");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("# Web Fetch"));
    assert!(stdout.contains("## Parameters"));
    assert!(stdout.contains("**Name:** `web_fetch`"));
}

// ============================================================================
// No arguments — usage message
// ============================================================================

#[test]
fn test_no_args_shows_usage() {
    let output = Command::new(fetchkit_bin())
        .output()
        .expect("failed to run fetchkit");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Usage:") || stderr.contains("fetchkit"),
        "Should show usage, got: {}",
        stderr
    );
}

// ============================================================================
// --help flag
// ============================================================================

#[test]
fn test_help_flag() {
    let output = Command::new(fetchkit_bin())
        .args(["--help"])
        .output()
        .expect("failed to run fetchkit");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("fetchkit") || stdout.contains("FetchKit"));
    assert!(stdout.contains("fetch") || stdout.contains("mcp"));
}

#[test]
fn test_fetch_help_lists_hardening_flags() {
    let output = Command::new(fetchkit_bin())
        .args(["fetch", "--help"])
        .output()
        .expect("failed to run fetchkit");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("--hardened"));
    assert!(stdout.contains("--allow-env-proxy"));
    assert!(stdout.contains("--content-focus"));
    assert!(stdout.contains("--crawl"));
    assert!(stdout.contains("--max-pages"));
}

#[test]
fn test_mcp_help_lists_hardening_flags() {
    let output = Command::new(fetchkit_bin())
        .args(["mcp", "--help"])
        .output()
        .expect("failed to run fetchkit");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(stdout.contains("--hardened"));
    assert!(stdout.contains("--allow-env-proxy"));
}

// ============================================================================
// --version flag
// ============================================================================

#[test]
fn test_version_flag() {
    let output = Command::new(fetchkit_bin())
        .args(["--version"])
        .output()
        .expect("failed to run fetchkit");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    // Version string should contain the package version
    assert!(
        stdout.contains("fetchkit"),
        "Expected version string, got: {}",
        stdout
    );
}

// ============================================================================
// MCP server — initialize handshake
// ============================================================================

#[test]
fn test_mcp_initialize() {
    let mut child = Command::new(fetchkit_bin())
        .args(["mcp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start MCP server");

    let stdin = child.stdin.as_mut().unwrap();

    // Send initialize request
    let init_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "test", "version": "0.1" }
        }
    });
    writeln!(stdin, "{}", serde_json::to_string(&init_req).unwrap()).unwrap();

    // Send tools/list request
    let list_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    });
    writeln!(stdin, "{}", serde_json::to_string(&list_req).unwrap()).unwrap();

    // Close stdin to let the server finish
    drop(child.stdin.take());

    let output = child.wait_with_output().expect("failed to wait");
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Parse responses (one per line)
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(
        lines.len() >= 2,
        "Expected at least 2 responses, got {}: {}",
        lines.len(),
        stdout
    );

    // Check initialize response
    let init_resp: serde_json::Value =
        serde_json::from_str(lines[0]).expect("First line should be valid JSON");
    assert_eq!(init_resp["id"], 1);
    assert!(init_resp["result"]["protocolVersion"].is_string());
    assert!(init_resp["result"]["serverInfo"]["name"].as_str() == Some("fetchkit"));

    // Check tools/list response
    let list_resp: serde_json::Value =
        serde_json::from_str(lines[1]).expect("Second line should be valid JSON");
    assert_eq!(list_resp["id"], 2);
    let tools = list_resp["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"], "web_fetch");
    assert!(tools[0]["inputSchema"].is_object());
}

// ============================================================================
// MCP server — unknown method
// ============================================================================

#[test]
fn test_mcp_unknown_method() {
    let mut child = Command::new(fetchkit_bin())
        .args(["mcp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start MCP server");

    let stdin = child.stdin.as_mut().unwrap();

    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "unknown/method",
        "params": {}
    });
    writeln!(stdin, "{}", serde_json::to_string(&req).unwrap()).unwrap();

    drop(child.stdin.take());

    let output = child.wait_with_output().expect("failed to wait");
    let stdout = String::from_utf8_lossy(&output.stdout);

    let resp: serde_json::Value =
        serde_json::from_str(stdout.lines().next().unwrap()).expect("Should be valid JSON");
    assert!(resp["error"].is_object());
    assert_eq!(resp["error"]["code"], -32601);
}
