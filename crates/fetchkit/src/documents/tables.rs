// Decisions:
// - THREAT[TM-DOS-008]: DOCX dimensions and padded cell work need admission
//   before either local backend renders. markkit's sheet limits exclude DOCX.
// - Use the legacy sheet dimensions for DOCX. This preflight bounds markkit's
//   table expansion; the legacy renderer additionally admits exact output bytes.
#[cfg(feature = "docs-markkit")]
use crate::content::ContentProcessorError;

#[derive(Clone, Copy)]
pub(super) struct TableLimits {
    pub rows: usize,
    pub columns: usize,
    pub cells: usize,
    pub output_bytes: usize,
}

pub(super) const TABLE_LIMITS: TableLimits = TableLimits {
    rows: 1_000,
    columns: 50,
    cells: 50_000,
    output_bytes: 10 * 1024 * 1024,
};

#[cfg(feature = "docs-markkit")]
const MAX_TABLE_DEPTH: usize = 50;

#[cfg(feature = "docs-markkit")]
pub(super) fn check_xml(xml: &[u8], limits: TableLimits) -> Result<(), ContentProcessorError> {
    use quick_xml::events::Event;
    let xml = xml.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(xml);
    let fail = || ContentProcessorError("DOCX table exceeds the conversion limit".into());
    let mut reader = quick_xml::Reader::from_reader(xml);
    let mut tables: Vec<(usize, usize, usize)> = Vec::new();
    let mut source_cells = 0usize;
    let mut padded_cells = 0usize;
    let mut padding_bytes = 0usize;
    loop {
        let event = reader.read_event().map_err(|_| fail())?;
        let empty = matches!(&event, Event::Empty(_));
        match event {
            Event::Start(start) | Event::Empty(start) => {
                let name = start.local_name();
                if name.as_ref() == b"tbl" {
                    if tables.len() >= MAX_TABLE_DEPTH {
                        return Err(fail());
                    }
                    tables.push((0, 0, 0));
                }
                if let Some((rows, width, columns)) = tables.last_mut() {
                    match name.as_ref() {
                        b"tr" => {
                            *rows = rows.checked_add(1).ok_or_else(fail)?;
                            *columns = 0;
                        }
                        b"tc" => {
                            source_cells = source_cells
                                .checked_add(1)
                                .filter(|n| *n <= limits.cells)
                                .ok_or_else(fail)?;
                            *columns = columns.checked_add(1).ok_or_else(fail)?;
                        }
                        b"gridSpan" => {
                            let span = start
                                .attributes()
                                .flatten()
                                .find(|a| a.key.local_name().as_ref() == b"val")
                                .and_then(|a| {
                                    a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                        .ok()?
                                        .parse::<usize>()
                                        .ok()
                                })
                                .unwrap_or(1)
                                .min(64);
                            *columns = columns
                                .checked_add(span.saturating_sub(1))
                                .ok_or_else(fail)?;
                        }
                        _ => {}
                    }
                    *width = (*width).max(*columns);
                    if *rows > limits.rows
                        || *width > limits.columns
                        || rows.checked_mul(*width).is_none_or(|n| n > limits.cells)
                    {
                        return Err(fail());
                    }
                }
                if empty && name.as_ref() == b"tbl" {
                    tables.pop();
                }
            }
            Event::End(end) if end.local_name().as_ref() == b"tbl" => {
                if let Some((rows, width, _)) = tables.pop() {
                    let cells = rows.checked_mul(width).ok_or_else(fail)?;
                    padded_cells = padded_cells
                        .checked_add(cells)
                        .filter(|n| *n <= limits.cells)
                        .ok_or_else(fail)?;
                    let bytes = cells
                        .checked_mul(3)
                        .and_then(|n| width.checked_mul(6)?.checked_add(n))
                        .and_then(|n| rows.checked_mul(2)?.checked_add(n))
                        .and_then(|n| n.checked_add(4))
                        .ok_or_else(fail)?;
                    padding_bytes = padding_bytes
                        .checked_add(bytes)
                        .filter(|n| *n <= limits.output_bytes)
                        .ok_or_else(fail)?;
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(())
}

#[cfg(feature = "docs-markkit")]
pub(super) fn check_archive(
    body: &[u8],
    config: &markkit::MarkkitBuilder,
) -> Result<(), super::ConvertError> {
    use std::io::{Cursor, Read};
    let fail = || {
        super::ConvertError::Failed(
            "DOCX table admission failed: archive or conversion limits".into(),
        )
    };
    let mut archive = zip::ZipArchive::new(Cursor::new(body)).map_err(|_| fail())?;
    if archive.len() > config.max_archive_entries.min(10_000) {
        return Err(fail());
    }
    let mut total = 0u64;
    let part_limit = config.max_archive_entry_size.min(64 * 1024 * 1024);
    let total_limit = config.max_archive_total_size.min(256 * 1024 * 1024);
    for index in 0..archive.len() {
        let entry = archive.by_index_raw(index).map_err(|_| fail())?;
        total = total
            .checked_add(entry.size())
            .filter(|n| *n <= total_limit)
            .ok_or_else(fail)?;
        if entry.size() > part_limit {
            return Err(fail());
        }
    }
    let mut inflated = 0u64;
    let mut read = |name: &str| -> Result<Vec<u8>, super::ConvertError> {
        let entry = match archive.by_name(name) {
            Ok(entry) => entry,
            Err(zip::result::ZipError::FileNotFound) => return Ok(Vec::new()),
            Err(_) => return Err(fail()),
        };
        let mut bytes = Vec::new();
        entry
            .take(part_limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| fail())?;
        if bytes.len() as u64 > part_limit {
            return Err(fail());
        }
        inflated = inflated
            .checked_add(bytes.len() as u64)
            .filter(|n| *n <= total_limit)
            .ok_or_else(fail)?;
        Ok(bytes)
    };
    // markkit may choose any internal officeDocument relationship, not just the
    // conventional filename. Admit every candidate before it makes that choice.
    let rels = read("_rels/.rels")?;
    let mut reader = quick_xml::Reader::from_reader(rels.as_slice());
    let mut paths = std::collections::BTreeSet::new();
    loop {
        use quick_xml::events::Event;
        match reader.read_event().map_err(|_| fail())? {
            Event::Start(start) | Event::Empty(start)
                if start.local_name().as_ref() == b"Relationship" =>
            {
                let attr = |name: &[u8]| {
                    start
                        .attributes()
                        .flatten()
                        .find(|a| a.key.local_name().as_ref() == name)
                        .and_then(|a| {
                            a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                .ok()
                                .map(|v| v.into_owned())
                        })
                };
                if attr(b"Type").is_some_and(|t| t.ends_with("/officeDocument"))
                    && !attr(b"TargetMode").is_some_and(|m| m.eq_ignore_ascii_case("External"))
                {
                    if let Some(target) = attr(b"Target") {
                        let path = if let Some(path) = target.strip_prefix('/') {
                            path.to_string()
                        } else {
                            let mut parts = Vec::new();
                            for part in target.split('/') {
                                match part {
                                    "" | "." => {}
                                    ".." => {
                                        parts.pop();
                                    }
                                    part => parts.push(part),
                                }
                            }
                            parts.join("/")
                        };
                        paths.insert(path);
                        if paths.len() > config.max_archive_entries.min(10_000) {
                            return Err(fail());
                        }
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    paths.insert("word/document.xml".to_string());
    for path in paths {
        check_xml(&read(&path)?, TABLE_LIMITS).map_err(|e| super::ConvertError::Failed(e.0))?;
    }
    Ok(())
}

#[cfg(all(test, feature = "docs-markkit"))]
mod tests {
    use super::*;

    const TABLE: &[u8] = b"<document><tbl><tr><tc/></tr><tr><tc/><tc/></tr></tbl></document>";

    #[test]
    fn docx_preflight_admits_small_table_dimensions() {
        for limits in [
            TableLimits {
                rows: 1,
                ..TABLE_LIMITS
            },
            TableLimits {
                columns: 1,
                ..TABLE_LIMITS
            },
            TableLimits {
                cells: 3,
                ..TABLE_LIMITS
            },
            TableLimits {
                output_bytes: 8,
                ..TABLE_LIMITS
            },
        ] {
            assert!(check_xml(TABLE, limits).is_err());
        }
        assert!(check_xml(TABLE, TABLE_LIMITS).is_ok());
        assert!(check_xml(
            b"prefix<document><tbl><tr><tc/></tr><tr><tc/></tr></tbl></document>",
            TableLimits {
                rows: 1,
                ..TABLE_LIMITS
            }
        )
        .is_err());
    }

    #[test]
    fn docx_preflight_counts_spans_nested_and_multiple_tables() {
        let limits = TableLimits {
            rows: 2,
            columns: 3,
            cells: 4,
            ..TABLE_LIMITS
        };
        assert!(check_xml(
            b"<tbl><tr><tc><tcPr><gridSpan val=\"&#51;\"/></tcPr></tc></tr><tr><tc/></tr></tbl>",
            limits
        )
        .is_err());
        assert!(check_xml(
            b"<tbl><tr><tc><tbl><tr><tc/><tc/></tr></tbl></tc></tr></tbl>",
            limits
        )
        .is_ok());
        assert!(check_xml(
            b"<doc><tbl/><tbl/><tbl/></doc>",
            TableLimits {
                columns: 1,
                ..limits
            }
        )
        .is_ok());
        assert!(check_xml(
            b"<doc><tbl><tr><tc/><tc/><tc/></tr></tbl><tbl><tr><tc/><tc/></tr></tbl></doc>",
            limits
        )
        .is_err());
    }
}
