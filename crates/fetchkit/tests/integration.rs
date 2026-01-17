//! Integration tests for FetchKit using wiremock

use fetchkit::{fetch, HttpMethod, Tool, FetchRequest};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_simple_get() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("Hello, World!")
                .insert_header("content-type", "text/plain"),
        )
        .mount(&mock_server)
        .await;

    let req = FetchRequest::new(format!("{}/", mock_server.uri()));
    let resp = fetch(req).await.unwrap();

    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.content_type, Some("text/plain".to_string()));
    assert!(resp.content.unwrap().contains("Hello, World!"));
    assert_eq!(resp.format, Some("raw".to_string()));
}

#[tokio::test]
async fn test_head_request() {
    let mock_server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/file.pdf"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/pdf")
                .insert_header("content-length", "12345")
                .insert_header("last-modified", "Tue, 01 Jan 2024 00:00:00 GMT"),
        )
        .mount(&mock_server)
        .await;

    let req =
        FetchRequest::new(format!("{}/file.pdf", mock_server.uri())).method(HttpMethod::Head);
    let resp = fetch(req).await.unwrap();

    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.method, Some("HEAD".to_string()));
    assert_eq!(resp.content_type, Some("application/pdf".to_string()));
    assert_eq!(resp.size, Some(12345));
    assert_eq!(
        resp.last_modified,
        Some("Tue, 01 Jan 2024 00:00:00 GMT".to_string())
    );
    assert!(resp.content.is_none());
}

#[tokio::test]
async fn test_html_to_markdown() {
    let mock_server = MockServer::start().await;

    let html = r#"<!DOCTYPE html>
<html>
<head><title>Test</title></head>
<body>
    <h1>Hello World</h1>
    <p>This is a <strong>test</strong> paragraph.</p>
    <ul>
        <li>Item 1</li>
        <li>Item 2</li>
    </ul>
</body>
</html>"#;

    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(html, "text/html"))
        .mount(&mock_server)
        .await;

    let tool = Tool::default();
    let req = FetchRequest::new(format!("{}/", mock_server.uri())).as_markdown();
    let resp = tool.execute(req).await.unwrap();

    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.format, Some("markdown".to_string()));

    let content = resp.content.unwrap();
    assert!(content.contains("# Hello World"));
    assert!(content.contains("**test**"));
    assert!(content.contains("- Item 1"));
    assert!(content.contains("- Item 2"));
}

#[tokio::test]
async fn test_html_to_text() {
    let mock_server = MockServer::start().await;

    let html = r#"<!DOCTYPE html>
<html>
<body>
    <h1>Title</h1>
    <p>Paragraph text.</p>
    <script>alert('bad');</script>
</body>
</html>"#;

    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(html, "text/html"))
        .mount(&mock_server)
        .await;

    let tool = Tool::default();
    let req = FetchRequest::new(format!("{}/", mock_server.uri())).as_text();
    let resp = tool.execute(req).await.unwrap();

    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.format, Some("text".to_string()));

    let content = resp.content.unwrap();
    assert!(content.contains("Title"));
    assert!(content.contains("Paragraph text"));
    assert!(!content.contains("alert")); // Script should be stripped
}

#[tokio::test]
async fn test_binary_content() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/image.png"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(vec![0x89, 0x50, 0x4E, 0x47]) // PNG magic bytes
                .insert_header("content-type", "image/png")
                .insert_header("content-length", "4"),
        )
        .mount(&mock_server)
        .await;

    let req = FetchRequest::new(format!("{}/image.png", mock_server.uri()));
    let resp = fetch(req).await.unwrap();

    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.content_type, Some("image/png".to_string()));
    assert_eq!(resp.size, Some(4));
    assert!(resp.content.is_none());
    assert!(resp.error.is_some());
    assert!(resp.error.unwrap().contains("Binary content"));
}

#[tokio::test]
async fn test_4xx_status() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/not-found"))
        .respond_with(
            ResponseTemplate::new(404)
                .set_body_string("Not Found")
                .insert_header("content-type", "text/plain"),
        )
        .mount(&mock_server)
        .await;

    let req = FetchRequest::new(format!("{}/not-found", mock_server.uri()));
    let resp = fetch(req).await.unwrap();

    // 4xx is still a success response (not a tool error)
    assert_eq!(resp.status_code, 404);
    assert!(resp.content.unwrap().contains("Not Found"));
}

#[tokio::test]
async fn test_5xx_status() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/error"))
        .respond_with(
            ResponseTemplate::new(500)
                .set_body_string("Internal Server Error")
                .insert_header("content-type", "text/plain"),
        )
        .mount(&mock_server)
        .await;

    let req = FetchRequest::new(format!("{}/error", mock_server.uri()));
    let resp = fetch(req).await.unwrap();

    // 5xx is still a success response (not a tool error)
    assert_eq!(resp.status_code, 500);
    assert!(resp.content.unwrap().contains("Internal Server Error"));
}

#[tokio::test]
async fn test_content_disposition_filename() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/download"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("file content")
                .insert_header("content-type", "text/plain")
                .insert_header("content-disposition", "attachment; filename=\"report.txt\""),
        )
        .mount(&mock_server)
        .await;

    let req = FetchRequest::new(format!("{}/download", mock_server.uri()));
    let resp = fetch(req).await.unwrap();

    assert_eq!(resp.filename, Some("report.txt".to_string()));
}

#[tokio::test]
async fn test_filename_from_url() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/path/to/document.pdf"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/pdf")
                .insert_header("content-length", "100"),
        )
        .mount(&mock_server)
        .await;

    let req = FetchRequest::new(format!("{}/path/to/document.pdf", mock_server.uri()))
        .method(HttpMethod::Head);
    let resp = fetch(req).await.unwrap();

    assert_eq!(resp.filename, Some("document.pdf".to_string()));
}

#[tokio::test]
async fn test_size_for_text_content() {
    let mock_server = MockServer::start().await;

    let body = "Hello, this is test content!";

    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(body)
                .insert_header("content-type", "text/plain"),
        )
        .mount(&mock_server)
        .await;

    let req = FetchRequest::new(format!("{}/", mock_server.uri()));
    let resp = fetch(req).await.unwrap();

    // Size should equal bytes read from body
    assert_eq!(resp.size, Some(body.len() as u64));
}

#[tokio::test]
async fn test_url_prefix_allow_list() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_string("OK"))
        .mount(&mock_server)
        .await;

    // Create tool with allow list that doesn't include the mock server
    let tool = Tool::builder()
        .allow_prefix("https://allowed.example.com")
        .build();

    let req = FetchRequest::new(format!("{}/", mock_server.uri()));
    let result = tool.execute(req).await;

    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("prefix not allowed"));
}

#[tokio::test]
async fn test_url_prefix_block_list() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_string("OK"))
        .mount(&mock_server)
        .await;

    // Create tool with block list that includes localhost
    let tool = Tool::builder().block_prefix("http://127.0.0.1").build();

    let req = FetchRequest::new(format!("{}/", mock_server.uri()));
    let result = tool.execute(req).await;

    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("prefix not allowed"));
}

#[tokio::test]
async fn test_invalid_url_scheme() {
    let req = FetchRequest::new("ftp://example.com/file.txt");
    let result = fetch(req).await;

    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("http:// or https://"));
}

#[tokio::test]
async fn test_missing_url() {
    let req = FetchRequest::new("");
    let result = fetch(req).await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Missing"));
}

#[tokio::test]
async fn test_entity_decoding_in_html() {
    let mock_server = MockServer::start().await;

    let html = "<p>Tom &amp; Jerry &lt;3 &gt; others &quot;quoted&quot;</p>";

    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(html, "text/html"))
        .mount(&mock_server)
        .await;

    let tool = Tool::default();
    let req = FetchRequest::new(format!("{}/", mock_server.uri())).as_text();
    let resp = tool.execute(req).await.unwrap();

    let content = resp.content.unwrap();
    assert!(content.contains("Tom & Jerry"));
    assert!(content.contains("<3"));
    assert!(content.contains("> others"));
    assert!(content.contains("\"quoted\""));
}

#[tokio::test]
async fn test_non_html_with_conversion_flags() {
    let mock_server = MockServer::start().await;

    let json = r#"{"key": "value"}"#;

    Mock::given(method("GET"))
        .and(path("/api/data"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(json)
                .insert_header("content-type", "application/json"),
        )
        .mount(&mock_server)
        .await;

    let tool = Tool::default();
    let req = FetchRequest::new(format!("{}/api/data", mock_server.uri())).as_markdown();
    let resp = tool.execute(req).await.unwrap();

    // Non-HTML should return raw even with as_markdown flag
    assert_eq!(resp.format, Some("raw".to_string()));
    assert!(resp.content.unwrap().contains("\"key\""));
}

#[tokio::test]
async fn test_html_detection_by_body() {
    let mock_server = MockServer::start().await;

    // Server returns HTML without proper content-type
    let html = "<!DOCTYPE html><html><body>Hello</body></html>";

    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(html)
                .insert_header("content-type", "text/plain"), // Wrong content-type
        )
        .mount(&mock_server)
        .await;

    let tool = Tool::default();
    let req = FetchRequest::new(format!("{}/", mock_server.uri())).as_markdown();
    let resp = tool.execute(req).await.unwrap();

    // Should detect HTML by body content and convert
    assert_eq!(resp.format, Some("markdown".to_string()));
}

#[tokio::test]
async fn test_custom_user_agent() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_string("OK"))
        .mount(&mock_server)
        .await;

    let tool = Tool::builder().user_agent("CustomBot/1.0").build();

    let req = FetchRequest::new(format!("{}/", mock_server.uri()));
    let resp = tool.execute(req).await.unwrap();

    assert_eq!(resp.status_code, 200);
}

#[tokio::test]
async fn test_excessive_newlines_filtered() {
    let mock_server = MockServer::start().await;

    let body = "Line1\n\n\n\n\n\nLine2";

    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(body)
                .insert_header("content-type", "text/plain"),
        )
        .mount(&mock_server)
        .await;

    let req = FetchRequest::new(format!("{}/", mock_server.uri()));
    let resp = fetch(req).await.unwrap();

    // Should have at most 2 consecutive newlines
    assert!(!resp.content.unwrap().contains("\n\n\n"));
}
