//! Post-download content processing.
//!
//! Fetchers own retrieval and network policy. Content processors operate only on
//! bounded response bytes, so format-specific extraction cannot bypass egress controls.

use async_trait::async_trait;
use bytes::Bytes;
use pdf_inspector::process_pdf_mem;
use thiserror::Error;
use tokio::sync::Semaphore;
use url::Url;

use crate::{PageMetadata, PageQuality};

// pdf-inspector uses Rayon internally; bound concurrent documents so batch fetches
// cannot multiply parser thread pools without limit.
static PDF_PROCESSING_LIMIT: Semaphore = Semaphore::const_new(2);

/// Bounded response bytes and metadata passed to a [`ContentProcessor`].
pub struct ContentProcessorInput {
    /// Final URL after redirects.
    pub url: Url,
    /// Response Content-Type header, when present.
    pub content_type: Option<String>,
    /// Response body after fetchkit's timeout and size limits were applied.
    pub body: Bytes,
}

/// Content extracted from a non-text response.
#[derive(Debug, Default)]
pub struct ProcessedContent {
    /// Output format, such as `markdown`.
    pub format: Option<String>,
    /// Extracted textual content.
    pub content: Option<String>,
    /// Structured metadata discovered while processing.
    pub metadata: Option<PageMetadata>,
    /// Agent-facing quality signals.
    pub quality: Option<PageQuality>,
    /// Recoverable processing limitation, such as OCR being required.
    pub error: Option<String>,
}

/// Error returned when a content processor cannot parse its input.
#[derive(Debug, Error)]
#[error("{0}")]
pub struct ContentProcessorError(pub String);

/// Converts bounded response bytes into LLM-friendly textual content.
#[async_trait]
pub trait ContentProcessor: Send + Sync {
    /// Unique identifier for logging and diagnostics.
    fn name(&self) -> &'static str;

    /// Whether this processor supports the response metadata.
    ///
    /// This is evaluated before downloading a body that would otherwise be
    /// rejected as binary, so implementations must not inspect body bytes here.
    fn matches(&self, url: &Url, content_type: Option<&str>) -> bool;

    /// Process an already-downloaded, bounded response body.
    async fn process(
        &self,
        input: ContentProcessorInput,
    ) -> Result<ProcessedContent, ContentProcessorError>;
}

/// Ordered registry of response content processors.
pub struct ContentProcessorRegistry {
    processors: Vec<Box<dyn ContentProcessor>>,
}

impl Default for ContentProcessorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ContentProcessorRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            processors: Vec::new(),
        }
    }

    /// Create a registry containing fetchkit's built-in processors.
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        registry.register(Box::new(PdfProcessor));
        registry
    }

    /// Append a processor. The first matching processor wins.
    pub fn register(&mut self, processor: Box<dyn ContentProcessor>) {
        self.processors.push(processor);
    }

    /// Find the first processor matching response metadata.
    pub fn find(&self, url: &Url, content_type: Option<&str>) -> Option<&dyn ContentProcessor> {
        self.processors
            .iter()
            .find(|processor| processor.matches(url, content_type))
            .map(Box::as_ref)
    }
}

/// Extracts native text PDFs as Markdown using `pdf-inspector`.
pub struct PdfProcessor;

#[async_trait]
impl ContentProcessor for PdfProcessor {
    fn name(&self) -> &'static str {
        "pdf"
    }

    fn matches(&self, url: &Url, content_type: Option<&str>) -> bool {
        let media_type = content_type
            .and_then(|value| value.split(';').next())
            .map(str::trim)
            .unwrap_or_default();
        if media_type.eq_ignore_ascii_case("application/pdf")
            || media_type.eq_ignore_ascii_case("application/x-pdf")
        {
            return true;
        }

        (media_type.is_empty() || media_type.eq_ignore_ascii_case("application/octet-stream"))
            && url.path().to_ascii_lowercase().ends_with(".pdf")
    }

    async fn process(
        &self,
        input: ContentProcessorInput,
    ) -> Result<ProcessedContent, ContentProcessorError> {
        let _permit = PDF_PROCESSING_LIMIT.acquire().await.map_err(|error| {
            ContentProcessorError(format!("PDF processing unavailable: {error}"))
        })?;
        let result = tokio::task::spawn_blocking(move || process_pdf_mem(&input.body))
            .await
            .map_err(|error| ContentProcessorError(format!("PDF processor task failed: {error}")))?
            .map_err(|error| ContentProcessorError(error.to_string()))?;

        let needs_ocr = !result.pages_needing_ocr.is_empty();
        let mut warnings = Vec::new();
        if needs_ocr {
            warnings.push("pdf_requires_ocr".to_string());
        }
        if result.has_encoding_issues {
            warnings.push("pdf_encoding_issues".to_string());
        }

        let content = result.markdown.filter(|value| !value.trim().is_empty());
        let error = content.is_none().then(|| {
            if needs_ocr {
                format!(
                    "PDF requires OCR for pages: {}",
                    comma_separated_pages(&result.pages_needing_ocr)
                )
            } else {
                "PDF contains no extractable text".to_string()
            }
        });
        let metadata = PageMetadata {
            title: result.title,
            extraction_method: Some("pdf_inspector".to_string()),
            ..Default::default()
        };
        let quality = PageQuality {
            score: if content.is_none() {
                0.0
            } else if warnings.is_empty() {
                1.0
            } else {
                0.6
            },
            warnings,
            extraction_method: Some("pdf_inspector".to_string()),
            suggested_next_action: needs_ocr.then(|| "use_ocr".to_string()),
            ..Default::default()
        };

        Ok(ProcessedContent {
            format: content.as_ref().map(|_| "markdown".to_string()),
            content,
            metadata: Some(metadata),
            quality: Some(quality),
            error,
        })
    }
}

fn comma_separated_pages(pages: &[u32]) -> String {
    pages
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_pdf(text: &str) -> Bytes {
        let escaped = text
            .replace('\\', "\\\\")
            .replace('(', "\\(")
            .replace(')', "\\)");
        let stream = format!("BT /F1 24 Tf 72 720 Td ({escaped}) Tj ET");
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".to_string(),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
            format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len()),
        ];

        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
        }
        let xref_offset = pdf.len();
        pdf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        pdf.extend_from_slice(b"0000000000 65535 f \n");
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        Bytes::from(pdf)
    }

    #[test]
    fn pdf_processor_matches_media_type_or_binary_pdf_path() {
        let processor = PdfProcessor;
        let extensionless = Url::parse("https://example.com/download?id=1").unwrap();
        let pdf_path = Url::parse("https://example.com/report.PDF").unwrap();

        assert!(processor.matches(&extensionless, Some("application/pdf; charset=binary")));
        assert!(processor.matches(&pdf_path, Some("application/octet-stream")));
        assert!(!processor.matches(&pdf_path, Some("text/html")));
        assert!(!processor.matches(&extensionless, Some("application/octet-stream")));
    }

    #[test]
    fn registry_uses_first_matching_processor() {
        let registry = ContentProcessorRegistry::with_defaults();
        let url = Url::parse("https://example.com/document").unwrap();

        assert_eq!(
            registry
                .find(&url, Some("application/pdf"))
                .map(ContentProcessor::name),
            Some("pdf")
        );
        assert!(registry.find(&url, Some("image/png")).is_none());
    }

    #[tokio::test]
    async fn pdf_processor_extracts_markdown_from_memory() {
        let processor = PdfProcessor;
        let result = processor
            .process(ContentProcessorInput {
                url: Url::parse("https://example.com/document").unwrap(),
                content_type: Some("application/pdf".to_string()),
                body: text_pdf("Hello from PDF"),
            })
            .await
            .unwrap();

        assert_eq!(result.format.as_deref(), Some("markdown"));
        assert!(result.content.unwrap().contains("Hello from PDF"));
        assert_eq!(
            result.metadata.unwrap().extraction_method.as_deref(),
            Some("pdf_inspector")
        );
        assert!(result.error.is_none());
    }
}
