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
