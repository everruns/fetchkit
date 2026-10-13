//! Office documents (DOCX, XLSX/XLSM, XLS, ODS) to Markdown via the `markkit` crate.
//!
//! Decisions:
//! - markkit is the default Office backend; the in-crate `office` converter is
//!   deprecated. Parsing is local on the bounded response bytes; markkit is built
//!   without its `http_client` feature, so it cannot fetch anything itself.
//! - THREAT[TM-DOS-008]: zip-bomb and grid limits are markkit's (archive entry count,
//!   per-entry and total decompressed size, rows/columns per sheet, total cell grid).
//!   fetchkit adds the blocking-pool two-document concurrency cap and caps the final
//!   Markdown at `max_body_size`. Callers tune markkit limits through
//!   [`MarkkitConverter::with_markkit`].
//! - DOCX tables pass Fetchkit's archive preflight before markkit parses them:
//!   sheet limits in markkit do not apply to DOCX padding (TM-DOS-008).
//! - The confirmed fetchkit format's canonical media type is passed as the hint, so
//!   markkit parses the same format fetchkit detected and magic-checked.
//! - markkit warnings are `code: detail`; only the stable code is surfaced as a
//!   quality warning, since details embed document-controlled text (sheet names).

use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use markkit::{DocumentHint, Markkit};

use super::{
    run_document_task, ConvertError, ConvertedDocument, DocumentConverter, DocumentFormat,
    DocumentInput, Locality,
};

/// Converts Word (DOCX) and spreadsheets (XLSX, XLSM, XLS, ODS) to Markdown
/// in-process with [`markkit`].
///
/// ```
/// use fetchkit::documents::{Markkit, MarkkitConverter};
/// use fetchkit::DocumentConversion;
///
/// let markkit = Markkit::builder()
///     .max_sheet_rows(500)
///     .include_hidden_sheets(true)
///     .build();
/// // Replaces the default `markkit` converter in place.
/// let conversion =
///     DocumentConversion::default().converter(MarkkitConverter::with_markkit(markkit));
/// # let _ = conversion;
/// ```
#[derive(Clone)]
pub struct MarkkitConverter {
    markkit: Arc<Markkit>,
}

impl MarkkitConverter {
    /// Converter with markkit's default limits.
    pub fn new() -> Self {
        Self::with_markkit(Markkit::builder().build())
    }

    /// Converter using a configured [`Markkit`] instance (sheet row/column caps,
    /// hidden sheets, archive limits, maximum input size).
    pub fn with_markkit(markkit: Markkit) -> Self {
        Self {
            markkit: Arc::new(markkit),
        }
    }

    /// The markkit instance used for conversion.
    pub fn markkit(&self) -> &Markkit {
        &self.markkit
    }
}

impl Default for MarkkitConverter {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for MarkkitConverter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MarkkitConverter")
            .field("config", self.markkit.config())
            .finish()
    }
}

#[async_trait]
impl DocumentConverter for MarkkitConverter {
    fn name(&self) -> &str {
        "markkit"
    }

    fn locality(&self) -> Locality {
        Locality::Local
    }

    fn supports(&self, format: DocumentFormat) -> bool {
        matches!(
            format,
            DocumentFormat::Docx | DocumentFormat::Xlsx | DocumentFormat::Xls | DocumentFormat::Ods
        )
    }

    fn max_input_bytes(&self) -> Option<usize> {
        Some(self.markkit.config().max_input_size)
    }

    async fn convert(&self, input: DocumentInput) -> Result<ConvertedDocument, ConvertError> {
        if !self.supports(input.format) {
            return Err(ConvertError::Unsupported(input.format.to_string()));
        }
        let markkit = Arc::clone(&self.markkit);
        let media_type = input.format.media_type();
        let is_docx = input.format == DocumentFormat::Docx;
        let DocumentInput {
            body, file_name, ..
        } = input;
        let extraction = run_document_task(move || {
            if is_docx {
                super::tables::check_archive(&body, markkit.config())?;
            }
            markkit
                .extract_document_sync(
                    &body,
                    DocumentHint {
                        content_type: Some(media_type),
                        file_name: Some(&file_name),
                    },
                )
                .map_err(map_error)
        })
        .await??;

        let mut warnings: Vec<String> = Vec::new();
        for warning in &extraction.warnings {
            let code = warning
                .split_once(':')
                .map_or(warning.as_str(), |(code, _)| code)
                .trim()
                .to_string();
            if !code.is_empty() && !warnings.contains(&code) {
                warnings.push(code);
            }
        }
        Ok(ConvertedDocument {
            markdown: extraction.markdown,
            title: extraction.title,
            warnings,
            ocr_pages: Vec::new(),
            extraction_method: Some(format!("markkit_{}", extraction.format)),
        })
    }
}

fn map_error(error: markkit::Error) -> ConvertError {
    match error {
        markkit::Error::UnsupportedFormat(message) => ConvertError::Unsupported(message),
        markkit::Error::ResourceLimit(message) => {
            ConvertError::Failed(format!("document exceeds markkit limits: {message}"))
        }
        other => ConvertError::Failed(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use bytes::Bytes;
    use url::Url;

    use super::*;

    fn zip(parts: &[(&str, &str)]) -> Vec<u8> {
        let mut buffer = std::io::Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(&mut buffer);
        for (name, body) in parts {
            writer
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(body.as_bytes()).unwrap();
        }
        writer.finish().unwrap();
        buffer.into_inner()
    }

    const W: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;

    fn docx() -> Vec<u8> {
        let document = format!(
            r#"<w:document {W}><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Plan</w:t></w:r></w:p><w:p><w:r><w:t>Ship on Friday.</w:t></w:r></w:p><w:tbl><w:tr><w:tc><w:p><w:r><w:t>Owner</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Task</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>Ann</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Release</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:body></w:document>"#
        );
        let core = r#"<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Release plan</dc:title></cp:coreProperties>"#;
        zip(&[
            ("word/document.xml", &document),
            ("docProps/core.xml", core),
        ])
    }

    fn xlsx(hidden_second_sheet: bool) -> Vec<u8> {
        let state = if hidden_second_sheet {
            r#" state="hidden""#
        } else {
            ""
        };
        let workbook = format!(
            r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Sales" sheetId="1" r:id="rId1"/><sheet name="Secret" sheetId="2"{state} r:id="rId2"/></sheets></workbook>"#
        );
        let rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/></Relationships>"#;
        let sheet = |rows: &str| {
            format!(
                r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>{rows}</sheetData></worksheet>"#
            )
        };
        let row = |r: u32, a: &str, b: &str| {
            format!(
                r#"<row r="{r}"><c r="A{r}" t="inlineStr"><is><t>{a}</t></is></c><c r="B{r}" t="inlineStr"><is><t>{b}</t></is></c></row>"#
            )
        };
        let sales = sheet(&format!(
            "{}{}{}",
            row(1, "Region", "Total"),
            row(2, "North", "10"),
            row(3, "South", "20")
        ));
        let secret = sheet(&row(1, "Key", "hunter2"));
        let content_types = r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/></Types>"#;
        let root_rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;
        zip(&[
            ("[Content_Types].xml", content_types),
            ("_rels/.rels", root_rels),
            ("xl/workbook.xml", &workbook),
            ("xl/_rels/workbook.xml.rels", rels),
            ("xl/worksheets/sheet1.xml", &sales),
            ("xl/worksheets/sheet2.xml", &secret),
        ])
    }

    fn input(format: DocumentFormat, body: Vec<u8>) -> DocumentInput {
        DocumentInput {
            url: Url::parse("https://example.com/file").unwrap(),
            format,
            content_type: Some("application/octet-stream".to_string()),
            file_name: format!("file.{}", format.extensions()[0]),
            body: Bytes::from(body),
        }
    }

    #[test]
    fn supports_markkit_office_formats_only() {
        let converter = MarkkitConverter::new();
        for format in [
            DocumentFormat::Docx,
            DocumentFormat::Xlsx,
            DocumentFormat::Xls,
            DocumentFormat::Ods,
        ] {
            assert!(converter.supports(format), "{format}");
        }
        for format in [
            DocumentFormat::Pdf,
            DocumentFormat::Pptx,
            DocumentFormat::Xlsb,
            DocumentFormat::Doc,
            DocumentFormat::Odt,
        ] {
            assert!(!converter.supports(format), "{format}");
        }
        assert_eq!(converter.name(), "markkit");
        assert_eq!(converter.locality(), Locality::Local);
    }

    #[tokio::test]
    async fn converts_docx_with_title_heading_and_table() {
        let converted = MarkkitConverter::new()
            .convert(input(DocumentFormat::Docx, docx()))
            .await
            .unwrap();
        assert_eq!(converted.extraction_method.as_deref(), Some("markkit_docx"));
        assert_eq!(converted.title.as_deref(), Some("Release plan"));
        assert!(
            converted.markdown.contains("# Plan"),
            "{}",
            converted.markdown
        );
        assert!(converted.markdown.contains("Ship on Friday."));
        assert!(converted.markdown.contains("| Ann | Release |"));
        assert!(converted.warnings.is_empty());
    }

    #[tokio::test]
    async fn converts_docx_with_relationship_selected_main_part() {
        let body = zip(&[
            (
                "_rels/.rels",
                r#"<Relationships><Relationship Id="r1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="./parts/custom.data"/></Relationships>"#,
            ),
            (
                "parts/custom.data",
                r#"<w:document xmlns:w="w"><w:body><w:tbl><w:tr><w:tc><w:p><w:r><w:t>Owner</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>Ann</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:body></w:document>"#,
            ),
            ("word/media/image.png", "ordinary binary fixture"),
        ]);
        let converted = MarkkitConverter::new()
            .convert(input(DocumentFormat::Docx, body))
            .await
            .unwrap();
        assert!(converted.markdown.contains("| Owner |\n| --- |\n| Ann |"));
    }

    #[tokio::test]
    async fn converts_xlsx_and_reports_warning_codes() {
        let converted = MarkkitConverter::new()
            .convert(input(DocumentFormat::Xlsx, xlsx(true)))
            .await
            .unwrap();
        assert_eq!(converted.extraction_method.as_deref(), Some("markkit_xlsx"));
        assert!(
            converted.markdown.contains("Sales"),
            "{}",
            converted.markdown
        );
        assert!(converted.markdown.contains("| North | 10 |"));
        assert!(!converted.markdown.contains("hunter2"));
        assert_eq!(converted.warnings, ["hidden_sheet_skipped"]);
    }

    #[tokio::test]
    async fn markkit_limits_are_configurable() {
        let markkit = Markkit::builder()
            .include_hidden_sheets(true)
            .max_sheet_rows(1)
            .build();
        let converted = MarkkitConverter::with_markkit(markkit)
            .convert(input(DocumentFormat::Xlsx, xlsx(true)))
            .await
            .unwrap();
        assert!(converted.markdown.contains("hunter2"));
        assert!(!converted.markdown.contains("South"));
        assert!(converted.warnings.contains(&"truncated_sheet".to_string()));

        let tiny = MarkkitConverter::with_markkit(Markkit::builder().max_input_size(16).build());
        assert_eq!(tiny.max_input_bytes(), Some(16));
    }

    #[tokio::test]
    async fn maps_markkit_errors() {
        let error = MarkkitConverter::new()
            .convert(input(DocumentFormat::Docx, b"PK\x03\x04broken".to_vec()))
            .await
            .unwrap_err();
        assert!(matches!(error, ConvertError::Failed(_)), "{error:?}");

        let error = MarkkitConverter::new()
            .convert(input(DocumentFormat::Pptx, docx()))
            .await
            .unwrap_err();
        assert!(matches!(error, ConvertError::Unsupported(_)), "{error:?}");

        let strict = Markkit::builder().max_archive_entries(1).build();
        let error = MarkkitConverter::with_markkit(strict)
            .convert(input(DocumentFormat::Docx, docx()))
            .await
            .unwrap_err();
        assert!(
            matches!(&error, ConvertError::Failed(message) if message.contains("limits")),
            "{error:?}"
        );
    }

    #[test]
    fn maps_unsupported_and_resource_limit_errors() {
        assert!(matches!(
            map_error(markkit::Error::UnsupportedFormat("xlsb".into())),
            ConvertError::Unsupported(_)
        ));
        assert!(matches!(
            map_error(markkit::Error::ResourceLimit("too big".into())),
            ConvertError::Failed(message) if message.contains("too big")
        ));
    }
}
