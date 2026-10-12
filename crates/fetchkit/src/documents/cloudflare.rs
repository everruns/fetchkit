// Decisions:
// - Uses the Workers AI `toMarkdown` REST endpoint with one file per request, so a
//   failure maps cleanly to one document.
// - Images are not in the default format set: Cloudflare bills image conversion,
//   and fetchkit's document chain only routes non-image formats anyway.
// - Formats follow Cloudflare's published list (no PPTX or legacy .doc). Override
//   with `CloudflareConverter::formats` when Cloudflare adds more.
//! Remote backend: Cloudflare Workers AI `toMarkdown`.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use url::Url;

use super::remote::{
    multipart_file, status_error, ConverterClient, ConverterRequest, ReqwestConverterClient,
    SecretString, DEFAULT_REMOTE_TIMEOUT, MAX_REMOTE_RESPONSE_BYTES,
};
use super::{
    ConvertError, ConvertedDocument, DocumentConverter, DocumentFormat, DocumentInput, Locality,
};

const DEFAULT_API_BASE: &str = "https://api.cloudflare.com/client/v4";

const DEFAULT_FORMATS: &[DocumentFormat] = &[
    DocumentFormat::Pdf,
    DocumentFormat::Docx,
    DocumentFormat::Xlsx,
    DocumentFormat::Xlsb,
    DocumentFormat::Xls,
    DocumentFormat::Odt,
    DocumentFormat::Ods,
];

/// Converts documents with Cloudflare Workers AI `toMarkdown`.
///
/// Requires an account id and an API token with Workers AI permission. This is a
/// [`Locality::Remote`] converter: it runs only when remote conversion is allowed.
#[derive(Clone)]
pub struct CloudflareConverter {
    account_id: String,
    api_token: SecretString,
    api_base: Url,
    formats: Vec<DocumentFormat>,
    timeout: Duration,
    client: Arc<dyn ConverterClient>,
}

impl std::fmt::Debug for CloudflareConverter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CloudflareConverter")
            .field("account_id", &self.account_id)
            .field("api_token", &self.api_token)
            .field("api_base", &self.api_base.as_str())
            .field("formats", &self.formats)
            .field("timeout", &self.timeout)
            .finish()
    }
}

impl CloudflareConverter {
    /// Create a converter for a Cloudflare account.
    pub fn new(account_id: impl Into<String>, api_token: impl Into<SecretString>) -> Self {
        Self {
            account_id: account_id.into(),
            api_token: api_token.into(),
            api_base: Url::parse(DEFAULT_API_BASE).expect("valid default Cloudflare API URL"),
            formats: DEFAULT_FORMATS.to_vec(),
            timeout: DEFAULT_REMOTE_TIMEOUT,
            client: Arc::new(ReqwestConverterClient::new()),
        }
    }

    /// Read `CLOUDFLARE_ACCOUNT_ID` and `CLOUDFLARE_API_TOKEN`. Intended for CLIs;
    /// libraries should pass credentials explicitly.
    pub fn from_env() -> Option<Self> {
        let account_id = std::env::var("CLOUDFLARE_ACCOUNT_ID").ok()?;
        let api_token = std::env::var("CLOUDFLARE_API_TOKEN").ok()?;
        (!account_id.trim().is_empty() && !api_token.trim().is_empty())
            .then(|| Self::new(account_id.trim(), api_token.trim()))
    }

    /// Override the API base URL (for tests or a gateway in front of Cloudflare).
    pub fn api_base(mut self, api_base: Url) -> Self {
        self.api_base = api_base;
        self
    }

    /// Override the formats sent to Cloudflare.
    pub fn formats(mut self, formats: impl IntoIterator<Item = DocumentFormat>) -> Self {
        self.formats = formats.into_iter().collect();
        self
    }

    /// Per-document request timeout.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Route uploads through a custom client (e.g. a host egress proxy).
    pub fn client(mut self, client: Arc<dyn ConverterClient>) -> Self {
        self.client = client;
        self
    }

    fn endpoint(&self) -> Result<Url, ConvertError> {
        let base = self.api_base.as_str().trim_end_matches('/');
        let account: String =
            url::form_urlencoded::byte_serialize(self.account_id.as_bytes()).collect();
        Url::parse(&format!("{base}/accounts/{account}/ai/tomarkdown"))
            .map_err(|error| ConvertError::Failed(format!("cloudflare endpoint: {error}")))
    }
}

#[derive(Deserialize)]
struct Envelope {
    #[serde(default)]
    success: bool,
    #[serde(default)]
    result: Vec<FileResult>,
    #[serde(default)]
    errors: Vec<ApiMessage>,
}

#[derive(Deserialize)]
struct FileResult {
    #[serde(default)]
    format: String,
    #[serde(default)]
    data: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Deserialize)]
struct ApiMessage {
    #[serde(default)]
    message: String,
}

#[async_trait]
impl DocumentConverter for CloudflareConverter {
    fn name(&self) -> &str {
        "cloudflare"
    }

    fn locality(&self) -> Locality {
        Locality::Remote
    }

    fn supports(&self, format: DocumentFormat) -> bool {
        self.formats.contains(&format)
    }

    async fn convert(&self, input: DocumentInput) -> Result<ConvertedDocument, ConvertError> {
        let (content_type, body) = multipart_file(
            "files",
            &input.file_name,
            input.format.media_type(),
            &input.body,
        );
        let response = self
            .client
            .post(ConverterRequest {
                url: self.endpoint()?,
                headers: vec![
                    ("Content-Type".to_string(), content_type),
                    (
                        "Authorization".to_string(),
                        format!("Bearer {}", self.api_token.expose()),
                    ),
                    ("Accept".to_string(), "application/json".to_string()),
                ],
                body,
                timeout: self.timeout,
                max_response_bytes: MAX_REMOTE_RESPONSE_BYTES,
            })
            .await?;

        let envelope: Option<Envelope> = serde_json::from_slice(&response.body).ok();
        if let Some(error) = status_error("cloudflare", response.status, &response.body) {
            let api_message = envelope
                .as_ref()
                .and_then(|envelope| envelope.errors.first())
                .map(|message| message.message.clone())
                .filter(|message| !message.is_empty());
            let auth_failure = matches!(response.status, 401 | 403);
            return Err(match (error, api_message) {
                (ConvertError::Failed(_), Some(message)) if !auth_failure => {
                    ConvertError::Failed(format!("cloudflare: HTTP {}: {message}", response.status))
                }
                (error, _) => error,
            });
        }
        let envelope = envelope.ok_or_else(|| {
            ConvertError::Failed("cloudflare: response is not valid JSON".to_string())
        })?;
        if !envelope.success {
            let message = envelope
                .errors
                .iter()
                .map(|message| message.message.as_str())
                .collect::<Vec<_>>()
                .join("; ");
            return Err(ConvertError::Failed(format!("cloudflare: {message}")));
        }
        let file = envelope
            .result
            .into_iter()
            .next()
            .ok_or_else(|| ConvertError::Failed("cloudflare: empty result".to_string()))?;
        if file.format != "markdown" {
            return Err(ConvertError::Failed(format!(
                "cloudflare: {}",
                file.error
                    .unwrap_or_else(|| "conversion failed".to_string())
            )));
        }
        Ok(ConvertedDocument {
            markdown: file.data.unwrap_or_default(),
            extraction_method: Some("cloudflare_tomarkdown".to_string()),
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use wiremock::matchers::{header, header_regex, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    fn input() -> DocumentInput {
        DocumentInput {
            url: Url::parse("https://example.com/report.docx").unwrap(),
            format: DocumentFormat::Docx,
            content_type: None,
            file_name: "report.docx".to_string(),
            body: Bytes::from_static(b"PK\x03\x04docx"),
        }
    }

    async fn converter(server: &MockServer) -> CloudflareConverter {
        CloudflareConverter::new("acct", "secret-token")
            .api_base(Url::parse(&server.uri()).unwrap())
    }

    #[tokio::test]
    async fn uploads_file_and_returns_markdown() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/accounts/acct/ai/tomarkdown"))
            .and(header("authorization", "Bearer secret-token"))
            .and(header_regex("content-type", "^multipart/form-data; boundary="))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "result": [{"name": "report.docx", "mimeType": "x", "format": "markdown", "tokens": 3, "data": "# Report"}]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let result = converter(&server).await.convert(input()).await.unwrap();
        assert_eq!(result.markdown, "# Report");
        assert_eq!(
            result.extraction_method.as_deref(),
            Some("cloudflare_tomarkdown")
        );
        let requests = server.received_requests().await.unwrap();
        let body = String::from_utf8_lossy(&requests[0].body);
        assert!(body.contains("filename=\"report.docx\""));
        assert!(body.contains("wordprocessingml"));
    }

    #[tokio::test]
    async fn per_file_error_is_reported() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "result": [{"name": "report.docx", "format": "error", "error": "corrupt file"}]
            })))
            .mount(&server)
            .await;

        let error = converter(&server).await.convert(input()).await.unwrap_err();
        assert_eq!(
            error,
            ConvertError::Failed("cloudflare: corrupt file".into())
        );
    }

    #[tokio::test]
    async fn auth_failure_does_not_leak_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({
                "success": false, "errors": [{"code": 10000, "message": "Authentication error"}]
            })))
            .mount(&server)
            .await;

        let converter = converter(&server).await;
        let error = converter.convert(input()).await.unwrap_err().to_string();
        assert!(error.contains("authentication failed"));
        assert!(!error.contains("secret-token"));
        assert!(!format!("{converter:?}").contains("secret-token"));
    }

    #[test]
    fn default_formats_exclude_unsupported_office_types() {
        let converter = CloudflareConverter::new("a", "b");
        assert!(converter.supports(DocumentFormat::Xlsx));
        assert!(!converter.supports(DocumentFormat::Pptx));
        assert!(!converter.supports(DocumentFormat::Doc));
        assert_eq!(converter.locality(), Locality::Remote);
    }
}
