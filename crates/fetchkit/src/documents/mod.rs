// Decisions:
// - Document conversion is a backend chain behind ONE ContentProcessor
//   (`DocumentProcessor`). Fetchers keep owning retrieval and egress policy; a
//   converter only ever receives bounded bytes fetchkit already downloaded, never a
//   URL to fetch itself.
// - Remote converters (Locality::Remote) send those bytes to a third party, so they
//   are skipped unless `DocumentConversion::allow_remote(true)` is set explicitly.
//   `ToolBuilder::hardened()` forces remote conversion off (TM-LEAK-006).
// - Per-format chain order is by converter name. A converter falls through to the
//   next one on unsupported input, an error, or an empty / OCR-required result, so
//   "local first, remote for scanned PDFs" needs no special case.
// - Format detection runs on response metadata before download (Content-Type, then
//   URL extension for missing/generic types) and is confirmed against magic bytes
//   after download, so a mislabelled body never reaches a parser for another format.
//! Pluggable document (PDF, Office, OpenDocument) to Markdown conversion.
//!
//! [`DocumentProcessor`] is a [`ContentProcessor`] that detects the document format
//! and runs an ordered chain of [`DocumentConverter`] backends configured by
//! [`DocumentConversion`]. Implement [`DocumentConverter`] to add a backend, or
//! [`FormatDetector`] to change how formats are recognised.

#[cfg(feature = "docs-cloudflare")]
mod cloudflare;
#[cfg(feature = "docs-http")]
mod http;
#[cfg(feature = "docs-markkit")]
mod markkit;
#[cfg(feature = "docs-office")]
mod office;
#[cfg(feature = "docs-pdf")]
mod pdf_inspector;
#[cfg(any(feature = "docs-cloudflare", feature = "docs-http"))]
mod remote;
mod settings;
#[cfg(any(feature = "docs-markkit", feature = "docs-office"))]
mod tables;

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use tracing::{debug, warn};
use url::Url;

use crate::content::{
    ContentOutputFormat, ContentProcessor, ContentProcessorError, ContentProcessorInput,
    ProcessedContent,
};
use crate::{PageMetadata, PageQuality};

#[cfg(feature = "docs-markkit")]
pub use self::markkit::MarkkitConverter;
/// markkit types for configuring [`MarkkitConverter`] limits.
#[cfg(feature = "docs-markkit")]
pub use ::markkit::{Markkit, MarkkitBuilder};
#[cfg(feature = "docs-cloudflare")]
pub use cloudflare::CloudflareConverter;
#[cfg(feature = "docs-http")]
pub use http::HttpConverter;
#[cfg(feature = "docs-office")]
#[allow(deprecated)]
pub use office::OfficeConverter;
#[cfg(feature = "docs-pdf")]
pub use pdf_inspector::PdfInspectorConverter;
#[cfg(any(feature = "docs-cloudflare", feature = "docs-http"))]
pub use remote::{
    ConverterClient, ConverterRequest, ConverterResponse, ReqwestConverterClient, SecretString,
};
pub use settings::{DocumentConversionSettings, BUILTIN_CONVERTERS};

/// Default maximum size of a downloaded document (25 MB).
///
/// Documents are commonly larger than HTML pages, so this is separate from
/// `max_body_size`. Converted output is still capped by `max_body_size`.
pub const DEFAULT_MAX_DOCUMENT_BYTES: usize = 25 * 1024 * 1024;

/// Binary document formats fetchkit can route to a converter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum DocumentFormat {
    /// Portable Document Format.
    Pdf,
    /// Word (Office Open XML).
    Docx,
    /// Word 97-2003.
    Doc,
    /// Excel (Office Open XML), including macro-enabled `.xlsm`.
    Xlsx,
    /// Excel binary workbook.
    Xlsb,
    /// Excel 97-2003.
    Xls,
    /// PowerPoint (Office Open XML).
    Pptx,
    /// PowerPoint 97-2003.
    Ppt,
    /// OpenDocument text.
    Odt,
    /// OpenDocument spreadsheet.
    Ods,
    /// OpenDocument presentation.
    Odp,
    /// Rich Text Format.
    Rtf,
    /// EPUB e-book.
    Epub,
}

/// How a container format is recognised from its first bytes.
#[derive(Clone, Copy)]
enum Magic {
    Pdf,
    Zip,
    Ole2,
    Rtf,
}

impl DocumentFormat {
    /// Every known format.
    pub const ALL: &'static [DocumentFormat] = &[
        Self::Pdf,
        Self::Docx,
        Self::Doc,
        Self::Xlsx,
        Self::Xlsb,
        Self::Xls,
        Self::Pptx,
        Self::Ppt,
        Self::Odt,
        Self::Ods,
        Self::Odp,
        Self::Rtf,
        Self::Epub,
    ];

    /// Stable lowercase identifier, used in config and CLI flags.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Docx => "docx",
            Self::Doc => "doc",
            Self::Xlsx => "xlsx",
            Self::Xlsb => "xlsb",
            Self::Xls => "xls",
            Self::Pptx => "pptx",
            Self::Ppt => "ppt",
            Self::Odt => "odt",
            Self::Ods => "ods",
            Self::Odp => "odp",
            Self::Rtf => "rtf",
            Self::Epub => "epub",
        }
    }

    /// Canonical media type.
    pub fn media_type(self) -> &'static str {
        self.media_types()[0]
    }

    /// Media types that identify this format.
    pub fn media_types(self) -> &'static [&'static str] {
        match self {
            Self::Pdf => &["application/pdf", "application/x-pdf"],
            Self::Docx => {
                &["application/vnd.openxmlformats-officedocument.wordprocessingml.document"]
            }
            Self::Doc => &["application/msword"],
            Self::Xlsx => &[
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                "application/vnd.ms-excel.sheet.macroenabled.12",
            ],
            Self::Xlsb => &["application/vnd.ms-excel.sheet.binary.macroenabled.12"],
            Self::Xls => &["application/vnd.ms-excel"],
            Self::Pptx => {
                &["application/vnd.openxmlformats-officedocument.presentationml.presentation"]
            }
            Self::Ppt => &["application/vnd.ms-powerpoint"],
            Self::Odt => &["application/vnd.oasis.opendocument.text"],
            Self::Ods => &["application/vnd.oasis.opendocument.spreadsheet"],
            Self::Odp => &["application/vnd.oasis.opendocument.presentation"],
            Self::Rtf => &["application/rtf", "text/rtf"],
            Self::Epub => &["application/epub+zip"],
        }
    }

    /// File extensions (lowercase, without dot) that identify this format.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Pdf => &["pdf"],
            Self::Docx => &["docx"],
            Self::Doc => &["doc"],
            Self::Xlsx => &["xlsx", "xlsm"],
            Self::Xlsb => &["xlsb"],
            Self::Xls => &["xls"],
            Self::Pptx => &["pptx"],
            Self::Ppt => &["ppt"],
            Self::Odt => &["odt"],
            Self::Ods => &["ods"],
            Self::Odp => &["odp"],
            Self::Rtf => &["rtf"],
            Self::Epub => &["epub"],
        }
    }

    /// Format for a media type (parameters such as `charset` are ignored).
    pub fn from_media_type(content_type: &str) -> Option<Self> {
        let media_type = media_type(content_type);
        Self::ALL.iter().copied().find(|format| {
            format
                .media_types()
                .iter()
                .any(|known| known.eq_ignore_ascii_case(media_type))
        })
    }

    /// Format for a file extension (case-insensitive, without dot).
    pub fn from_extension(extension: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|format| {
            format
                .extensions()
                .iter()
                .any(|known| known.eq_ignore_ascii_case(extension))
        })
    }

    /// Whether `body` starts like this format's container.
    pub fn matches_magic(self, body: &[u8]) -> bool {
        match self.magic() {
            // PDF readers accept a header within the first KiB.
            Magic::Pdf => body
                .get(..body.len().min(1024))
                .is_some_and(|head| head.windows(5).any(|window| window == b"%PDF-")),
            Magic::Zip => body.starts_with(b"PK\x03\x04"),
            Magic::Ole2 => body.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]),
            Magic::Rtf => body.starts_with(b"{\\rtf"),
        }
    }

    fn magic(self) -> Magic {
        match self {
            Self::Pdf => Magic::Pdf,
            Self::Doc | Self::Xls | Self::Ppt => Magic::Ole2,
            Self::Rtf => Magic::Rtf,
            Self::Docx
            | Self::Xlsx
            | Self::Xlsb
            | Self::Pptx
            | Self::Odt
            | Self::Ods
            | Self::Odp
            | Self::Epub => Magic::Zip,
        }
    }
}

impl fmt::Display for DocumentFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for DocumentFormat {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::from_extension(value.trim().trim_start_matches('.'))
            .ok_or_else(|| format!("unknown document format: {value}"))
    }
}

fn media_type(content_type: &str) -> &str {
    content_type.split(';').next().unwrap_or_default().trim()
}

// Document parsers are CPU-bound (pdf-inspector also uses Rayon internally); bound
// concurrent local conversions so batch fetches cannot multiply parser work.
#[cfg(any(
    feature = "docs-pdf",
    feature = "docs-markkit",
    feature = "docs-office"
))]
static DOCUMENT_PROCESSING_LIMIT: std::sync::LazyLock<Arc<tokio::sync::Semaphore>> =
    std::sync::LazyLock::new(|| Arc::new(tokio::sync::Semaphore::new(2)));

/// Run CPU-bound conversion on the blocking pool under the shared document limit.
#[cfg(any(
    feature = "docs-pdf",
    feature = "docs-markkit",
    feature = "docs-office"
))]
pub(crate) async fn run_document_task<T, F>(task: F) -> Result<T, ConvertError>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    run_limited(Arc::clone(&DOCUMENT_PROCESSING_LIMIT), task).await
}

#[cfg(any(
    feature = "docs-pdf",
    feature = "docs-markkit",
    feature = "docs-office"
))]
async fn run_limited<T, F>(limit: Arc<tokio::sync::Semaphore>, task: F) -> Result<T, ConvertError>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let permit = limit.acquire_owned().await.map_err(|error| {
        ConvertError::Failed(format!("document processing unavailable: {error}"))
    })?;
    tokio::task::spawn_blocking(move || {
        // Keep the permit in the blocking task: cancelling its async caller must not
        // admit another parse while this non-cancellable work is still running.
        let _permit = permit;
        task()
    })
    .await
    .map_err(|error| ConvertError::Failed(format!("document processor task failed: {error}")))
}

/// Recognises document formats from response metadata.
pub trait FormatDetector: Send + Sync {
    /// Detect the format from the final URL and Content-Type, before download.
    fn detect(&self, url: &Url, content_type: Option<&str>) -> Option<DocumentFormat>;

    /// Confirm the detected format against the downloaded bytes.
    fn confirm(&self, format: DocumentFormat, body: &[u8]) -> bool {
        format.matches_magic(body)
    }
}

/// Content-Type first; URL extension only for missing or generic binary types.
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultFormatDetector;

const GENERIC_BINARY_TYPES: &[&str] = &[
    "",
    "application/octet-stream",
    "binary/octet-stream",
    "application/binary",
    "application/download",
    "application/force-download",
    "application/x-download",
    "application/zip",
];

impl FormatDetector for DefaultFormatDetector {
    fn detect(&self, url: &Url, content_type: Option<&str>) -> Option<DocumentFormat> {
        let media_type = media_type(content_type.unwrap_or_default());
        if let Some(format) = DocumentFormat::from_media_type(media_type) {
            return Some(format);
        }
        if !GENERIC_BINARY_TYPES
            .iter()
            .any(|generic| generic.eq_ignore_ascii_case(media_type))
        {
            return None;
        }
        let file_name = url.path_segments()?.next_back()?;
        let (_, extension) = file_name.rsplit_once('.')?;
        DocumentFormat::from_extension(extension)
    }
}

/// Where a converter runs. Remote converters receive document bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locality {
    /// Converts in-process or on the same host.
    Local,
    /// Sends document bytes to another service.
    Remote,
}

/// Bounded document bytes handed to a [`DocumentConverter`].
#[derive(Debug, Clone)]
pub struct DocumentInput {
    /// Final URL after redirects (informational; converters must not fetch it).
    pub url: Url,
    /// Detected and magic-byte-confirmed format.
    pub format: DocumentFormat,
    /// Response Content-Type, when present.
    pub content_type: Option<String>,
    /// File name derived from the URL, with an extension matching `format`.
    pub file_name: String,
    /// Complete document bytes. Truncated downloads are never converted.
    pub body: Bytes,
}

/// Markdown produced by a [`DocumentConverter`].
#[derive(Debug, Clone, Default)]
pub struct ConvertedDocument {
    /// Converted Markdown. Empty means nothing extractable was found.
    pub markdown: String,
    /// Document title, when known.
    pub title: Option<String>,
    /// Machine-readable warnings, such as `pdf_encoding_issues`.
    pub warnings: Vec<String>,
    /// 1-based pages that need OCR to extract text.
    pub ocr_pages: Vec<u32>,
    /// Response `extraction_method`. Defaults to the converter name with `_` for `-`.
    pub extraction_method: Option<String>,
}

impl ConvertedDocument {
    /// Converted Markdown with no title or warnings.
    pub fn markdown(markdown: impl Into<String>) -> Self {
        Self {
            markdown: markdown.into(),
            ..Default::default()
        }
    }

    fn is_usable(&self) -> bool {
        !self.markdown.trim().is_empty() && self.ocr_pages.is_empty()
    }
}

/// Why a converter could not produce a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConvertError {
    /// The converter does not handle this input; try the next one.
    Unsupported(String),
    /// Input exceeds this converter's size limit.
    TooLarge {
        /// Input size in bytes.
        size: usize,
        /// Converter limit in bytes.
        limit: usize,
    },
    /// The conversion failed.
    Failed(String),
}

impl fmt::Display for ConvertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(message) => write!(f, "unsupported: {message}"),
            Self::TooLarge { size, limit } => {
                write!(f, "document is {size} bytes, converter limit is {limit}")
            }
            Self::Failed(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ConvertError {}

/// One way to turn document bytes into Markdown.
#[async_trait]
pub trait DocumentConverter: Send + Sync {
    /// Unique name used in chain config, logs and `extraction_method`.
    fn name(&self) -> &str;

    /// Where the conversion runs.
    fn locality(&self) -> Locality;

    /// Whether this converter handles `format`.
    fn supports(&self, format: DocumentFormat) -> bool;

    /// Largest input this converter accepts, if it has its own limit.
    fn max_input_bytes(&self) -> Option<usize> {
        None
    }

    /// Convert a complete, bounded document.
    async fn convert(&self, input: DocumentInput) -> Result<ConvertedDocument, ConvertError>;
}

/// Which formats are converted, by which backends, in what order.
///
/// The default contains the built-in local converters enabled by Cargo features, in
/// this order: `docs-pdf` adds `PdfInspectorConverter`, `docs-markkit` adds
/// `MarkkitConverter` (the default Office backend), and the deprecated
/// `docs-office` adds `OfficeConverter` last, so with both Office features enabled
/// markkit runs first and `office` is the fallback (and the only PPTX/XLSB backend).
/// It accepts every format they support.
#[derive(Clone)]
pub struct DocumentConversion {
    converters: Vec<Arc<dyn DocumentConverter>>,
    formats: Option<Vec<DocumentFormat>>,
    chains: BTreeMap<DocumentFormat, Vec<String>>,
    max_document_bytes: usize,
    allow_remote: bool,
    detector: Arc<dyn FormatDetector>,
}

impl Default for DocumentConversion {
    fn default() -> Self {
        let conversion = Self::empty();
        #[cfg(feature = "docs-pdf")]
        let conversion = conversion.converter(PdfInspectorConverter::new());
        #[cfg(feature = "docs-markkit")]
        let conversion = conversion.converter(MarkkitConverter::new());
        #[cfg(feature = "docs-office")]
        #[allow(deprecated)]
        let conversion = conversion.converter(OfficeConverter::new());
        conversion
    }
}

impl fmt::Debug for DocumentConversion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DocumentConversion")
            .field(
                "converters",
                &self
                    .converters
                    .iter()
                    .map(|converter| converter.name().to_string())
                    .collect::<Vec<_>>(),
            )
            .field("formats", &self.formats)
            .field("chains", &self.chains)
            .field("max_document_bytes", &self.max_document_bytes)
            .field("allow_remote", &self.allow_remote)
            .finish()
    }
}

impl DocumentConversion {
    /// Configuration with no converters: every document is rejected as binary.
    pub fn empty() -> Self {
        Self {
            converters: Vec::new(),
            formats: None,
            chains: BTreeMap::new(),
            max_document_bytes: DEFAULT_MAX_DOCUMENT_BYTES,
            allow_remote: false,
            detector: Arc::new(DefaultFormatDetector),
        }
    }

    /// Register a converter. Without an explicit chain, converters run in
    /// registration order. A converter with an existing name replaces it in place.
    pub fn converter(self, converter: impl DocumentConverter + 'static) -> Self {
        self.converter_arc(Arc::new(converter))
    }

    /// Register a shared converter. See [`converter`](Self::converter).
    pub fn converter_arc(mut self, converter: Arc<dyn DocumentConverter>) -> Self {
        match self
            .converters
            .iter_mut()
            .find(|existing| existing.name() == converter.name())
        {
            Some(existing) => *existing = converter,
            None => self.converters.push(converter),
        }
        self
    }

    /// Accept only these formats. Others are rejected as binary.
    pub fn formats(mut self, formats: impl IntoIterator<Item = DocumentFormat>) -> Self {
        self.formats = Some(formats.into_iter().collect());
        self
    }

    /// Set the converter order for one format, by converter name.
    pub fn chain<I, S>(mut self, format: DocumentFormat, converters: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.chains
            .insert(format, converters.into_iter().map(Into::into).collect());
        self
    }

    /// Maximum downloaded document size in bytes.
    pub fn max_document_bytes(mut self, bytes: usize) -> Self {
        self.max_document_bytes = bytes;
        self
    }

    /// Allow converters that send document bytes to another service.
    pub fn allow_remote(mut self, allow: bool) -> Self {
        self.allow_remote = allow;
        self
    }

    /// Replace format detection.
    pub fn detector(mut self, detector: impl FormatDetector + 'static) -> Self {
        self.detector = Arc::new(detector);
        self
    }

    /// Whether remote converters may run.
    pub fn remote_allowed(&self) -> bool {
        self.allow_remote
    }

    /// Registered converter names in registration order.
    pub fn converter_names(&self) -> Vec<&str> {
        self.converters
            .iter()
            .map(|converter| converter.name())
            .collect()
    }

    /// Converters that will run for `format`, in order, after remote filtering.
    pub fn chain_for(&self, format: DocumentFormat) -> Vec<Arc<dyn DocumentConverter>> {
        if self
            .formats
            .as_ref()
            .is_some_and(|formats| !formats.contains(&format))
        {
            return Vec::new();
        }
        let candidates: Vec<Arc<dyn DocumentConverter>> = match self.chains.get(&format) {
            Some(names) => names
                .iter()
                .filter_map(|name| {
                    let found = self
                        .converters
                        .iter()
                        .find(|converter| converter.name() == name);
                    if found.is_none() {
                        warn!(converter = %name, %format, "Unknown converter in document chain");
                    }
                    found.cloned()
                })
                .collect(),
            None => self.converters.clone(),
        };
        candidates
            .into_iter()
            .filter(|converter| converter.supports(format))
            .filter(|converter| self.allow_remote || converter.locality() == Locality::Local)
            .collect()
    }

    /// Detect a format this configuration will convert.
    pub fn detect(&self, url: &Url, content_type: Option<&str>) -> Option<DocumentFormat> {
        self.detector
            .detect(url, content_type)
            .filter(|format| !self.chain_for(*format).is_empty())
    }
}

/// [`ContentProcessor`] that converts documents through a [`DocumentConversion`] chain.
#[derive(Debug, Clone, Default)]
pub struct DocumentProcessor {
    conversion: DocumentConversion,
}

impl DocumentProcessor {
    /// Create a processor for a conversion configuration.
    pub fn new(conversion: DocumentConversion) -> Self {
        Self { conversion }
    }
}

#[async_trait]
impl ContentProcessor for DocumentProcessor {
    fn name(&self) -> &'static str {
        "document"
    }

    fn matches(&self, url: &Url, content_type: Option<&str>) -> bool {
        self.conversion.detect(url, content_type).is_some()
    }

    fn max_input_bytes(&self) -> Option<usize> {
        Some(self.conversion.max_document_bytes)
    }

    async fn process(
        &self,
        input: ContentProcessorInput,
    ) -> Result<ProcessedContent, ContentProcessorError> {
        debug_assert_eq!(input.output_format, ContentOutputFormat::Markdown);
        let format = self
            .conversion
            .detect(&input.url, input.content_type.as_deref())
            .ok_or_else(|| ContentProcessorError("document format not enabled".to_string()))?;
        if !self.conversion.detector.confirm(format, &input.body) {
            return Err(ContentProcessorError(format!(
                "response body is not a valid {} document",
                format.as_str().to_ascii_uppercase()
            )));
        }

        let document = DocumentInput {
            file_name: file_name(&input.url, format),
            url: input.url,
            format,
            content_type: input.content_type,
            body: input.body,
        };
        let size = document.body.len();
        let mut failures = Vec::new();
        let mut fallback: Option<(String, ConvertedDocument)> = None;

        for (index, converter) in self.conversion.chain_for(format).iter().enumerate() {
            let name = converter.name().to_string();
            if let Some(limit) = converter.max_input_bytes().filter(|limit| size > *limit) {
                failures.push(format!(
                    "{name}: {}",
                    ConvertError::TooLarge { size, limit }
                ));
                continue;
            }
            debug!(converter = %name, %format, size, "Converting document");
            match converter.convert(document.clone()).await {
                Ok(mut converted) => {
                    if index > 0 {
                        converted.warnings.push("fallback_converter".to_string());
                    }
                    if converted.is_usable() {
                        return Ok(processed(&name, converted));
                    }
                    // Keep the first partial result (e.g. OCR-required) in case no
                    // later converter does better.
                    fallback.get_or_insert((name, converted));
                }
                Err(error) => failures.push(format!("{name}: {error}")),
            }
        }

        match fallback {
            Some((name, converted)) => Ok(processed(&name, converted)),
            None if failures.is_empty() => Err(ContentProcessorError(format!(
                "no converter enabled for {format}"
            ))),
            None => Err(ContentProcessorError(failures.join("; "))),
        }
    }
}

fn file_name(url: &Url, format: DocumentFormat) -> String {
    let last = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .map(|segment| {
            percent_encoding::percent_decode_str(segment)
                .decode_utf8_lossy()
                .into_owned()
        })
        .unwrap_or_default();
    let safe: String = last
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
        .take(100)
        .collect();
    let has_extension = safe.rsplit_once('.').is_some_and(|(stem, extension)| {
        !stem.is_empty() && DocumentFormat::from_extension(extension) == Some(format)
    });
    if has_extension {
        safe
    } else {
        format!("document.{}", format.extensions()[0])
    }
}

fn processed(converter: &str, converted: ConvertedDocument) -> ProcessedContent {
    let extraction_method = converted
        .extraction_method
        .unwrap_or_else(|| converter.replace('-', "_"));
    let needs_ocr = !converted.ocr_pages.is_empty();
    let mut warnings = converted.warnings;
    if needs_ocr {
        warnings.insert(0, "pdf_requires_ocr".to_string());
    }
    let content = Some(converted.markdown).filter(|value| !value.trim().is_empty());
    let error = content.is_none().then(|| {
        if needs_ocr {
            format!(
                "Document requires OCR for pages: {}",
                converted
                    .ocr_pages
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        } else {
            "Document contains no extractable text".to_string()
        }
    });
    let degraded = needs_ocr || warnings.iter().any(|warning| warning.ends_with("_issues"));
    let score = if content.is_none() {
        0.0
    } else if degraded {
        0.6
    } else if warnings
        .iter()
        .any(|warning| warning != "fallback_converter")
    {
        0.8
    } else {
        1.0
    };

    ProcessedContent {
        format: content.as_ref().map(|_| "markdown".to_string()),
        content,
        metadata: Some(PageMetadata {
            title: converted.title,
            extraction_method: Some(extraction_method.clone()),
            ..Default::default()
        }),
        quality: Some(PageQuality {
            score,
            warnings,
            extraction_method: Some(extraction_method),
            suggested_next_action: needs_ocr.then(|| "use_ocr".to_string()),
            ..Default::default()
        }),
        error,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::content::ContentFocus;

    struct Stub {
        name: &'static str,
        locality: Locality,
        result: Result<ConvertedDocument, ConvertError>,
        calls: AtomicUsize,
    }

    impl Stub {
        fn new(name: &'static str, result: Result<ConvertedDocument, ConvertError>) -> Self {
            Self {
                name,
                locality: Locality::Local,
                result,
                calls: AtomicUsize::new(0),
            }
        }

        fn remote(mut self) -> Self {
            self.locality = Locality::Remote;
            self
        }
    }

    #[async_trait]
    impl DocumentConverter for Stub {
        fn name(&self) -> &str {
            self.name
        }
        fn locality(&self) -> Locality {
            self.locality
        }
        fn supports(&self, format: DocumentFormat) -> bool {
            matches!(format, DocumentFormat::Pdf | DocumentFormat::Docx)
        }
        async fn convert(&self, input: DocumentInput) -> Result<ConvertedDocument, ConvertError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            assert!(input.file_name.ends_with(input.format.extensions()[0]));
            self.result.clone()
        }
    }

    fn input(url: &str, content_type: &str, body: &'static [u8]) -> ContentProcessorInput {
        ContentProcessorInput {
            url: Url::parse(url).unwrap(),
            content_type: Some(content_type.to_string()),
            body: Bytes::from_static(body),
            output_format: ContentOutputFormat::Markdown,
            content_focus: ContentFocus::Full,
        }
    }

    const DOCX: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";

    #[test]
    fn detects_by_media_type_then_extension_for_generic_types() {
        let detector = DefaultFormatDetector;
        let url = Url::parse("https://example.com/files/Report.DOCX?x=1").unwrap();
        assert_eq!(
            detector.detect(&url, Some("application/pdf; charset=binary")),
            Some(DocumentFormat::Pdf)
        );
        assert_eq!(
            detector.detect(&url, Some("application/octet-stream")),
            Some(DocumentFormat::Docx)
        );
        assert_eq!(detector.detect(&url, None), Some(DocumentFormat::Docx));
        assert_eq!(detector.detect(&url, Some("text/html")), None);
        assert_eq!(
            detector.detect(
                &Url::parse("https://example.com/a.xlsm").unwrap(),
                Some("application/vnd.ms-excel.sheet.macroEnabled.12")
            ),
            Some(DocumentFormat::Xlsx)
        );
    }

    #[test]
    fn magic_bytes_confirm_container_type() {
        assert!(DocumentFormat::Pdf.matches_magic(b"%PDF-1.7"));
        assert!(DocumentFormat::Docx.matches_magic(b"PK\x03\x04rest"));
        assert!(!DocumentFormat::Docx.matches_magic(b"%PDF-1.7"));
        assert!(
            DocumentFormat::Xls.matches_magic(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1])
        );
        assert!(!DocumentFormat::Pdf.matches_magic(b"<html>"));
    }

    #[test]
    fn format_parses_from_config_strings() {
        assert_eq!("DOCX".parse::<DocumentFormat>(), Ok(DocumentFormat::Docx));
        assert_eq!(".xlsm".parse::<DocumentFormat>(), Ok(DocumentFormat::Xlsx));
        assert!("exe".parse::<DocumentFormat>().is_err());
    }

    #[test]
    fn remote_converters_are_skipped_unless_allowed() {
        let conversion = DocumentConversion::empty()
            .converter(Stub::new("remote", Ok(ConvertedDocument::markdown("x"))).remote());
        let url = Url::parse("https://example.com/a").unwrap();
        assert!(conversion.detect(&url, Some(DOCX)).is_none());
        let conversion = conversion.allow_remote(true);
        assert_eq!(
            conversion.detect(&url, Some(DOCX)),
            Some(DocumentFormat::Docx)
        );
    }

    #[test]
    fn formats_filter_and_explicit_chain_order_apply() {
        let conversion = DocumentConversion::empty()
            .converter(Stub::new("a", Ok(ConvertedDocument::markdown("a"))))
            .converter(Stub::new("b", Ok(ConvertedDocument::markdown("b"))))
            .chain(DocumentFormat::Pdf, ["b", "missing", "a"])
            .formats([DocumentFormat::Pdf]);
        let names: Vec<String> = conversion
            .chain_for(DocumentFormat::Pdf)
            .iter()
            .map(|converter| converter.name().to_string())
            .collect();
        assert_eq!(names, ["b", "a"]);
        assert!(conversion.chain_for(DocumentFormat::Docx).is_empty());
    }

    #[tokio::test]
    async fn falls_through_on_error_and_ocr_results() {
        let ocr = ConvertedDocument {
            ocr_pages: vec![1],
            ..Default::default()
        };
        let processor = DocumentProcessor::new(
            DocumentConversion::empty()
                .converter(Stub::new(
                    "broken",
                    Err(ConvertError::Failed("boom".into())),
                ))
                .converter(Stub::new("local-ocr", Ok(ocr)))
                .converter(Stub::new("good", Ok(ConvertedDocument::markdown("# Done")))),
        );
        let result = processor
            .process(input(
                "https://example.com/a.pdf",
                "application/pdf",
                b"%PDF-1.4",
            ))
            .await
            .unwrap();
        assert_eq!(result.content.as_deref(), Some("# Done"));
        let quality = result.quality.unwrap();
        assert_eq!(quality.extraction_method.as_deref(), Some("good"));
        assert_eq!(quality.warnings, ["fallback_converter"]);
    }

    #[tokio::test]
    async fn returns_ocr_hint_when_no_converter_does_better() {
        let ocr = ConvertedDocument {
            ocr_pages: vec![2, 3],
            ..Default::default()
        };
        let processor = DocumentProcessor::new(
            DocumentConversion::empty().converter(Stub::new("only", Ok(ocr))),
        );
        let result = processor
            .process(input(
                "https://example.com/a",
                "application/pdf",
                b"%PDF-1.4",
            ))
            .await
            .unwrap();
        assert!(result.content.is_none());
        assert_eq!(
            result.error.as_deref(),
            Some("Document requires OCR for pages: 2, 3")
        );
        assert_eq!(
            result.quality.unwrap().suggested_next_action.as_deref(),
            Some("use_ocr")
        );
    }

    #[tokio::test]
    async fn rejects_body_that_does_not_match_detected_format() {
        let stub = Arc::new(Stub::new("a", Ok(ConvertedDocument::markdown("x"))));
        let processor =
            DocumentProcessor::new(DocumentConversion::empty().converter_arc(stub.clone()));
        let error = processor
            .process(input(
                "https://example.com/a",
                DOCX,
                b"<html>not a docx</html>",
            ))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("not a valid DOCX"));
        assert_eq!(stub.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn skips_converter_whose_limit_is_exceeded() {
        struct Small;
        #[async_trait]
        impl DocumentConverter for Small {
            fn name(&self) -> &str {
                "small"
            }
            fn locality(&self) -> Locality {
                Locality::Local
            }
            fn supports(&self, _format: DocumentFormat) -> bool {
                true
            }
            fn max_input_bytes(&self) -> Option<usize> {
                Some(2)
            }
            async fn convert(
                &self,
                _input: DocumentInput,
            ) -> Result<ConvertedDocument, ConvertError> {
                unreachable!("limit must be enforced before convert")
            }
        }
        let processor = DocumentProcessor::new(DocumentConversion::empty().converter(Small));
        let error = processor
            .process(input(
                "https://example.com/a.pdf",
                "application/pdf",
                b"%PDF-1.4",
            ))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("converter limit is 2"));
    }

    #[cfg(any(
        feature = "docs-pdf",
        feature = "docs-markkit",
        feature = "docs-office"
    ))]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancelled_caller_does_not_release_running_task_permit() {
        use std::sync::mpsc;

        let limit = Arc::new(tokio::sync::Semaphore::new(1));
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let handle = tokio::spawn(run_limited(Arc::clone(&limit), move || {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        }));

        started_rx.recv().unwrap();
        handle.abort();
        assert_eq!(limit.available_permits(), 0);

        release_tx.send(()).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while limit.available_permits() == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    #[test]
    fn file_name_is_sanitised_and_keeps_matching_extension() {
        let url = Url::parse("https://example.com/a/Q3%20report%3B.docx").unwrap();
        assert_eq!(file_name(&url, DocumentFormat::Docx), "Q3report.docx");
        let url = Url::parse("https://example.com/download?id=7").unwrap();
        assert_eq!(file_name(&url, DocumentFormat::Xlsx), "document.xlsx");
        let url = Url::parse("https://example.com/.pdf").unwrap();
        assert_eq!(file_name(&url, DocumentFormat::Pdf), "document.pdf");
    }
}
