//! Local PDF backend using `pdf-inspector`.

use async_trait::async_trait;
use pdf_inspector::process_pdf_mem;

use super::{
    run_document_task, ConvertError, ConvertedDocument, DocumentConverter, DocumentFormat,
    DocumentInput, Locality,
};

/// Extracts Markdown from text-based PDFs in-process. Does not perform OCR:
/// image-only pages are reported through [`ConvertedDocument::ocr_pages`].
#[derive(Debug, Default, Clone, Copy)]
pub struct PdfInspectorConverter;

impl PdfInspectorConverter {
    /// Create the converter.
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl DocumentConverter for PdfInspectorConverter {
    fn name(&self) -> &str {
        "pdf-inspector"
    }

    fn locality(&self) -> Locality {
        Locality::Local
    }

    fn supports(&self, format: DocumentFormat) -> bool {
        format == DocumentFormat::Pdf
    }

    async fn convert(&self, input: DocumentInput) -> Result<ConvertedDocument, ConvertError> {
        let result = run_document_task(move || process_pdf_mem(&input.body))
            .await?
            .map_err(|error| ConvertError::Failed(error.to_string()))?;

        let mut warnings = Vec::new();
        if result.has_encoding_issues {
            warnings.push("pdf_encoding_issues".to_string());
        }
        Ok(ConvertedDocument {
            markdown: result.markdown.unwrap_or_default(),
            title: result.title,
            warnings,
            ocr_pages: result.pages_needing_ocr,
            extraction_method: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn deeply_nested_pdf_returns_without_aborting() {
        let nested = format!("{}0{}", "[".repeat(10_000), "]".repeat(10_000));
        let objects = [
            format!("<< /Type /Catalog /Pages 2 0 R /Nested {nested} >>"),
            "<< /Type /Pages /Kids [] /Count 0 >>".to_owned(),
        ];
        let mut body = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(body.len());
            body.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
        }
        let xref = body.len();
        body.extend_from_slice(b"xref\n0 3\n0000000000 65535 f \n");
        for offset in offsets {
            body.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        body.extend_from_slice(
            format!("trailer\n<< /Size 3 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
        );

        let result = PdfInspectorConverter::new()
            .convert(DocumentInput {
                url: "https://example.test/nested.pdf".parse().unwrap(),
                format: DocumentFormat::Pdf,
                content_type: Some("application/pdf".to_owned()),
                file_name: "nested.pdf".to_owned(),
                body: body.into(),
            })
            .await;
        // The bounded parser may reject the document or discard its invalid
        // nested object. Either outcome must return control rather than abort.
        if let Ok(document) = result {
            assert!(document.markdown.is_empty());
        }
    }
}
