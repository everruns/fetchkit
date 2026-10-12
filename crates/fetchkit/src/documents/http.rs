//! Remote backend: any HTTP service that accepts a multipart upload and returns
//! Markdown, such as a self-hosted Docling or MarkItDown wrapper.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use url::Url;

use super::remote::{
    multipart_file, status_error, ConverterClient, ConverterRequest, ReqwestConverterClient,
    SecretString, DEFAULT_REMOTE_TIMEOUT, MAX_REMOTE_RESPONSE_BYTES,
};
use super::{
    ConvertError, ConvertedDocument, DocumentConverter, DocumentFormat, DocumentInput, Locality,
};

/// Generic multipart-upload converter.
///
/// Sends the document as one multipart file field and reads Markdown either from
/// the whole response body or, with [`json_pointer`](Self::json_pointer), from a
/// string inside a JSON response.
#[derive(Clone)]
pub struct HttpConverter {
    name: String,
    endpoint: Url,
    field: String,
    headers: Vec<(String, SecretString)>,
    formats: Vec<DocumentFormat>,
    json_pointer: Option<String>,
    locality: Locality,
    timeout: Duration,
    max_input_bytes: Option<usize>,
    client: Arc<dyn ConverterClient>,
}

impl std::fmt::Debug for HttpConverter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpConverter")
            .field("name", &self.name)
            .field("endpoint", &self.endpoint.as_str())
            .field("field", &self.field)
            .field(
                "headers",
                &self
                    .headers
                    .iter()
                    .map(|(name, _)| name.as_str())
                    .collect::<Vec<_>>(),
            )
            .field("formats", &self.formats)
            .field("json_pointer", &self.json_pointer)
            .field("locality", &self.locality)
            .finish()
    }
}

impl HttpConverter {
    /// Create a converter named `name` that POSTs to `endpoint`.
    ///
    /// Defaults: field `file`, all formats, raw-body Markdown response, remote.
    pub fn new(name: impl Into<String>, endpoint: Url) -> Self {
        Self {
            name: name.into(),
            endpoint,
            field: "file".to_string(),
            headers: Vec::new(),
            formats: DocumentFormat::ALL.to_vec(),
            json_pointer: None,
            locality: Locality::Remote,
            timeout: DEFAULT_REMOTE_TIMEOUT,
            max_input_bytes: None,
            client: Arc::new(ReqwestConverterClient::new()),
        }
    }

    /// Multipart field name for the file.
    pub fn field(mut self, field: impl Into<String>) -> Self {
        self.field = field.into();
        self
    }

    /// Add a request header. Values are redacted from `Debug` output.
    pub fn header(mut self, name: impl Into<String>, value: impl Into<SecretString>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// Formats this service accepts.
    pub fn formats(mut self, formats: impl IntoIterator<Item = DocumentFormat>) -> Self {
        self.formats = formats.into_iter().collect();
        self
    }

    /// Read Markdown from this JSON pointer (e.g. `/markdown`) instead of the raw body.
    pub fn json_pointer(mut self, pointer: impl Into<String>) -> Self {
        self.json_pointer = Some(pointer.into());
        self
    }

    /// Mark the service as local. Only for services you operate on the same trust
    /// boundary: local converters run even when remote conversion is not allowed.
    pub fn locality(mut self, locality: Locality) -> Self {
        self.locality = locality;
        self
    }

    /// Per-document request timeout.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Largest document this service accepts.
    pub fn max_input_bytes(mut self, bytes: usize) -> Self {
        self.max_input_bytes = Some(bytes);
        self
    }

    /// Route uploads through a custom client.
    pub fn client(mut self, client: Arc<dyn ConverterClient>) -> Self {
        self.client = client;
        self
    }
}

#[async_trait]
impl DocumentConverter for HttpConverter {
    fn name(&self) -> &str {
        &self.name
    }

    fn locality(&self) -> Locality {
        self.locality
    }

    fn supports(&self, format: DocumentFormat) -> bool {
        self.formats.contains(&format)
    }

    fn max_input_bytes(&self) -> Option<usize> {
        self.max_input_bytes
    }

    async fn convert(&self, input: DocumentInput) -> Result<ConvertedDocument, ConvertError> {
        let (content_type, body) = multipart_file(
            &self.field,
            &input.file_name,
            input.format.media_type(),
            &input.body,
        );
        let mut headers = vec![("Content-Type".to_string(), content_type)];
        headers.extend(
            self.headers
                .iter()
                .map(|(name, value)| (name.clone(), value.expose().to_string())),
        );
        let response = self
            .client
            .post(ConverterRequest {
                url: self.endpoint.clone(),
                headers,
                body,
                timeout: self.timeout,
                max_response_bytes: MAX_REMOTE_RESPONSE_BYTES,
            })
            .await?;
        if let Some(error) = status_error(&self.name, response.status, &response.body) {
            return Err(error);
        }

        let markdown = match &self.json_pointer {
            None => String::from_utf8_lossy(&response.body).into_owned(),
            Some(pointer) => {
                let json: serde_json::Value =
                    serde_json::from_slice(&response.body).map_err(|error| {
                        ConvertError::Failed(format!("{}: invalid JSON: {error}", self.name))
                    })?;
                json.pointer(pointer)
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        ConvertError::Failed(format!("{}: no string at {pointer}", self.name))
                    })?
                    .to_string()
            }
        };
        Ok(ConvertedDocument::markdown(markdown))
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    fn input() -> DocumentInput {
        DocumentInput {
            url: Url::parse("https://example.com/deck.pptx").unwrap(),
            format: DocumentFormat::Pptx,
            content_type: None,
            file_name: "deck.pptx".to_string(),
            body: Bytes::from_static(b"PK\x03\x04pptx"),
        }
    }

    #[tokio::test]
    async fn reads_markdown_from_json_pointer() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/convert"))
            .and(header("x-api-key", "k"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"document": {"md": "# Slides"}})),
            )
            .mount(&server)
            .await;

        let converter = HttpConverter::new(
            "docling",
            Url::parse(&format!("{}/convert", server.uri())).unwrap(),
        )
        .header("x-api-key", "k")
        .json_pointer("/document/md");
        let result = converter.convert(input()).await.unwrap();
        assert_eq!(result.markdown, "# Slides");
        assert!(!format!("{converter:?}").contains("\"k\""));
        let requests = server.received_requests().await.unwrap();
        assert!(String::from_utf8_lossy(&requests[0].body).contains("name=\"file\""));
    }

    #[tokio::test]
    async fn raw_body_and_http_errors() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/ok"))
            .respond_with(ResponseTemplate::new(200).set_body_string("plain md"))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/fail"))
            .respond_with(ResponseTemplate::new(500).set_body_string("kaput"))
            .mount(&server)
            .await;

        let ok = HttpConverter::new("svc", Url::parse(&format!("{}/ok", server.uri())).unwrap());
        assert_eq!(ok.convert(input()).await.unwrap().markdown, "plain md");
        let fail = HttpConverter::new(
            "svc",
            Url::parse(&format!("{}/fail", server.uri())).unwrap(),
        );
        let error = fail.convert(input()).await.unwrap_err().to_string();
        assert!(error.contains("HTTP 500: kaput"));
    }
}
