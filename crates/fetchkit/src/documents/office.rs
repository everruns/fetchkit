//! Office document extraction: Word (DOCX), PowerPoint (PPTX) and spreadsheets
//! (XLSX, XLS, ODS) to Markdown.
//!
//! Decisions:
//! - Everything is parsed locally from the bounded response bytes. Documents never
//!   leave the host, and no account or network call is involved, so the processor
//!   behaves the same in every embedding (CLI, MCP, library, hosted runtimes).
//! - DOCX and PPTX text is read straight from their WordprocessingML / DrawingML XML
//!   parts with `quick-xml`; full fidelity (styles, images, footnotes) is not a goal,
//!   readable text with headings, lists and tables is. Spreadsheets go through
//!   `calamine`, which also covers legacy binary XLS.
//! - THREAT[TM-DOS-008]: OOXML and ODS files are zip archives. Before any part is
//!   inflated, the central directory is checked against an entry-count and a total
//!   uncompressed-size budget, and every part fetchkit reads itself is read through a
//!   size-capped reader, so a small zip bomb within `max_body_size` cannot expand
//!   without limit. DOCX tables reuse the sheet dimensions and a checked padded-cell
//!   budget. DOCX Markdown is admitted into one 10 MiB buffer before escaping or
//!   padding; the caller may apply a smaller `max_body_size`. Parsing runs on a blocking
//!   task under the same two-document concurrency cap as PDFs.
//! - Deprecated in favour of `MarkkitConverter` (`docs-markkit`), the default Office
//!   backend. Kept behind the non-default `docs-office` feature for one release; it
//!   is the only built-in PPTX and XLSB converter.
#![allow(deprecated)]

use std::borrow::Cow;
use std::io::{Cursor, Read};

use async_trait::async_trait;
use calamine::{open_workbook_auto_from_rs, Data, Reader};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader as XmlReader;

use super::tables::{TableLimits as DocxLimits, TABLE_LIMITS as DOCX_LIMITS};
use super::{
    run_document_task, ConvertError, ConvertedDocument, DocumentConverter, DocumentFormat,
    DocumentInput, Locality,
};
use crate::content::ContentProcessorError;

/// Maximum number of entries in an Office zip archive.
const MAX_ARCHIVE_ENTRIES: usize = 10_000;
/// Maximum total uncompressed size of an Office zip archive.
const MAX_ARCHIVE_UNCOMPRESSED_BYTES: u64 = 200 * 1024 * 1024;
/// Maximum uncompressed size of one XML part fetchkit reads itself.
const MAX_PART_BYTES: u64 = 64 * 1024 * 1024;
/// Maximum slides read from one presentation.
const MAX_SLIDES: usize = 1_000;
/// Maximum rows rendered per spreadsheet sheet.
const MAX_SHEET_ROWS: usize = DOCX_LIMITS.rows;
/// Maximum columns rendered per spreadsheet sheet.
const MAX_SHEET_COLUMNS: usize = DOCX_LIMITS.columns;

/// Office document kinds with distinct extraction paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OfficeKind {
    Docx,
    Pptx,
    Spreadsheet,
}

impl OfficeKind {
    fn from_format(format: DocumentFormat) -> Option<Self> {
        match format {
            DocumentFormat::Docx => Some(Self::Docx),
            DocumentFormat::Pptx => Some(Self::Pptx),
            DocumentFormat::Xlsx
            | DocumentFormat::Xlsb
            | DocumentFormat::Xls
            | DocumentFormat::Ods => Some(Self::Spreadsheet),
            _ => None,
        }
    }

    fn extraction_method(self) -> &'static str {
        match self {
            Self::Docx => "office_docx",
            Self::Pptx => "office_pptx",
            Self::Spreadsheet => "office_spreadsheet",
        }
    }
}

/// Converts Word (DOCX), PowerPoint (PPTX) and spreadsheets (XLSX, XLSM, XLSB,
/// XLS, ODS) to Markdown in-process.
///
/// Deprecated: use `MarkkitConverter` (`docs-markkit` feature), the default Office
/// backend. This converter remains available behind the non-default `docs-office`
/// feature and is the only built-in PPTX and XLSB backend.
#[deprecated(note = "use MarkkitConverter (docs-markkit)")]
#[derive(Debug, Default, Clone, Copy)]
pub struct OfficeConverter;

impl OfficeConverter {
    /// Create the converter.
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl DocumentConverter for OfficeConverter {
    fn name(&self) -> &str {
        "office"
    }

    fn locality(&self) -> Locality {
        Locality::Local
    }

    fn supports(&self, format: DocumentFormat) -> bool {
        OfficeKind::from_format(format).is_some()
    }

    async fn convert(&self, input: DocumentInput) -> Result<ConvertedDocument, ConvertError> {
        let kind = OfficeKind::from_format(input.format)
            .ok_or_else(|| ConvertError::Unsupported(input.format.to_string()))?;
        let body = input.body;
        let extracted = run_document_task(move || extract(kind, &body))
            .await?
            .map_err(|error| ConvertError::Failed(error.0))?;
        Ok(ConvertedDocument {
            markdown: extracted.markdown,
            title: extracted.title,
            warnings: extracted.warnings,
            ocr_pages: Vec::new(),
            extraction_method: Some(kind.extraction_method().to_string()),
        })
    }
}

#[derive(Debug, Default)]
struct Extracted {
    markdown: String,
    title: Option<String>,
    warnings: Vec<String>,
}

fn extract(kind: OfficeKind, body: &[u8]) -> Result<Extracted, ContentProcessorError> {
    match kind {
        OfficeKind::Docx => {
            let mut archive = open_archive(body)?;
            let document = read_part(&mut archive, "word/document.xml")?
                .ok_or_else(|| ContentProcessorError("DOCX has no word/document.xml".into()))?;
            Ok(Extracted {
                markdown: docx_to_markdown(&document)?,
                title: core_title(&mut archive),
                warnings: Vec::new(),
            })
        }
        OfficeKind::Pptx => {
            let mut archive = open_archive(body)?;
            pptx_to_markdown(&mut archive)
        }
        OfficeKind::Spreadsheet => {
            // XLS is a compound binary file, not a zip; only zip containers get the
            // archive budget check.
            if body.starts_with(b"PK") {
                let mut archive = open_archive(body)?;
                let title = core_title(&mut archive);
                let mut extracted = spreadsheet_to_markdown(body)?;
                extracted.title = title;
                Ok(extracted)
            } else {
                spreadsheet_to_markdown(body)
            }
        }
    }
}

type Archive<'a> = zip::ZipArchive<Cursor<&'a [u8]>>;

fn open_archive(body: &[u8]) -> Result<Archive<'_>, ContentProcessorError> {
    open_archive_within(body, MAX_ARCHIVE_UNCOMPRESSED_BYTES)
}

fn open_archive_within(body: &[u8], budget: u64) -> Result<Archive<'_>, ContentProcessorError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(body))
        .map_err(|error| ContentProcessorError(format!("invalid Office archive: {error}")))?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(ContentProcessorError(format!(
            "Office archive has too many entries ({})",
            archive.len()
        )));
    }
    let mut total: u64 = 0;
    for index in 0..archive.len() {
        let entry = archive
            .by_index_raw(index)
            .map_err(|error| ContentProcessorError(format!("invalid Office archive: {error}")))?;
        total = total.saturating_add(entry.size());
    }
    if total > budget {
        return Err(ContentProcessorError(format!(
            "Office archive expands to {total} bytes, over the {budget} byte limit"
        )));
    }
    Ok(archive)
}

/// Read one archive part as UTF-8, or `None` when the part does not exist.
fn read_part(
    archive: &mut Archive<'_>,
    name: &str,
) -> Result<Option<String>, ContentProcessorError> {
    let entry = match archive.by_name(name) {
        Ok(entry) => entry,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(error) => {
            return Err(ContentProcessorError(format!(
                "cannot read Office part {name}: {error}"
            )))
        }
    };
    // The declared size was budgeted in open_archive, but declared sizes can lie;
    // cap what is actually inflated.
    let mut bytes = Vec::new();
    entry
        .take(MAX_PART_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            ContentProcessorError(format!("cannot read Office part {name}: {error}"))
        })?;
    if bytes.len() as u64 > MAX_PART_BYTES {
        return Err(ContentProcessorError(format!(
            "Office part {name} is larger than {MAX_PART_BYTES} bytes"
        )));
    }
    Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
}

/// `dc:title` from `docProps/core.xml`, when present.
fn core_title(archive: &mut Archive<'_>) -> Option<String> {
    let core = read_part(archive, "docProps/core.xml").ok()??;
    let mut reader = XmlReader::from_str(&core);
    let mut in_title = false;
    let mut title = String::new();
    loop {
        match reader.read_event().ok()? {
            Event::Start(start) if start.local_name().as_ref() == b"title" => in_title = true,
            Event::End(end) if end.local_name().as_ref() == b"title" => break,
            Event::Text(text) if in_title => title.push_str(&text.xml10_content().ok()?),
            Event::GeneralRef(reference) if in_title => push_reference(&mut title, &reference),
            Event::Eof => break,
            _ => {}
        }
    }
    let title = title.trim();
    (!title.is_empty()).then(|| title.to_string())
}

fn push_reference(out: &mut String, reference: &quick_xml::events::BytesRef<'_>) {
    if let Ok(Some(character)) = reference.resolve_char_ref() {
        out.push(character);
        return;
    }
    let name = reference.decode().unwrap_or(Cow::Borrowed(""));
    out.push_str(match name.as_ref() {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        _ => "",
    });
}

fn attribute(start: &BytesStart<'_>, local: &[u8]) -> Option<String> {
    start.attributes().flatten().find_map(|attr| {
        (attr.key.local_name().as_ref() == local)
            .then(|| {
                attr.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                    .ok()
                    .map(Cow::into_owned)
            })
            .flatten()
    })
}

fn xml_error(error: quick_xml::Error) -> ContentProcessorError {
    ContentProcessorError(format!("invalid Office XML: {error}"))
}

/// Paragraph being assembled from WordprocessingML runs.
#[derive(Default)]
struct DocxParagraph {
    text: String,
    heading: Option<usize>,
    list_level: Option<usize>,
}

impl DocxParagraph {
    fn render(self, limit: usize) -> Result<Option<String>, ContentProcessorError> {
        let text = self.text.trim();
        if text.is_empty() {
            return Ok(None);
        }
        let prefix = match (self.heading, self.list_level) {
            (Some(level), _) => format!("{} ", "#".repeat(level)),
            (None, Some(level)) => format!("{}- ", "  ".repeat(level)),
            (None, None) => String::new(),
        };
        let mut rendered = prefix;
        append_bounded(&mut rendered, text, limit)?;
        Ok(Some(rendered))
    }
}

/// Heading level for a WordprocessingML paragraph style id.
fn heading_level(style: &str) -> Option<usize> {
    let style = style.to_ascii_lowercase();
    if style == "title" {
        return Some(1);
    }
    let level = style
        .strip_prefix("heading")?
        .trim_start_matches(['_', ' ', '-']);
    level
        .parse::<usize>()
        .ok()
        .filter(|level| (1..=6).contains(level))
}

fn docx_to_markdown(xml: &str) -> Result<String, ContentProcessorError> {
    docx_to_markdown_with_limits(xml, DOCX_LIMITS)
}

fn docx_to_markdown_with_limits(
    xml: &str,
    limits: DocxLimits,
) -> Result<String, ContentProcessorError> {
    let mut reader = XmlReader::from_str(xml);
    let mut output = String::new();
    let mut paragraph: Option<DocxParagraph> = None;
    let mut in_text = false;
    // Tables: rows of cells, each cell a list of paragraph texts. Nested tables are
    // flattened into their parent cell.
    let mut table_depth = 0usize;
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut cell: Option<String> = None;
    let mut width = 0;
    let mut source_cells = 0;
    let mut table_text_bytes = 0;

    loop {
        match reader.read_event().map_err(xml_error)? {
            Event::Start(start) => match start.local_name().as_ref() {
                b"p" => paragraph = Some(DocxParagraph::default()),
                b"t" => in_text = true,
                b"tbl" => {
                    table_depth += 1;
                    if table_depth == 1 {
                        rows.clear();
                        width = 0;
                        source_cells = 0;
                        table_text_bytes = 0;
                    }
                }
                b"tr" if table_depth == 1 => {
                    check_table_shape(rows.len() + 1, width, limits)?;
                    rows.push(Vec::new());
                }
                b"tc" if table_depth > 0 => {
                    source_cells = checked_size(source_cells, 1, limits.cells)?;
                    if table_depth == 1 {
                        width = width.max(rows.last().map_or(1, |row| row.len() + 1));
                        check_table_shape(rows.len(), width, limits)?;
                        cell = Some(String::new());
                    }
                }
                _ => docx_property(&start, &mut paragraph, limits.output_bytes)?,
            },
            // Self-closing elements never open a container: an empty `<w:p/>` or
            // `<w:t/>` has no matching End event.
            Event::Empty(start) => docx_property(&start, &mut paragraph, limits.output_bytes)?,
            Event::Text(text) if in_text => {
                let text = text.xml10_content().map_err(|e| xml_error(e.into()))?;
                if let Some(p) = &mut paragraph {
                    append_bounded(&mut p.text, &text, limits.output_bytes)?;
                }
            }
            Event::GeneralRef(reference) if in_text => {
                if let Some(p) = &mut paragraph {
                    let mut text = String::new();
                    push_reference(&mut text, &reference);
                    append_bounded(&mut p.text, &text, limits.output_bytes)?;
                }
            }
            Event::End(end) => match end.local_name().as_ref() {
                b"t" => in_text = false,
                b"p" => {
                    if let Some(rendered) = paragraph
                        .take()
                        .map(|p| p.render(limits.output_bytes))
                        .transpose()?
                        .flatten()
                    {
                        match &mut cell {
                            Some(cell) => {
                                let separator = if cell.is_empty() { "" } else { "<br>" };
                                table_text_bytes = checked_size(
                                    table_text_bytes,
                                    separator.len(),
                                    limits.output_bytes,
                                )?;
                                table_text_bytes = checked_size(
                                    table_text_bytes,
                                    rendered.len(),
                                    limits.output_bytes,
                                )?;
                                append_bounded(cell, separator, limits.output_bytes)?;
                                append_bounded(cell, &rendered, limits.output_bytes)?;
                            }
                            None => append_block(&mut output, &rendered, limits.output_bytes)?,
                        }
                    }
                }
                b"tc" if table_depth == 1 => {
                    if let (Some(done), Some(row)) = (cell.take(), rows.last_mut()) {
                        row.push(done);
                    }
                }
                b"tbl" => {
                    table_depth = table_depth.saturating_sub(1);
                    if table_depth == 0 {
                        append_table(&rows, &mut output, limits)?;
                        rows.clear();
                    }
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(output)
}

/// Paragraph properties and inline breaks, which may be self-closing.
fn docx_property(
    start: &BytesStart<'_>,
    paragraph: &mut Option<DocxParagraph>,
    limit: usize,
) -> Result<(), ContentProcessorError> {
    let Some(p) = paragraph else {
        return Ok(());
    };
    match start.local_name().as_ref() {
        b"pStyle" => {
            if let Some(style) = attribute(start, b"val") {
                p.heading = heading_level(&style);
            }
        }
        b"numPr" => {
            p.list_level.get_or_insert(0);
        }
        b"ilvl" => {
            let level = attribute(start, b"val")
                .and_then(|value| value.parse().ok())
                .unwrap_or(0usize);
            p.list_level = Some(level.min(8));
        }
        b"tab" => append_bounded(&mut p.text, "\t", limit)?,
        b"br" | b"cr" => append_bounded(&mut p.text, "\n", limit)?,
        _ => {}
    }
    Ok(())
}

/// Slide part names in presentation order, read from `ppt/presentation.xml`
/// and its relationships; falls back to numeric file order when either is missing.
fn slide_parts(archive: &mut Archive<'_>) -> Result<Vec<String>, ContentProcessorError> {
    let ordered = (|| -> Result<Option<Vec<String>>, ContentProcessorError> {
        let Some(presentation) = read_part(archive, "ppt/presentation.xml")? else {
            return Ok(None);
        };
        let Some(rels) = read_part(archive, "ppt/_rels/presentation.xml.rels")? else {
            return Ok(None);
        };
        let mut targets = std::collections::HashMap::new();
        let mut reader = XmlReader::from_str(&rels);
        loop {
            match reader.read_event().map_err(xml_error)? {
                Event::Start(start) | Event::Empty(start)
                    if start.local_name().as_ref() == b"Relationship" =>
                {
                    if let (Some(id), Some(target)) =
                        (attribute(&start, b"Id"), attribute(&start, b"Target"))
                    {
                        targets.insert(id, target);
                    }
                }
                Event::Eof => break,
                _ => {}
            }
        }
        let mut slides = Vec::new();
        let mut reader = XmlReader::from_str(&presentation);
        loop {
            match reader.read_event().map_err(xml_error)? {
                Event::Start(start) | Event::Empty(start)
                    if start.local_name().as_ref() == b"sldId" =>
                {
                    if let Some(target) = relationship_id(&start).and_then(|id| targets.get(&id)) {
                        let target = target.trim_start_matches('/');
                        let part = if target.starts_with("ppt/") {
                            target.to_string()
                        } else {
                            format!("ppt/{target}")
                        };
                        slides.push(part);
                    }
                }
                Event::Eof => break,
                _ => {}
            }
        }
        Ok((!slides.is_empty()).then_some(slides))
    })()?;

    if let Some(slides) = ordered {
        return Ok(slides);
    }
    let mut numbered: Vec<(u32, String)> = archive
        .file_names()
        .filter_map(|name| {
            let number = name
                .strip_prefix("ppt/slides/slide")?
                .strip_suffix(".xml")?
                .parse()
                .ok()?;
            Some((number, name.to_string()))
        })
        .collect();
    numbered.sort();
    Ok(numbered.into_iter().map(|(_, name)| name).collect())
}

/// The `r:id` attribute of a `p:sldId` element. Its local name is `id`, the same
/// as the numeric slide id, so the namespace prefix tells them apart.
fn relationship_id(start: &BytesStart<'_>) -> Option<String> {
    start.attributes().flatten().find_map(|attr| {
        let key = attr.key.as_ref();
        (key.ends_with(b":id") && key != b"id")
            .then(|| {
                attr.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                    .ok()
                    .map(Cow::into_owned)
            })
            .flatten()
    })
}

fn pptx_to_markdown(archive: &mut Archive<'_>) -> Result<Extracted, ContentProcessorError> {
    let mut parts = slide_parts(archive)?;
    let mut warnings = Vec::new();
    if parts.len() > MAX_SLIDES {
        parts.truncate(MAX_SLIDES);
        warnings.push("slides_truncated".to_string());
    }
    let mut sections = Vec::new();
    for (index, part) in parts.iter().enumerate() {
        let Some(xml) = read_part(archive, part)? else {
            continue;
        };
        let paragraphs = slide_paragraphs(&xml)?;
        let mut section = format!("## Slide {}", index + 1);
        let mut body = paragraphs.into_iter();
        if let Some(title) = body.next() {
            section = format!("{section}: {title}");
        }
        let rest: Vec<String> = body.collect();
        if !rest.is_empty() {
            section.push_str("\n\n");
            section.push_str(&rest.join("\n"));
        }
        sections.push(section);
    }
    Ok(Extracted {
        markdown: sections.join("\n\n"),
        title: core_title(archive),
        warnings,
    })
}

/// Non-empty `a:p` paragraphs of one slide, in document order.
fn slide_paragraphs(xml: &str) -> Result<Vec<String>, ContentProcessorError> {
    let mut reader = XmlReader::from_str(xml);
    let mut paragraphs = Vec::new();
    let mut current: Option<String> = None;
    let mut in_text = false;
    loop {
        match reader.read_event().map_err(xml_error)? {
            Event::Start(start) => match start.local_name().as_ref() {
                b"p" => current = Some(String::new()),
                b"t" => in_text = true,
                _ => {}
            },
            Event::Empty(start) if start.local_name().as_ref() == b"br" => {
                if let Some(text) = &mut current {
                    text.push(' ');
                }
            }
            Event::Text(text) if in_text => {
                if let Some(current) = &mut current {
                    current.push_str(&text.xml10_content().map_err(|e| xml_error(e.into()))?);
                }
            }
            Event::GeneralRef(reference) if in_text => {
                if let Some(current) = &mut current {
                    push_reference(current, &reference);
                }
            }
            Event::End(end) => match end.local_name().as_ref() {
                b"t" => in_text = false,
                b"p" => {
                    if let Some(text) = current.take() {
                        let text = text.trim();
                        if !text.is_empty() {
                            paragraphs.push(text.to_string());
                        }
                    }
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(paragraphs)
}

fn spreadsheet_to_markdown(body: &[u8]) -> Result<Extracted, ContentProcessorError> {
    let mut workbook = open_workbook_auto_from_rs(Cursor::new(body))
        .map_err(|error| ContentProcessorError(format!("cannot open spreadsheet: {error}")))?;
    let mut warnings = Vec::new();
    let mut sections = Vec::new();
    for name in workbook.sheet_names() {
        let range = match workbook.worksheet_range(&name) {
            Ok(range) => range,
            Err(error) => {
                warnings.push("sheet_unreadable".to_string());
                sections.push(format!("## {name}\n\n_Sheet could not be read: {error}_"));
                continue;
            }
        };
        let total_rows = range.height();
        let total_columns = range.width();
        let rows: Vec<Vec<String>> = range
            .rows()
            .take(MAX_SHEET_ROWS)
            .map(|row| row.iter().take(MAX_SHEET_COLUMNS).map(cell_text).collect())
            .collect();
        let Some(table) = markdown_table(&rows)? else {
            continue;
        };
        let mut section = format!("## {name}\n\n{table}");
        if total_rows > MAX_SHEET_ROWS || total_columns > MAX_SHEET_COLUMNS {
            push_unique(&mut warnings, "sheet_truncated");
            section.push_str(&format!(
                "\n\n_Showing {} of {total_rows} rows and {} of {total_columns} columns._",
                total_rows.min(MAX_SHEET_ROWS),
                total_columns.min(MAX_SHEET_COLUMNS)
            ));
        }
        sections.push(section);
    }
    Ok(Extracted {
        markdown: sections.join("\n\n"),
        title: None,
        warnings,
    })
}

fn push_unique(warnings: &mut Vec<String>, warning: &str) {
    if !warnings.iter().any(|existing| existing == warning) {
        warnings.push(warning.to_string());
    }
}

fn cell_text(cell: &Data) -> String {
    match cell {
        Data::Empty => String::new(),
        // Excel stores dates as day serials; show them as ISO 8601 rather than
        // numbers such as 41640. Durations keep their numeric form.
        Data::DateTime(value) if value.is_datetime() => {
            let (year, month, day, hour, minute, second, _) = value.to_ymd_hms_milli();
            if (hour, minute, second) == (0, 0, 0) {
                format!("{year:04}-{month:02}-{day:02}")
            } else {
                format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}")
            }
        }
        other => other.to_string(),
    }
}

/// Render rows as a Markdown table, the first row as the header. Rows are padded
/// to the widest row; fully empty rows and trailing empty columns are dropped.
fn markdown_table(rows: &[Vec<String>]) -> Result<Option<String>, ContentProcessorError> {
    let mut output = String::new();
    append_table(rows, &mut output, DOCX_LIMITS)?;
    Ok((!output.is_empty()).then_some(output))
}

fn limit_error() -> ContentProcessorError {
    ContentProcessorError("Office table or DOCX output exceeds the conversion limit".into())
}

fn checked_size(
    current: usize,
    extra: usize,
    limit: usize,
) -> Result<usize, ContentProcessorError> {
    current
        .checked_add(extra)
        .filter(|size| *size <= limit)
        .ok_or_else(limit_error)
}

fn check_table_shape(
    rows: usize,
    width: usize,
    limits: DocxLimits,
) -> Result<(), ContentProcessorError> {
    if rows > limits.rows
        || width > limits.columns
        || rows
            .checked_mul(width)
            .is_none_or(|cells| cells > limits.cells)
    {
        return Err(limit_error());
    }
    Ok(())
}

fn reserve_bounded(
    out: &mut String,
    extra: usize,
    limit: usize,
) -> Result<(), ContentProcessorError> {
    let size = checked_size(out.len(), extra, limit)?;
    if size > out.capacity() {
        let capacity = out.capacity().saturating_mul(2).max(size).min(limit);
        out.try_reserve_exact(capacity - out.len())
            .map_err(|_| limit_error())?;
    }
    Ok(())
}

fn append_bounded(out: &mut String, text: &str, limit: usize) -> Result<(), ContentProcessorError> {
    reserve_bounded(out, text.len(), limit)?;
    out.push_str(text);
    Ok(())
}

fn append_block(out: &mut String, block: &str, limit: usize) -> Result<(), ContentProcessorError> {
    let separator = if out.is_empty() { "" } else { "\n\n" };
    reserve_bounded(
        out,
        checked_size(block.len(), separator.len(), limit)?,
        limit,
    )?;
    out.push_str(separator);
    out.push_str(block);
    Ok(())
}

fn nonempty_row(row: &&Vec<String>) -> bool {
    row.iter().any(|cell| !cell.trim().is_empty())
}

fn append_table(
    rows: &[Vec<String>],
    out: &mut String,
    limits: DocxLimits,
) -> Result<(), ContentProcessorError> {
    let count = rows.iter().filter(nonempty_row).count();
    let width = rows
        .iter()
        .map(|row| {
            row.iter()
                .rposition(|cell| !cell.trim().is_empty())
                .map_or(0, |last| last + 1)
        })
        .max()
        .unwrap_or(0);
    check_table_shape(rows.len(), width, limits)?;
    if width == 0 {
        return Ok(());
    }
    let mut bytes = if out.is_empty() { 0 } else { 2 };
    // Admission precedes all escaping, padding and allocation. Include every
    // delimiter, newline and UTF-8 byte in the single output buffer.
    for row in rows.iter().filter(nonempty_row) {
        bytes = checked_size(
            bytes,
            width
                .checked_mul(3)
                .and_then(|n| n.checked_add(1))
                .ok_or_else(limit_error)?,
            limits.output_bytes,
        )?;
        for cell in row.iter().take(width) {
            visit_escaped_cell(cell, |piece| {
                bytes = checked_size(bytes, piece.len(), limits.output_bytes)?;
                Ok(())
            })?;
        }
    }
    bytes = checked_size(
        bytes,
        width
            .checked_mul(6)
            .and_then(|n| n.checked_add(1))
            .ok_or_else(limit_error)?,
        limits.output_bytes,
    )?;
    bytes = checked_size(bytes, count, limits.output_bytes)?;
    reserve_bounded(out, bytes, limits.output_bytes)?;
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    for (index, row) in rows.iter().filter(nonempty_row).enumerate() {
        if index > 0 {
            out.push('\n');
        }
        out.push_str("| ");
        for column in 0..width {
            if column > 0 {
                out.push_str(" | ");
            }
            visit_escaped_cell(row.get(column).map(String::as_str).unwrap_or(""), |piece| {
                out.push_str(piece);
                Ok(())
            })?;
        }
        out.push_str(" |");
        if index == 0 {
            out.push_str("\n|");
            for _ in 0..width {
                out.push_str(" --- |");
            }
        }
    }
    Ok(())
}

fn visit_escaped_cell(
    cell: &str,
    mut visit: impl FnMut(&str) -> Result<(), ContentProcessorError>,
) -> Result<(), ContentProcessorError> {
    let mut chars = cell.trim().chars().peekable();
    while let Some(ch) = chars.next() {
        let mut utf8 = [0; 4];
        let piece = match ch {
            '\\' => "\\\\",
            '|' => "\\|",
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                "<br>"
            }
            '\n' => "<br>",
            other => other.encode_utf8(&mut utf8),
        };
        visit(piece)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use bytes::Bytes;

    use super::*;
    use crate::content::{
        ContentFocus, ContentOutputFormat, ContentProcessor, ContentProcessorInput,
        ProcessedContent,
    };
    use crate::documents::{DocumentConversion, DocumentProcessor};
    use url::Url;

    fn processor() -> DocumentProcessor {
        DocumentProcessor::new(DocumentConversion::empty().converter(OfficeConverter))
    }

    const DOCX: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
    const PPTX: &str = "application/vnd.openxmlformats-officedocument.presentationml.presentation";

    fn zip_of(parts: &[(&str, &str)]) -> Bytes {
        let mut buffer = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut buffer);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            for (name, content) in parts {
                writer.start_file(*name, options).unwrap();
                writer.write_all(content.as_bytes()).unwrap();
            }
            writer.finish().unwrap();
        }
        Bytes::from(buffer.into_inner())
    }

    async fn process(url: &str, content_type: &str, body: Bytes) -> ProcessedContent {
        processor()
            .process(ContentProcessorInput {
                url: Url::parse(url).unwrap(),
                content_type: Some(content_type.to_string()),
                body,
                output_format: ContentOutputFormat::Markdown,
                content_focus: ContentFocus::Full,
            })
            .await
            .unwrap()
    }

    const DOCUMENT_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Quarterly report</w:t></w:r></w:p>
    <w:p><w:r><w:t xml:space="preserve">Revenue grew </w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>12%</w:t></w:r><w:r><w:t> &amp; costs fell.</w:t></w:r></w:p>
    <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>First point</w:t></w:r></w:p>
    <w:p><w:pPr><w:numPr><w:ilvl w:val="1"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>Nested point</w:t></w:r></w:p>
    <w:tbl>
      <w:tr><w:tc><w:p><w:r><w:t>Region</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Sales</w:t></w:r></w:p></w:tc></w:tr>
      <w:tr><w:tc><w:p><w:r><w:t>EU|West</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>42</w:t></w:r></w:p></w:tc></w:tr>
    </w:tbl>
    <w:p/>
  </w:body>
</w:document>"#;

    const CORE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Q3 &amp; outlook</dc:title></cp:coreProperties>"#;

    fn small_table() -> &'static str {
        r#"<w:document xmlns:w="w"><w:body><w:tbl>
<w:tr><w:tc><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>C</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:tc><w:p><w:r><w:t>D</w:t></w:r></w:p></w:tc></w:tr>
</w:tbl></w:body></w:document>"#
    }

    #[test]
    fn docx_table_limits_admit_before_rendering() {
        for limits in [
            DocxLimits {
                rows: 1,
                ..DOCX_LIMITS
            },
            DocxLimits {
                columns: 2,
                ..DOCX_LIMITS
            },
            DocxLimits {
                cells: 5,
                ..DOCX_LIMITS
            },
            DocxLimits {
                output_bytes: 32,
                ..DOCX_LIMITS
            },
        ] {
            let error = docx_to_markdown_with_limits(small_table(), limits).unwrap_err();
            assert!(error.0.contains("limit"), "{error}");
        }
    }

    #[test]
    fn docx_table_limits_preserve_ordinary_padding() {
        let expected = "| A | B | C |\n| --- | --- | --- |\n| D |  |  |";
        let limits = DocxLimits {
            rows: 2,
            columns: 3,
            cells: 6,
            output_bytes: expected.len(),
        };
        assert_eq!(
            docx_to_markdown_with_limits(small_table(), limits).unwrap(),
            expected
        );
    }

    #[test]
    fn docx_table_bytes_include_escaping_and_document_blocks() {
        let rows = vec![vec![" é|\\\r\nZ\nY\rQ ".into(), "".into()], vec!["".into()]];
        let expected = "intro\n\n| é\\|\\\\<br>Z<br>Y<br>Q |\n| --- |";
        let limits = DocxLimits {
            output_bytes: expected.len(),
            ..DOCX_LIMITS
        };
        let mut output = "intro".to_string();
        append_table(&rows, &mut output, limits).unwrap();
        assert_eq!(output, expected);
        let before = output.clone();
        assert!(append_table(&rows, &mut output, limits).is_err());
        assert_eq!(output, before);
        let mut short = "intro".to_string();
        assert!(append_table(
            &rows,
            &mut short,
            DocxLimits {
                output_bytes: expected.len() - 1,
                ..limits
            }
        )
        .is_err());
        assert_eq!(short, "intro");
    }

    #[tokio::test]
    async fn docx_becomes_markdown_with_headings_lists_and_tables() {
        let body = zip_of(&[
            ("word/document.xml", DOCUMENT_XML),
            ("docProps/core.xml", CORE_XML),
        ]);
        let processed = process("https://example.com/report", DOCX, body).await;

        assert_eq!(processed.format.as_deref(), Some("markdown"));
        assert_eq!(
            processed.content.as_deref(),
            Some(
                "# Quarterly report\n\nRevenue grew 12% & costs fell.\n\n- First point\n\n  - Nested point\n\n| Region | Sales |\n| --- | --- |\n| EU\\|West | 42 |"
            )
        );
        let metadata = processed.metadata.unwrap();
        assert_eq!(metadata.title.as_deref(), Some("Q3 & outlook"));
        assert_eq!(metadata.extraction_method.as_deref(), Some("office_docx"));
        assert!(processed.error.is_none());
    }

    #[tokio::test]
    async fn pptx_slides_follow_presentation_order() {
        let slide = |title: &str, body: &str| {
            format!(
                r#"<p:sld xmlns:p="p" xmlns:a="a"><p:cSld><p:spTree>
<p:sp><p:txBody><a:p><a:r><a:t>{title}</a:t></a:r></a:p></p:txBody></p:sp>
<p:sp><p:txBody><a:p><a:r><a:t>{body}</a:t></a:r></a:p><a:p/></p:txBody></p:sp>
</p:spTree></p:cSld></p:sld>"#
            )
        };
        let first = slide("Intro", "Why we are here");
        let second = slide("Plan", "Ship it");
        let body = zip_of(&[
            (
                "ppt/presentation.xml",
                r#"<p:presentation xmlns:p="p" xmlns:r="r"><p:sldIdLst><p:sldId id="256" r:id="rId3"/><p:sldId id="257" r:id="rId2"/></p:sldIdLst></p:presentation>"#,
            ),
            (
                "ppt/_rels/presentation.xml.rels",
                r#"<Relationships><Relationship Id="rId2" Target="slides/slide1.xml"/><Relationship Id="rId3" Target="slides/slide2.xml"/></Relationships>"#,
            ),
            ("ppt/slides/slide1.xml", &second),
            ("ppt/slides/slide2.xml", &first),
        ]);
        let processed = process("https://example.com/deck.pptx", PPTX, body).await;

        assert_eq!(
            processed.content.as_deref(),
            Some("## Slide 1: Intro\n\nWhy we are here\n\n## Slide 2: Plan\n\nShip it")
        );
    }

    #[test]
    fn matches_media_types_and_generic_downloads_by_extension() {
        let processor = processor();
        let page = Url::parse("https://example.com/download").unwrap();
        let xlsx = Url::parse("https://example.com/data.XLSX").unwrap();
        assert!(processor.matches(&page, Some(DOCX)));
        assert!(processor.matches(&page, Some("application/vnd.ms-excel")));
        assert!(processor.matches(
            &page,
            Some("application/vnd.oasis.opendocument.spreadsheet")
        ));
        assert!(processor.matches(&xlsx, Some("application/octet-stream")));
        assert!(processor.matches(&xlsx, None));
        assert!(!processor.matches(&xlsx, Some("text/html")));
        assert!(!processor.matches(&page, Some("application/octet-stream")));
        assert!(!processor.matches(&page, Some("application/vnd.ms-powerpoint")));
    }

    #[tokio::test]
    async fn spreadsheet_from_ods_becomes_tables() {
        let content = r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" office:version="1.2">
<office:body><office:spreadsheet>
<table:table table:name="Prices">
<table:table-row><table:table-cell office:value-type="string"><text:p>Item</text:p></table:table-cell><table:table-cell office:value-type="string"><text:p>Price</text:p></table:table-cell></table:table-row>
<table:table-row><table:table-cell office:value-type="string"><text:p>Tea</text:p></table:table-cell><table:table-cell office:value-type="float" office:value="3.5"><text:p>3.5</text:p></table:table-cell></table:table-row>
</table:table>
</office:spreadsheet></office:body></office:document-content>"#;
        let body = zip_of(&[
            ("mimetype", "application/vnd.oasis.opendocument.spreadsheet"),
            (
                "META-INF/manifest.xml",
                r#"<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"><manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/></manifest:manifest>"#,
            ),
            ("content.xml", content),
        ]);
        let processed = process(
            "https://example.com/prices",
            "application/vnd.oasis.opendocument.spreadsheet",
            body,
        )
        .await;

        assert_eq!(
            processed.content.as_deref(),
            Some("## Prices\n\n| Item | Price |\n| --- | --- |\n| Tea | 3.5 |")
        );
        assert_eq!(
            processed.metadata.unwrap().extraction_method.as_deref(),
            Some("office_spreadsheet")
        );
    }

    #[test]
    fn archive_budget_rejects_zip_bombs() {
        // 4 MB of zeros deflates to a few KB; a budget of 1 MB must reject it from
        // the central directory alone, before inflating anything.
        let zeros = "\0".repeat(4 * 1024 * 1024);
        let body = zip_of(&[("word/document.xml", &zeros)]);
        assert!(body.len() < 64 * 1024);

        let error = open_archive_within(&body, 1024 * 1024).unwrap_err();
        assert!(error.0.contains("over the 1048576 byte limit"), "{error}");
        assert!(open_archive_within(&body, 8 * 1024 * 1024).is_ok());
    }

    #[tokio::test]
    async fn xlsx_becomes_tables() {
        let body = zip_of(&[
            (
                "[Content_Types].xml",
                r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#,
            ),
            (
                "_rels/.rels",
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
            ),
            (
                "xl/workbook.xml",
                r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Budget" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
            ),
            (
                "xl/_rels/workbook.xml.rels",
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
            ),
            (
                "xl/worksheets/sheet1.xml",
                r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Team</t></is></c><c r="B1" t="inlineStr"><is><t>Spend</t></is></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>Infra</t></is></c><c r="B2"><v>1200</v></c></row></sheetData></worksheet>"#,
            ),
        ]);
        let processed = process(
            "https://example.com/budget.xlsx",
            "application/octet-stream",
            body,
        )
        .await;

        assert_eq!(
            processed.content.as_deref(),
            Some("## Budget\n\n| Team | Spend |\n| --- | --- |\n| Infra | 1200 |")
        );
    }

    #[test]
    fn corrupt_document_is_an_error_not_garbage() {
        let error = extract(OfficeKind::Docx, b"not a zip").unwrap_err();
        assert!(error.0.contains("invalid Office archive"), "{error}");
    }

    #[test]
    fn spreadsheet_dates_render_as_iso() {
        use calamine::{ExcelDateTime, ExcelDateTimeType};

        let date = Data::DateTime(ExcelDateTime::new(
            41640.0,
            ExcelDateTimeType::DateTime,
            false,
        ));
        let stamp = Data::DateTime(ExcelDateTime::new(
            45943.5,
            ExcelDateTimeType::DateTime,
            false,
        ));
        assert_eq!(cell_text(&date), "2014-01-01");
        assert_eq!(cell_text(&stamp), "2025-10-13 12:00:00");
        assert_eq!(cell_text(&Data::Float(3.5)), "3.5");
    }

    #[test]
    fn heading_levels_from_style_ids() {
        assert_eq!(heading_level("Heading1"), Some(1));
        assert_eq!(heading_level("heading 3"), Some(3));
        assert_eq!(heading_level("Title"), Some(1));
        assert_eq!(heading_level("Heading9"), None);
        assert_eq!(heading_level("Normal"), None);
    }
}
