//! End-to-end document conversion through the Tool surface (wiremock, no network).

use std::sync::Arc;

use async_trait::async_trait;
use fetchkit::{
    ContentProcessor, ContentProcessorError, ContentProcessorInput, ConvertError,
    ConvertedDocument, DocumentConversion, DocumentConverter, DocumentFormat, DocumentInput,
    FetchRequest, Locality, ProcessedContent, Tool, ToolBuilder,
};
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const DOCX: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

struct EchoDocx {
    name: &'static str,
    locality: Locality,
}

#[async_trait]
impl DocumentConverter for EchoDocx {
    fn name(&self) -> &str {
        self.name
    }
    fn locality(&self) -> Locality {
        self.locality
    }
    fn supports(&self, format: DocumentFormat) -> bool {
        format == DocumentFormat::Docx
    }
    async fn convert(&self, input: DocumentInput) -> Result<ConvertedDocument, ConvertError> {
        Ok(ConvertedDocument {
            markdown: format!("# {} ({} bytes)", input.file_name, input.body.len()),
            title: Some("Quarterly".to_string()),
            ..Default::default()
        })
    }
}

fn local() -> EchoDocx {
    EchoDocx {
        name: "echo",
        locality: Locality::Local,
    }
}

fn docx_body(len: usize) -> Vec<u8> {
    let mut body = b"PK\x03\x04".to_vec();
    body.resize(len, b'x');
    body
}

async fn serve_docx(len: usize) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/files/q3.docx"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", DOCX)
                .set_body_bytes(docx_body(len)),
        )
        .mount(&server)
        .await;
    server
}

fn builder() -> ToolBuilder {
    Tool::builder().block_private_ips(false)
}

fn request(server: &MockServer) -> FetchRequest {
    FetchRequest::new(format!("{}/files/q3.docx", server.uri())).as_markdown()
}

#[tokio::test]
async fn unconverted_format_is_rejected_with_actionable_message() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/files/old.doc"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/msword")
                .set_body_bytes(vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]),
        )
        .mount(&server)
        .await;
    let response = builder()
        .build()
        .execute(FetchRequest::new(format!("{}/files/old.doc", server.uri())).as_markdown())
        .await
        .unwrap();

    assert!(response.content.is_none());
    let error = response.error.unwrap();
    assert!(error.contains("DOC conversion is not enabled"), "{error}");
}

#[tokio::test]
async fn empty_conversion_disables_built_in_converters() {
    let server = serve_docx(64).await;
    let response = builder()
        .document_conversion(DocumentConversion::empty())
        .build()
        .execute(request(&server))
        .await
        .unwrap();
    assert!(response
        .error
        .unwrap()
        .contains("DOCX conversion is not enabled"));
}

#[tokio::test]
async fn custom_converter_converts_docx_through_tool() {
    let server = serve_docx(64).await;
    let tool = builder()
        .document_conversion(DocumentConversion::empty().converter(local()))
        .build();
    let response = tool.execute(request(&server)).await.unwrap();

    assert_eq!(response.format.as_deref(), Some("markdown"));
    assert_eq!(response.content.as_deref(), Some("# q3.docx (64 bytes)"));
    let metadata = response.metadata.unwrap();
    assert_eq!(metadata.title.as_deref(), Some("Quarterly"));
    assert_eq!(metadata.extraction_method.as_deref(), Some("echo"));
}

#[tokio::test]
async fn remote_converter_needs_explicit_opt_in_and_hardened_denies_it() {
    let server = serve_docx(64).await;
    let remote = || {
        DocumentConversion::empty().converter(EchoDocx {
            name: "remote-echo",
            locality: Locality::Remote,
        })
    };

    let not_allowed = builder().document_conversion(remote()).build();
    let response = not_allowed.execute(request(&server)).await.unwrap();
    assert!(response.content.is_none());

    let allowed = builder()
        .document_conversion(remote().allow_remote(true))
        .build();
    let response = allowed.execute(request(&server)).await.unwrap();
    assert!(response.content.unwrap().starts_with("# q3.docx"));

    // hardened() wins even when remote conversion was allowed; loopback is
    // re-enabled so only the conversion policy is under test.
    let hardened = Tool::builder()
        .document_conversion(remote().allow_remote(true))
        .hardened()
        .block_private_ips(false)
        .build();
    assert!(format!("{hardened:?}").contains("allow_remote: false"));
}

#[tokio::test]
async fn document_cap_is_separate_from_body_cap() {
    let server = serve_docx(4096).await;
    let small_body_cap = || builder().max_body_size(1024);

    let converted = small_body_cap()
        .document_conversion(DocumentConversion::empty().converter(local()))
        .build()
        .execute(request(&server))
        .await
        .unwrap();
    assert_eq!(converted.content.as_deref(), Some("# q3.docx (4096 bytes)"));

    let too_large = small_body_cap()
        .document_conversion(
            DocumentConversion::empty()
                .converter(local())
                .max_document_bytes(2048),
        )
        .build()
        .execute(request(&server))
        .await
        .unwrap();
    assert!(too_large.content.is_none());
    assert!(too_large
        .error
        .unwrap()
        .contains("cannot process a partial response body"));
}

#[tokio::test]
async fn format_allowlist_limits_conversion() {
    let server = serve_docx(64).await;
    let tool = builder()
        .document_conversion(
            DocumentConversion::empty()
                .converter(local())
                .formats([DocumentFormat::Pdf]),
        )
        .build();
    let response = tool.execute(request(&server)).await.unwrap();
    assert!(response.content.is_none());
}

struct Shout;

#[async_trait]
impl ContentProcessor for Shout {
    fn name(&self) -> &'static str {
        "shout"
    }
    fn matches(&self, _url: &Url, content_type: Option<&str>) -> bool {
        content_type.is_some_and(|value| value.starts_with(DOCX))
    }
    async fn process(
        &self,
        input: ContentProcessorInput,
    ) -> Result<ProcessedContent, ContentProcessorError> {
        Ok(ProcessedContent {
            format: Some("markdown".to_string()),
            content: Some(format!("CUSTOM {}", input.body.len())),
            ..Default::default()
        })
    }
}

#[tokio::test]
async fn custom_content_processor_runs_before_document_chain() {
    let server = serve_docx(10).await;
    let tool = builder()
        .document_conversion(DocumentConversion::empty().converter(local()))
        .content_processor(Arc::new(Shout))
        .build();
    let response = tool.execute(request(&server)).await.unwrap();
    assert_eq!(response.content.as_deref(), Some("CUSTOM 10"));
}

#[cfg(feature = "docs-cloudflare")]
#[tokio::test]
async fn cloudflare_backend_converts_downloaded_docx() {
    use fetchkit::documents::CloudflareConverter;
    use wiremock::matchers::header;

    let origin = serve_docx(32).await;
    let cloudflare = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/accounts/acct/ai/tomarkdown"))
        .and(header("authorization", "Bearer tok"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true,
            "result": [{"name": "q3.docx", "format": "markdown", "data": "# From Cloudflare"}]
        })))
        .expect(1)
        .mount(&cloudflare)
        .await;

    let converter =
        CloudflareConverter::new("acct", "tok").api_base(Url::parse(&cloudflare.uri()).unwrap());
    let tool = builder()
        .document_conversion(
            DocumentConversion::empty()
                .converter(converter)
                .allow_remote(true),
        )
        .build();
    let response = tool.execute(request(&origin)).await.unwrap();

    assert_eq!(response.content.as_deref(), Some("# From Cloudflare"));
    assert_eq!(
        response.metadata.unwrap().extraction_method.as_deref(),
        Some("cloudflare_tomarkdown")
    );
    let uploads = cloudflare.received_requests().await.unwrap();
    assert!(uploads[0]
        .body
        .windows(4)
        .any(|window| window == b"PK\x03\x04"));
}
