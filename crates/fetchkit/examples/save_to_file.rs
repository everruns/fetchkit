//! Example: Download files using save_to_file
//!
//! Run with: cargo run -p fetchkit --example save_to_file
//!
//! Demonstrates the FileSaver trait and save_to_file feature using
//! a local wiremock server (no external network required).

use fetchkit::{FetchRequest, LocalFileSaver, Tool};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::main]
async fn main() {
    println!("FetchKit save_to_file Example");
    println!("==============================\n");

    let mock_server = MockServer::start().await;

    // Mount mock endpoints
    Mock::given(method("GET"))
        .and(path("/report.json"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(r#"{"status": "ok", "items": [1, 2, 3]}"#)
                .insert_header("content-type", "application/json"),
        )
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/image.png"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(vec![
                    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG header
                    0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR chunk
                ])
                .insert_header("content-type", "image/png")
                .insert_header("content-disposition", "attachment; filename=\"test.png\""),
        )
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/large.txt"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("x".repeat(10_000))
                .insert_header("content-type", "text/plain"),
        )
        .mount(&mock_server)
        .await;

    // Set up save directory and tool
    let dir = tempfile::tempdir().expect("Failed to create temp dir");
    let saver = LocalFileSaver::new(Some(dir.path().to_path_buf()));
    let tool = Tool::builder()
        .block_private_ips(false) // Allow loopback for mock server
        .enable_save_to_file(true)
        .build();

    let mut passed = 0;
    let mut failed = 0;

    // 1. Save text/JSON file
    println!("1. Save JSON file");
    let req = FetchRequest::new(format!("{}/report.json", mock_server.uri()))
        .save_to_file("downloads/report.json");
    match tool.execute_with_saver(req, Some(&saver)).await {
        Ok(resp) => {
            println!("   Status: {}", resp.status_code);
            println!("   Saved to: {:?}", resp.saved_path);
            println!("   Bytes written: {:?}", resp.bytes_written);
            assert_eq!(resp.status_code, 200);
            assert!(resp.saved_path.is_some());
            assert!(resp.content.is_none(), "No inline content when saving");
            let on_disk = std::fs::read_to_string(dir.path().join("downloads/report.json"))
                .expect("File should exist on disk");
            assert!(on_disk.contains("\"status\": \"ok\""));
            println!("   PASS\n");
            passed += 1;
        }
        Err(e) => {
            println!("   Error: {e}\n   FAIL\n");
            failed += 1;
        }
    }

    // 2. Save binary file (PNG)
    println!("2. Save binary file (PNG)");
    let req = FetchRequest::new(format!("{}/image.png", mock_server.uri()))
        .save_to_file("downloads/image.png");
    match tool.execute_with_saver(req, Some(&saver)).await {
        Ok(resp) => {
            println!("   Status: {}", resp.status_code);
            println!("   Saved to: {:?}", resp.saved_path);
            println!("   Bytes written: {:?}", resp.bytes_written);
            assert_eq!(resp.status_code, 200);
            assert_eq!(resp.bytes_written, Some(16));
            let bytes =
                std::fs::read(dir.path().join("downloads/image.png")).expect("File should exist");
            assert_eq!(bytes[0..4], [0x89, 0x50, 0x4E, 0x47]); // PNG magic
            println!("   PASS\n");
            passed += 1;
        }
        Err(e) => {
            println!("   Error: {e}\n   FAIL\n");
            failed += 1;
        }
    }

    // 3. Save larger file
    println!("3. Save larger text file (10KB)");
    let req = FetchRequest::new(format!("{}/large.txt", mock_server.uri()))
        .save_to_file("downloads/large.txt");
    match tool.execute_with_saver(req, Some(&saver)).await {
        Ok(resp) => {
            println!("   Status: {}", resp.status_code);
            println!("   Bytes written: {:?}", resp.bytes_written);
            assert_eq!(resp.bytes_written, Some(10_000));
            println!("   PASS\n");
            passed += 1;
        }
        Err(e) => {
            println!("   Error: {e}\n   FAIL\n");
            failed += 1;
        }
    }

    // 4. Path traversal rejection
    println!("4. Path traversal rejection (safety check)");
    let req = FetchRequest::new(format!("{}/report.json", mock_server.uri()))
        .save_to_file("../../etc/passwd");
    match tool.execute_with_saver(req, Some(&saver)).await {
        Err(e) => {
            println!("   Correctly rejected: {e}");
            println!("   PASS\n");
            passed += 1;
        }
        Ok(_) => {
            println!("   Should have been rejected!\n   FAIL\n");
            failed += 1;
        }
    }

    // 5. No saver provided
    println!("5. No saver provided (safety check)");
    let req =
        FetchRequest::new(format!("{}/report.json", mock_server.uri())).save_to_file("file.txt");
    match tool.execute_with_saver(req, None).await {
        Err(e) => {
            println!("   Correctly rejected: {e}");
            println!("   PASS\n");
            passed += 1;
        }
        Ok(_) => {
            println!("   Should have been rejected!\n   FAIL\n");
            failed += 1;
        }
    }

    // 6. Feature disabled
    println!("6. Feature disabled (safety check)");
    let disabled_tool = Tool::builder().block_private_ips(false).build();
    let req =
        FetchRequest::new(format!("{}/report.json", mock_server.uri())).save_to_file("file.txt");
    match disabled_tool.execute_with_saver(req, Some(&saver)).await {
        Err(e) => {
            println!("   Correctly rejected: {e}");
            println!("   PASS\n");
            passed += 1;
        }
        Ok(_) => {
            println!("   Should have been rejected!\n   FAIL\n");
            failed += 1;
        }
    }

    // 7. Normal fetch still works via execute_with_saver
    println!("7. Normal fetch (no save_to_file) still works");
    let req = FetchRequest::new(format!("{}/report.json", mock_server.uri()));
    match tool.execute_with_saver(req, Some(&saver)).await {
        Ok(resp) => {
            println!("   Status: {}", resp.status_code);
            assert!(resp.content.is_some());
            assert!(resp.saved_path.is_none());
            println!("   PASS\n");
            passed += 1;
        }
        Err(e) => {
            println!("   Error: {e}\n   FAIL\n");
            failed += 1;
        }
    }

    println!("==============================");
    println!("Results: {} passed, {} failed", passed, failed);

    if failed > 0 {
        std::process::exit(1);
    }
}
