//! Pulling readable text out of the office formats, as markdown.
//!
//! A `.docx` is a zip of XML. So is a `.odt`, a `.xlsx` and a `.pptx`. The
//! pane already renders markdown safely, so the cheapest way to show one of
//! these is to turn it into markdown here and hand it to the renderer that
//! already exists. Nothing in this module produces HTML.
//!
//! The fidelity is deliberately low. Headings, paragraphs, lists and tables
//! come across. Images, fonts, colours, page layout, footnotes, comments and
//! tracked changes do not. That is the agreed scope: this is for reading what
//! a run produced, not for rendering a document faithfully.
//!
//! Everything here treats its input as hostile. A model wrote the file, or
//! downloaded it, so an archive that claims to hold a terabyte of XML is a
//! case to refuse rather than a case to allocate for.

use std::io::Read;
use std::path::Path;

use quick_xml::events::{BytesStart, Event as XmlEvent};
use quick_xml::Reader;

/// Which office format a file is. Chosen from the extension, like every other
/// type decision the artifact endpoints make.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Kind {
    Docx,
    Odt,
    Xlsx,
    Pptx,
}

impl Kind {
    pub fn for_path(path: &Path) -> Option<Kind> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "docx" => Some(Kind::Docx),
            "odt" => Some(Kind::Odt),
            "xlsx" => Some(Kind::Xlsx),
            "pptx" => Some(Kind::Pptx),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Kind::Docx => "Word document",
            Kind::Odt => "OpenDocument text",
            Kind::Xlsx => "spreadsheet",
            Kind::Pptx => "slide deck",
        }
    }
}

/// Why a document could not be turned into markdown. Every variant is
/// something the pane can say out loud, because a pane showing nothing and a
/// pane showing an empty document look identical and are not the same.
#[derive(Debug, Eq, PartialEq)]
pub enum Error {
    /// The bytes are not a zip archive, so they are not this format either.
    NotAnArchive(String),
    /// A zip archive, but without the part that carries the content.
    MissingPart(&'static str),
    /// The archive holds more entries than any real document does.
    TooManyEntries(usize),
    /// A part decompresses past the cap. The classic zip bomb: a few hundred
    /// bytes on disk that become gigabytes in memory.
    PartTooLarge,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotAnArchive(why) => {
                write!(f, "this file is not a readable office document: {why}")
            }
            Error::MissingPart(part) => {
                write!(f, "this archive has no {part}, so there is nothing to read")
            }
            Error::TooManyEntries(count) => write!(
                f,
                "this archive holds {count} entries, far more than a document has"
            ),
            Error::PartTooLarge => write!(
                f,
                "part of this document decompresses to more than this server will read"
            ),
        }
    }
}

impl std::error::Error for Error {}

/// How much of an untrusted archive this module will touch.
#[derive(Debug, Clone, Copy)]
struct Limits {
    /// Entries in the archive. A `.docx` with styles, fonts and images is a
    /// few dozen; a `.xlsx` with many sheets a few hundred.
    entries: usize,
    /// Decompressed bytes from any one part.
    part_bytes: u64,
    /// Decompressed bytes across every part read from one archive, which is
    /// the cap that matters for a spreadsheet with many sheets.
    total_bytes: u64,
    /// Rows and columns rendered per sheet. Past this the markdown is longer
    /// than anyone reads and slower than the pane can lay out.
    sheet_rows: usize,
    sheet_cols: usize,
    /// Slides rendered from one deck.
    slides: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            entries: 4096,
            part_bytes: 16 * 1024 * 1024,
            total_bytes: 48 * 1024 * 1024,
            sheet_rows: 500,
            sheet_cols: 64,
            slides: 300,
        }
    }
}

/// Turn one office file into markdown.
pub fn to_markdown(kind: Kind, bytes: &[u8]) -> Result<String, Error> {
    to_markdown_limited(kind, bytes, Limits::default())
}

fn to_markdown_limited(kind: Kind, bytes: &[u8], limits: Limits) -> Result<String, Error> {
    let mut archive = Archive::open(bytes, limits)?;
    let text = match kind {
        Kind::Docx => docx(&mut archive)?,
        Kind::Odt => odt(&mut archive)?,
        Kind::Xlsx => xlsx(&mut archive)?,
        Kind::Pptx => pptx(&mut archive)?,
    };
    if text.trim().is_empty() {
        return Ok(format!(
            "*This {} has no text this reader can extract.*",
            kind.label()
        ));
    }
    Ok(text)
}

/* ------------------------------------------------------------------ */
/* the archive                                                         */
/* ------------------------------------------------------------------ */

/// A zip archive being read under a budget.
///
/// Entry names are never joined onto a filesystem path, so a name containing
/// `..` is not a traversal here: parts are looked up by exact name or matched
/// against a prefix, and the bytes only ever go into a String.
struct Archive<'a> {
    zip: zip::ZipArchive<std::io::Cursor<&'a [u8]>>,
    limits: Limits,
    spent: u64,
}

impl<'a> Archive<'a> {
    fn open(bytes: &'a [u8], limits: Limits) -> Result<Archive<'a>, Error> {
        let zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))
            .map_err(|e| Error::NotAnArchive(e.to_string()))?;
        if zip.len() > limits.entries {
            return Err(Error::TooManyEntries(zip.len()));
        }
        Ok(Archive {
            zip,
            limits,
            spent: 0,
        })
    }

    fn names(&self) -> Vec<String> {
        self.zip.file_names().map(str::to_string).collect()
    }

    /// Read one part as text, or `None` when the archive has no such part.
    ///
    /// The cap is enforced on what comes out of the decompressor, not on what
    /// the archive claims the size is. A zip bomb's headers are as much a lie
    /// as its contents.
    fn part(&mut self, name: &str) -> Result<Option<String>, Error> {
        let room = self
            .limits
            .total_bytes
            .saturating_sub(self.spent)
            .min(self.limits.part_bytes);
        let Ok(entry) = self.zip.by_name(name) else {
            return Ok(None);
        };
        let mut out = Vec::new();
        // One byte past the budget, so "filled the budget exactly" and "still
        // had more to give" are distinguishable.
        if entry.take(room + 1).read_to_end(&mut out).is_err() {
            return Err(Error::NotAnArchive(format!("{name} could not be read")));
        }
        if out.len() as u64 > room {
            return Err(Error::PartTooLarge);
        }
        self.spent += out.len() as u64;
        Ok(Some(String::from_utf8_lossy(&out).into_owned()))
    }

    fn required(&mut self, name: &'static str) -> Result<String, Error> {
        self.part(name)?.ok_or(Error::MissingPart(name))
    }
}

/* ------------------------------------------------------------------ */
/* xml plumbing                                                        */
/* ------------------------------------------------------------------ */

/// The element name without its namespace prefix.
///
/// Matching on `w:p` would work for every producer anyone has ever shipped,
/// and would still be wrong: nothing in XML makes a prefix stable, only the
/// namespace is. The local name is the closer approximation and it costs one
/// byte scan.
fn local(name: &[u8]) -> &[u8] {
    match name.iter().rposition(|b| *b == b':') {
        Some(colon) => &name[colon + 1..],
        None => name,
    }
}

fn is(start: &BytesStart<'_>, want: &str) -> bool {
    local(start.name().as_ref()) == want.as_bytes()
}

fn attribute(start: &BytesStart<'_>, want: &str) -> Option<String> {
    for attr in start.attributes().flatten() {
        if local(attr.key.as_ref()) == want.as_bytes() {
            return Some(String::from_utf8_lossy(&attr.value).into_owned());
        }
    }
    None
}

/// Resolve the entity references XML defines, and nothing else.
///
/// The five predefined entities and numeric character references, full stop.
/// A document that declares its own entities in a DTD gets them rendered as
/// the literal `&name;` rather than expanded, which is exactly the behaviour
/// that makes the billion laughs attack a non-event here.
fn resolve_entity(reference: &str) -> String {
    let body = reference.trim_start_matches('&').trim_end_matches(';');
    match body {
        "amp" => return "&".into(),
        "lt" => return "<".into(),
        "gt" => return ">".into(),
        "quot" => return "\"".into(),
        "apos" => return "'".into(),
        _ => {}
    }
    if let Some(digits) = body.strip_prefix('#') {
        let code = match digits
            .strip_prefix('x')
            .or_else(|| digits.strip_prefix('X'))
        {
            Some(hex) => u32::from_str_radix(hex, 16).ok(),
            None => digits.parse::<u32>().ok(),
        };
        if let Some(ch) = code.and_then(char::from_u32) {
            return ch.to_string();
        }
    }
    format!("&{body};")
}

fn reader(xml: &str) -> Reader<&[u8]> {
    let mut reader = Reader::from_str(xml);
    // An empty element must arrive as Empty, not as a Start/End pair, because
    // markers like `<w:tab/>` and `<w:numPr/>` are read off exactly that.
    reader.config_mut().expand_empty_elements = false;
    // A document from the wild is not guaranteed to be well formed, and a
    // half readable document beats a refusal.
    reader.config_mut().check_end_names = false;
    reader
}

/* ------------------------------------------------------------------ */
/* markdown assembly                                                   */
/* ------------------------------------------------------------------ */

/// Blocks joined by blank lines. Consecutive list items separated by a blank
/// line still render as one list, so this needs no special case for them.
fn join(blocks: Vec<String>) -> String {
    blocks
        .into_iter()
        .filter(|b| !b.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Collapse a cell's whitespace and escape the one character that would break
/// the row it lands in.
fn cell_text(raw: &str) -> String {
    raw.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('|', "\\|")
}

/// A markdown table, header row first, from rows that may be ragged.
fn markdown_table(rows: &[Vec<String>]) -> String {
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    for (index, row) in rows.iter().enumerate() {
        out.push('|');
        for column in 0..width {
            let cell = row.get(column).map(String::as_str).unwrap_or("");
            out.push(' ');
            out.push_str(if cell.is_empty() { " " } else { cell });
            out.push_str(" |");
        }
        out.push('\n');
        // The separator is what makes it a table at all to the renderer, so it
        // goes in whether or not the document had a header row of its own.
        if index == 0 {
            out.push('|');
            for _ in 0..width {
                out.push_str(" --- |");
            }
            out.push('\n');
        }
    }
    out.trim_end().to_string()
}

/* ------------------------------------------------------------------ */
/* docx                                                                */
/* ------------------------------------------------------------------ */

/// Word paragraph styles that mean a heading, normalized so `Heading 1`,
/// `heading1` and `Heading1` all land in the same place.
fn heading_level(style: &str) -> Option<usize> {
    let normalized: String = style
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    let digits = normalized.strip_prefix("heading")?;
    match digits.parse::<usize>() {
        Ok(level @ 1..=6) => Some(level),
        _ => None,
    }
}

#[derive(Default)]
struct Table {
    rows: Vec<Vec<String>>,
}

fn docx(archive: &mut Archive<'_>) -> Result<String, Error> {
    let xml = archive.required("word/document.xml")?;
    Ok(docx_body(&xml))
}

fn docx_body(xml: &str) -> String {
    let mut reader = reader(xml);
    let mut blocks: Vec<String> = Vec::new();
    let mut tables: Vec<Table> = Vec::new();
    let mut cell: Option<String> = None;
    let mut text = String::new();
    let mut style: Option<String> = None;
    let mut listed = false;
    let mut list_level = 0usize;
    let mut in_text = false;
    let mut in_body = false;
    // `mc:Fallback` repeats whatever `mc:Choice` already said, so reading both
    // prints every such paragraph twice.
    let mut skipping: Option<usize> = None;
    let mut depth = 0usize;

    // A document from the wild may not parse to the end. Stopping at the
    // first error keeps whatever was readable rather than throwing it away.
    while let Ok(event) = reader.read_event() {
        match event {
            XmlEvent::Start(e) => {
                depth += 1;
                if skipping.is_some() {
                    continue;
                }
                if is(&e, "Fallback") || is(&e, "instrText") {
                    skipping = Some(depth);
                    continue;
                }
                if is(&e, "body") {
                    in_body = true;
                    continue;
                }
                if !in_body {
                    continue;
                }
                match local(e.name().as_ref()) {
                    b"p" => {
                        text.clear();
                        style = None;
                        listed = false;
                        list_level = 0;
                    }
                    b"t" => in_text = true,
                    b"tbl" => tables.push(Table::default()),
                    b"tr" => {
                        if let Some(table) = tables.last_mut() {
                            table.rows.push(Vec::new());
                        }
                    }
                    b"tc" => cell = Some(String::new()),
                    b"numPr" => listed = true,
                    _ => {}
                }
            }
            XmlEvent::Empty(e) => {
                if skipping.is_some() || !in_body {
                    continue;
                }
                match local(e.name().as_ref()) {
                    b"pStyle" => style = attribute(&e, "val"),
                    b"numPr" => listed = true,
                    b"ilvl" => {
                        list_level = attribute(&e, "val")
                            .and_then(|v| v.parse::<usize>().ok())
                            .unwrap_or(0)
                    }
                    // A break or a tab inside a run is a space here. Keeping
                    // the line break would start a new markdown block in the
                    // middle of a sentence.
                    b"tab" | b"br" | b"cr" => text.push(' '),
                    _ => {}
                }
            }
            XmlEvent::Text(e) if in_text && skipping.is_none() => {
                if let Ok(decoded) = e.decode() {
                    text.push_str(&decoded);
                }
            }
            XmlEvent::GeneralRef(e) if in_text && skipping.is_none() => {
                if let Ok(name) = e.decode() {
                    text.push_str(&resolve_entity(&name));
                }
            }
            XmlEvent::End(e) => {
                if let Some(at) = skipping {
                    if depth <= at {
                        skipping = None;
                    }
                    depth = depth.saturating_sub(1);
                    continue;
                }
                depth = depth.saturating_sub(1);
                if !in_body {
                    continue;
                }
                match local(e.name().as_ref()) {
                    b"t" => in_text = false,
                    b"p" => {
                        let paragraph = std::mem::take(&mut text);
                        match cell.as_mut() {
                            Some(into) => {
                                if !paragraph.trim().is_empty() {
                                    if !into.is_empty() {
                                        into.push(' ');
                                    }
                                    into.push_str(paragraph.trim());
                                }
                            }
                            None => blocks.push(paragraph_block(
                                &paragraph,
                                style.as_deref(),
                                listed,
                                list_level,
                            )),
                        }
                    }
                    b"tc" => {
                        let done = cell.take().unwrap_or_default();
                        if let Some(table) = tables.last_mut() {
                            if let Some(row) = table.rows.last_mut() {
                                row.push(cell_text(&done));
                            }
                        }
                    }
                    b"tbl" => {
                        if let Some(table) = tables.pop() {
                            let rendered = markdown_table(&table.rows);
                            match tables.last_mut() {
                                // A nested table is flattened into the cell
                                // that holds it. Markdown has no nested
                                // tables and inventing one is worse.
                                Some(_) => {
                                    if let Some(into) = cell.as_mut() {
                                        into.push(' ');
                                        into.push_str(&cell_text(&rendered));
                                    }
                                }
                                None => blocks.push(rendered),
                            }
                        }
                    }
                    b"body" => in_body = false,
                    _ => {}
                }
            }
            XmlEvent::Eof => break,
            _ => {}
        }
    }
    join(blocks)
}

fn paragraph_block(text: &str, style: Option<&str>, listed: bool, level: usize) -> String {
    let body = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if body.is_empty() {
        return String::new();
    }
    if let Some(level) = style.and_then(heading_level) {
        return format!("{} {body}", "#".repeat(level));
    }
    if listed {
        // Bullets for every list. Telling a numbered list from a bulleted one
        // means resolving numbering.xml, and getting it wrong silently is
        // worse than a bullet that is honestly a bullet.
        return format!("{}- {body}", "  ".repeat(level.min(5)));
    }
    body
}

/* ------------------------------------------------------------------ */
/* odt                                                                 */
/* ------------------------------------------------------------------ */

fn odt(archive: &mut Archive<'_>) -> Result<String, Error> {
    let xml = archive.required("content.xml")?;
    Ok(odt_body(&xml))
}

fn odt_body(xml: &str) -> String {
    let mut reader = reader(xml);
    let mut blocks: Vec<String> = Vec::new();
    let mut tables: Vec<Table> = Vec::new();
    let mut cell: Option<String> = None;
    let mut text = String::new();
    let mut heading: Option<usize> = None;
    let mut list_depth = 0usize;
    let mut in_paragraph = false;
    // `office:automatic-styles` carries list and paragraph style definitions
    // that contain no document text. Starting at the body skips all of it.
    let mut in_body = false;

    // A document from the wild may not parse to the end. Stopping at the
    // first error keeps whatever was readable rather than throwing it away.
    while let Ok(event) = reader.read_event() {
        match event {
            XmlEvent::Start(e) => {
                if is(&e, "body") {
                    in_body = true;
                    continue;
                }
                if !in_body {
                    continue;
                }
                match local(e.name().as_ref()) {
                    b"h" => {
                        text.clear();
                        in_paragraph = true;
                        heading = attribute(&e, "outline-level")
                            .and_then(|v| v.parse::<usize>().ok())
                            .map(|level| level.clamp(1, 6));
                    }
                    b"p" => {
                        text.clear();
                        in_paragraph = true;
                        heading = None;
                    }
                    b"list" => list_depth += 1,
                    b"table" => tables.push(Table::default()),
                    b"table-row" => {
                        if let Some(table) = tables.last_mut() {
                            table.rows.push(Vec::new());
                        }
                    }
                    b"table-cell" => cell = Some(String::new()),
                    _ => {}
                }
            }
            XmlEvent::Empty(e) if in_body => match local(e.name().as_ref()) {
                b"s" => {
                    let count = attribute(&e, "c")
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(1)
                        .min(64);
                    text.push_str(&" ".repeat(count));
                }
                b"tab" | b"line-break" => text.push(' '),
                _ => {}
            },
            XmlEvent::Text(e) if in_paragraph && in_body => {
                if let Ok(decoded) = e.decode() {
                    text.push_str(&decoded);
                }
            }
            XmlEvent::GeneralRef(e) if in_paragraph && in_body => {
                if let Ok(name) = e.decode() {
                    text.push_str(&resolve_entity(&name));
                }
            }
            XmlEvent::End(e) => {
                if !in_body {
                    continue;
                }
                match local(e.name().as_ref()) {
                    b"p" | b"h" => {
                        in_paragraph = false;
                        let paragraph = std::mem::take(&mut text);
                        match cell.as_mut() {
                            Some(into) => {
                                if !paragraph.trim().is_empty() {
                                    if !into.is_empty() {
                                        into.push(' ');
                                    }
                                    into.push_str(paragraph.trim());
                                }
                            }
                            None => blocks.push(odt_block(
                                &paragraph,
                                heading,
                                list_depth.saturating_sub(1),
                                list_depth > 0,
                            )),
                        }
                        heading = None;
                    }
                    b"list" => list_depth = list_depth.saturating_sub(1),
                    b"table-cell" => {
                        let done = cell.take().unwrap_or_default();
                        if let Some(table) = tables.last_mut() {
                            if let Some(row) = table.rows.last_mut() {
                                row.push(cell_text(&done));
                            }
                        }
                    }
                    b"table" => {
                        if let Some(table) = tables.pop() {
                            let rendered = markdown_table(&table.rows);
                            match tables.last_mut() {
                                Some(_) => {
                                    if let Some(into) = cell.as_mut() {
                                        into.push(' ');
                                        into.push_str(&cell_text(&rendered));
                                    }
                                }
                                None => blocks.push(rendered),
                            }
                        }
                    }
                    b"body" => in_body = false,
                    _ => {}
                }
            }
            XmlEvent::Eof => break,
            _ => {}
        }
    }
    join(blocks)
}

fn odt_block(text: &str, heading: Option<usize>, level: usize, listed: bool) -> String {
    let body = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if body.is_empty() {
        return String::new();
    }
    if let Some(level) = heading {
        return format!("{} {body}", "#".repeat(level));
    }
    if listed {
        return format!("{}- {body}", "  ".repeat(level.min(5)));
    }
    body
}

/* ------------------------------------------------------------------ */
/* xlsx                                                                */
/* ------------------------------------------------------------------ */

fn xlsx(archive: &mut Archive<'_>) -> Result<String, Error> {
    let shared = match archive.part("xl/sharedStrings.xml")? {
        Some(xml) => shared_strings(&xml),
        None => Vec::new(),
    };
    let sheets = worksheet_parts(archive)?;
    if sheets.is_empty() {
        return Err(Error::MissingPart("worksheet"));
    }

    let limits = archive.limits;
    let mut blocks: Vec<String> = Vec::new();
    for (name, part) in sheets {
        let Some(xml) = archive.part(&part)? else {
            continue;
        };
        let (rows, clipped) = sheet_rows(&xml, &shared, limits);
        blocks.push(format!("## {}", cell_text(&name)));
        if rows.is_empty() {
            blocks.push("*(empty sheet)*".to_string());
            continue;
        }
        blocks.push(markdown_table(&rows));
        if clipped {
            blocks.push(format!(
                "*Only the first {} rows and {} columns of this sheet are shown.*",
                limits.sheet_rows, limits.sheet_cols
            ));
        }
    }
    Ok(join(blocks))
}

/// Sheet names paired with the part that holds them.
///
/// The order and the names come from `xl/workbook.xml`, resolved through the
/// workbook relationships. Without those the parts still get listed, sorted by
/// name, because a spreadsheet with generic sheet titles beats no spreadsheet.
fn worksheet_parts(archive: &mut Archive<'_>) -> Result<Vec<(String, String)>, Error> {
    let workbook = archive.part("xl/workbook.xml")?;
    let rels = archive.part("xl/_rels/workbook.xml.rels")?;
    if let (Some(workbook), Some(rels)) = (workbook, rels) {
        let targets = relationships(&rels);
        let mut sheets = Vec::new();
        for (name, id) in workbook_sheets(&workbook) {
            if let Some(target) = targets.iter().find(|(rid, _)| *rid == id) {
                sheets.push((name, normalize_target(&target.1)));
            }
        }
        if !sheets.is_empty() {
            return Ok(sheets);
        }
    }

    let mut parts: Vec<String> = archive
        .names()
        .into_iter()
        .filter(|n| n.starts_with("xl/worksheets/") && n.ends_with(".xml"))
        .collect();
    parts.sort_by_key(|name| (trailing_number(name), name.clone()));
    Ok(parts
        .into_iter()
        .enumerate()
        .map(|(index, part)| (format!("Sheet {}", index + 1), part))
        .collect())
}

/// A relationship target is relative to `xl/`, and may be written with a
/// leading slash meaning "from the package root".
fn normalize_target(target: &str) -> String {
    match target.strip_prefix('/') {
        Some(absolute) => absolute.to_string(),
        None => format!("xl/{target}"),
    }
}

fn workbook_sheets(xml: &str) -> Vec<(String, String)> {
    let mut reader = reader(xml);
    let mut sheets = Vec::new();
    // A document from the wild may not parse to the end. Stopping at the
    // first error keeps whatever was readable rather than throwing it away.
    while let Ok(event) = reader.read_event() {
        match event {
            XmlEvent::Start(e) | XmlEvent::Empty(e) if is(&e, "sheet") => {
                let name = attribute(&e, "name").unwrap_or_default();
                if let Some(id) = attribute(&e, "id") {
                    sheets.push((name, id));
                }
            }
            XmlEvent::Eof => break,
            _ => {}
        }
    }
    sheets
}

fn relationships(xml: &str) -> Vec<(String, String)> {
    let mut reader = reader(xml);
    let mut out = Vec::new();
    // A document from the wild may not parse to the end. Stopping at the
    // first error keeps whatever was readable rather than throwing it away.
    while let Ok(event) = reader.read_event() {
        match event {
            XmlEvent::Start(e) | XmlEvent::Empty(e) if is(&e, "Relationship") => {
                if let (Some(id), Some(target)) = (attribute(&e, "Id"), attribute(&e, "Target")) {
                    out.push((id, target));
                }
            }
            XmlEvent::Eof => break,
            _ => {}
        }
    }
    out
}

fn shared_strings(xml: &str) -> Vec<String> {
    let mut reader = reader(xml);
    let mut out = Vec::new();
    let mut current: Option<String> = None;
    let mut in_text = false;
    // A document from the wild may not parse to the end. Stopping at the
    // first error keeps whatever was readable rather than throwing it away.
    while let Ok(event) = reader.read_event() {
        match event {
            XmlEvent::Start(e) => match local(e.name().as_ref()) {
                b"si" => current = Some(String::new()),
                b"t" => in_text = true,
                _ => {}
            },
            XmlEvent::Text(e) if in_text => {
                if let (Some(into), Ok(decoded)) = (current.as_mut(), e.decode()) {
                    into.push_str(&decoded);
                }
            }
            XmlEvent::GeneralRef(e) if in_text => {
                if let (Some(into), Ok(name)) = (current.as_mut(), e.decode()) {
                    into.push_str(&resolve_entity(&name));
                }
            }
            XmlEvent::End(e) => match local(e.name().as_ref()) {
                b"t" => in_text = false,
                // A shared string built from several runs is one string; the
                // runs exist to carry formatting this reader drops.
                b"si" => out.push(current.take().unwrap_or_default()),
                _ => {}
            },
            XmlEvent::Eof => break,
            _ => {}
        }
    }
    out
}

/// Rows of one worksheet, and whether the caps clipped it.
fn sheet_rows(xml: &str, shared: &[String], limits: Limits) -> (Vec<Vec<String>>, bool) {
    let mut reader = reader(xml);
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut clipped = false;
    let mut in_row = false;
    let mut column = 0usize;
    let mut cell_type = String::new();
    let mut value = String::new();
    let mut in_value = false;
    let mut in_inline = false;

    // A document from the wild may not parse to the end. Stopping at the
    // first error keeps whatever was readable rather than throwing it away.
    while let Ok(event) = reader.read_event() {
        match event {
            XmlEvent::Start(e) | XmlEvent::Empty(e) => match local(e.name().as_ref()) {
                b"row" => {
                    in_row = true;
                    row = Vec::new();
                }
                b"c" => {
                    cell_type = attribute(&e, "t").unwrap_or_default();
                    value.clear();
                    // The `r` reference is the only thing that says which
                    // column a cell is in. Without it a sheet with gaps
                    // silently shifts every value left.
                    column = attribute(&e, "r")
                        .as_deref()
                        .and_then(column_index)
                        .unwrap_or(row.len());
                }
                b"v" => in_value = true,
                b"is" => in_inline = true,
                b"t" if in_inline => in_value = true,
                _ => {}
            },
            XmlEvent::Text(e) if in_value => {
                if let Ok(decoded) = e.decode() {
                    value.push_str(&decoded);
                }
            }
            XmlEvent::GeneralRef(e) if in_value => {
                if let Ok(name) = e.decode() {
                    value.push_str(&resolve_entity(&name));
                }
            }
            XmlEvent::End(e) => match local(e.name().as_ref()) {
                b"v" => in_value = false,
                b"t" if in_inline => in_value = false,
                b"is" => in_inline = false,
                b"c" => {
                    let rendered = render_cell(&cell_type, &value, shared);
                    if column < limits.sheet_cols {
                        while row.len() < column {
                            row.push(String::new());
                        }
                        row.push(cell_text(&rendered));
                    } else if !rendered.trim().is_empty() {
                        clipped = true;
                    }
                }
                b"row" if in_row => {
                    in_row = false;
                    if rows.len() < limits.sheet_rows {
                        // A sheet's trailing empty rows carry no information
                        // and make the table taller than the pane.
                        if row.iter().any(|c| !c.is_empty()) {
                            rows.push(std::mem::take(&mut row));
                        }
                    } else {
                        clipped = true;
                    }
                }
                _ => {}
            },
            XmlEvent::Eof => break,
            _ => {}
        }
    }
    (rows, clipped)
}

fn render_cell(cell_type: &str, value: &str, shared: &[String]) -> String {
    match cell_type {
        // A shared string index out of range is a broken file, not a reason
        // to render somebody else's cell.
        "s" => value
            .parse::<usize>()
            .ok()
            .and_then(|index| shared.get(index).cloned())
            .unwrap_or_default(),
        "b" => match value {
            "1" => "TRUE".to_string(),
            "0" => "FALSE".to_string(),
            other => other.to_string(),
        },
        _ => value.to_string(),
    }
}

/// `BC12` is column 55. Letters only, and a reference with no letters at all
/// is not a reference.
fn column_index(reference: &str) -> Option<usize> {
    let mut index = 0usize;
    let mut seen = false;
    for ch in reference.chars() {
        if !ch.is_ascii_alphabetic() {
            break;
        }
        seen = true;
        index = index.checked_mul(26)?.checked_add(
            (ch.to_ascii_uppercase() as usize)
                .checked_sub('A' as usize)?
                .checked_add(1)?,
        )?;
    }
    if !seen {
        return None;
    }
    index.checked_sub(1)
}

/* ------------------------------------------------------------------ */
/* pptx                                                                */
/* ------------------------------------------------------------------ */

fn pptx(archive: &mut Archive<'_>) -> Result<String, Error> {
    let mut parts: Vec<String> = archive
        .names()
        .into_iter()
        .filter(|n| n.starts_with("ppt/slides/") && n.ends_with(".xml"))
        .collect();
    if parts.is_empty() {
        return Err(Error::MissingPart("ppt/slides"));
    }
    // `slide10.xml` sorts before `slide2.xml` as a string, and a deck shown
    // out of order is worse than one that is merely plain.
    parts.sort_by_key(|name| (trailing_number(name), name.clone()));
    parts.truncate(archive.limits.slides);

    let mut blocks: Vec<String> = Vec::new();
    for (index, part) in parts.iter().enumerate() {
        let Some(xml) = archive.part(part)? else {
            continue;
        };
        blocks.push(format!("## Slide {}", index + 1));
        let lines = slide_lines(&xml);
        if lines.is_empty() {
            blocks.push("*(no text on this slide)*".to_string());
            continue;
        }
        blocks.extend(lines);
    }
    Ok(join(blocks))
}

fn slide_lines(xml: &str) -> Vec<String> {
    let mut reader = reader(xml);
    let mut lines = Vec::new();
    let mut text = String::new();
    let mut in_paragraph = false;
    let mut in_text = false;
    // A document from the wild may not parse to the end. Stopping at the
    // first error keeps whatever was readable rather than throwing it away.
    while let Ok(event) = reader.read_event() {
        match event {
            XmlEvent::Start(e) => match local(e.name().as_ref()) {
                b"p" => {
                    in_paragraph = true;
                    text.clear();
                }
                b"t" => in_text = true,
                _ => {}
            },
            XmlEvent::Text(e) if in_text => {
                if let Ok(decoded) = e.decode() {
                    text.push_str(&decoded);
                }
            }
            XmlEvent::GeneralRef(e) if in_text => {
                if let Ok(name) = e.decode() {
                    text.push_str(&resolve_entity(&name));
                }
            }
            XmlEvent::End(e) => match local(e.name().as_ref()) {
                b"t" => in_text = false,
                b"p" if in_paragraph => {
                    in_paragraph = false;
                    let line = std::mem::take(&mut text);
                    let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
                    if !line.is_empty() {
                        lines.push(line);
                    }
                }
                _ => {}
            },
            XmlEvent::Eof => break,
            _ => {}
        }
    }
    lines
}

/// The digits at the end of a part name, for sorting `slide2` before
/// `slide10`. Names with no trailing digits sort first and then by name.
fn trailing_number(name: &str) -> u64 {
    let stem = name.rsplit('/').next().unwrap_or(name);
    let stem = stem.split('.').next().unwrap_or(stem);
    let digits: String = stem
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    digits.parse().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    /// Build a zip in memory, the way every one of these formats is packaged.
    fn archive(parts: &[(&str, &str)]) -> Vec<u8> {
        let mut buffer = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buffer));
            for (name, body) in parts {
                writer
                    .start_file(
                        *name,
                        SimpleFileOptions::default()
                            .compression_method(zip::CompressionMethod::Deflated),
                    )
                    .unwrap();
                writer.write_all(body.as_bytes()).unwrap();
            }
            writer.finish().unwrap();
        }
        buffer
    }

    const DOCX: &str = r#"<?xml version="1.0"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Findings</w:t></w:r></w:p>
    <w:p><w:r><w:t>Latency fell by </w:t></w:r><w:r><w:t>40%.</w:t></w:r></w:p>
    <w:p><w:pPr><w:pStyle w:val="Heading2"/></w:pPr><w:r><w:t>Detail</w:t></w:r></w:p>
    <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/></w:numPr></w:pPr><w:r><w:t>First point</w:t></w:r></w:p>
    <w:p><w:pPr><w:numPr><w:ilvl w:val="1"/></w:numPr></w:pPr><w:r><w:t>Nested point</w:t></w:r></w:p>
    <w:tbl>
      <w:tr><w:tc><w:p><w:r><w:t>Region</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>p99</w:t></w:r></w:p></w:tc></w:tr>
      <w:tr><w:tc><w:p><w:r><w:t>eu-west</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>12ms</w:t></w:r></w:p></w:tc></w:tr>
    </w:tbl>
  </w:body>
</w:document>"#;

    #[test]
    fn a_docx_becomes_headings_paragraphs_lists_and_a_table() {
        let bytes = archive(&[("word/document.xml", DOCX)]);
        let md = to_markdown(Kind::Docx, &bytes).unwrap();

        assert!(md.contains("# Findings"), "{md}");
        assert!(md.contains("## Detail"), "{md}");
        assert!(md.contains("Latency fell by 40%."), "{md}");
        assert!(md.contains("- First point"), "{md}");
        assert!(md.contains("  - Nested point"), "{md}");
        assert!(md.contains("| Region | p99 |"), "{md}");
        assert!(md.contains("| --- | --- |"), "{md}");
        assert!(md.contains("| eu-west | 12ms |"), "{md}");
    }

    /// The text of a table cell must not be able to add a column to the row
    /// it sits in. It is only a rendering bug, but a table that silently
    /// reshapes itself is a document that lies about its own contents.
    #[test]
    fn a_pipe_inside_a_cell_does_not_add_a_column() {
        let xml = r#"<w:document xmlns:w="x"><w:body><w:tbl>
          <w:tr><w:tc><w:p><w:r><w:t>a|b</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>c</w:t></w:r></w:p></w:tc></w:tr>
        </w:tbl></w:body></w:document>"#;
        let bytes = archive(&[("word/document.xml", xml)]);
        let md = to_markdown(Kind::Docx, &bytes).unwrap();
        assert!(md.contains(r"a\|b"), "the pipe was left bare: {md}");
    }

    /// A `.docx` that is a zip of something else is a message, not an empty
    /// pane. An empty pane and an empty document look the same.
    #[test]
    fn a_docx_with_no_document_part_says_so() {
        let bytes = archive(&[("word/styles.xml", "<styles/>")]);
        assert_eq!(
            to_markdown(Kind::Docx, &bytes),
            Err(Error::MissingPart("word/document.xml"))
        );
    }

    #[test]
    fn something_that_is_not_a_zip_is_a_readable_refusal() {
        let error = to_markdown(Kind::Docx, b"this is just text").unwrap_err();
        assert!(matches!(error, Error::NotAnArchive(_)), "{error:?}");
        assert!(error.to_string().contains("not a readable office document"));
    }

    const ODT: &str = r#"<?xml version="1.0"?>
<office:document-content xmlns:office="urn:o" xmlns:text="urn:t" xmlns:table="urn:tb">
  <office:automatic-styles><text:p>style sheet noise</text:p></office:automatic-styles>
  <office:body><office:text>
    <text:h text:outline-level="1">Report</text:h>
    <text:p>Plain paragraph.</text:p>
    <text:list><text:list-item><text:p>Bullet one</text:p></text:list-item>
      <text:list-item><text:list><text:list-item><text:p>Deeper</text:p></text:list-item></text:list></text:list-item>
    </text:list>
    <table:table>
      <table:table-row><table:table-cell><text:p>Name</text:p></table:table-cell><table:table-cell><text:p>Value</text:p></table:table-cell></table:table-row>
      <table:table-row><table:table-cell><text:p>rows</text:p></table:table-cell><table:table-cell><text:p>7</text:p></table:table-cell></table:table-row>
    </table:table>
  </office:text></office:body>
</office:document-content>"#;

    #[test]
    fn an_odt_becomes_headings_paragraphs_lists_and_a_table() {
        let bytes = archive(&[("content.xml", ODT)]);
        let md = to_markdown(Kind::Odt, &bytes).unwrap();

        assert!(md.contains("# Report"), "{md}");
        assert!(md.contains("Plain paragraph."), "{md}");
        assert!(md.contains("- Bullet one"), "{md}");
        assert!(md.contains("  - Deeper"), "{md}");
        assert!(md.contains("| Name | Value |"), "{md}");
        assert!(md.contains("| rows | 7 |"), "{md}");
        assert!(
            !md.contains("style sheet noise"),
            "style definitions leaked into the document: {md}"
        );
    }

    #[test]
    fn an_xlsx_becomes_one_markdown_table_per_sheet_with_shared_strings_resolved() {
        let workbook = r#"<workbook xmlns:r="urn:r">
          <sheets><sheet name="Latency" sheetId="1" r:id="rId1"/><sheet name="Cost" sheetId="2" r:id="rId2"/></sheets>
        </workbook>"#;
        let rels = r#"<Relationships>
          <Relationship Id="rId1" Target="worksheets/sheet1.xml"/>
          <Relationship Id="rId2" Target="worksheets/sheet2.xml"/>
        </Relationships>"#;
        let shared =
            r#"<sst><si><t>Region</t></si><si><t>p99</t></si><si><t>eu-west</t></si></sst>"#;
        let sheet1 = r#"<worksheet><sheetData>
          <row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row>
          <row r="2"><c r="A2" t="s"><v>2</v></c><c r="C2"><v>12.5</v></c></row>
        </sheetData></worksheet>"#;
        let sheet2 = r#"<worksheet><sheetData>
          <row r="1"><c r="A1" t="inlineStr"><is><t>Total</t></is></c><c r="B1"><v>99</v></c></row>
        </sheetData></worksheet>"#;

        let bytes = archive(&[
            ("xl/workbook.xml", workbook),
            ("xl/_rels/workbook.xml.rels", rels),
            ("xl/sharedStrings.xml", shared),
            ("xl/worksheets/sheet1.xml", sheet1),
            ("xl/worksheets/sheet2.xml", sheet2),
        ]);
        let md = to_markdown(Kind::Xlsx, &bytes).unwrap();

        assert!(md.contains("## Latency"), "{md}");
        assert!(md.contains("## Cost"), "{md}");
        assert!(md.contains("| Region | p99 |"), "{md}");
        // C2 is the third column, so the gap at B2 has to be preserved.
        assert!(md.contains("| eu-west |   | 12.5 |"), "{md}");
        assert!(md.contains("| Total | 99 |"), "{md}");
    }

    #[test]
    fn a_pptx_becomes_one_heading_per_slide_in_slide_order() {
        let slide =
            |text: &str| format!(r#"<p:sld xmlns:a="urn:a"><a:p><a:t>{text}</a:t></a:p></p:sld>"#);
        let one = slide("Opening");
        let two = slide("Middle");
        let ten = slide("Closing");
        let bytes = archive(&[
            ("ppt/slides/slide1.xml", one.as_str()),
            ("ppt/slides/slide10.xml", ten.as_str()),
            ("ppt/slides/slide2.xml", two.as_str()),
        ]);
        let md = to_markdown(Kind::Pptx, &bytes).unwrap();

        assert!(md.contains("## Slide 1"), "{md}");
        assert!(md.contains("Opening"), "{md}");
        let opening = md.find("Opening").unwrap();
        let middle = md.find("Middle").unwrap();
        let closing = md.find("Closing").unwrap();
        assert!(
            opening < middle && middle < closing,
            "slide10 sorted before slide2: {md}"
        );
    }

    /// A model wrote this file. An archive claiming a hundred thousand parts
    /// is not a document, and walking it is the whole attack.
    #[test]
    fn an_archive_with_absurdly_many_entries_is_refused() {
        let parts: Vec<(String, &str)> =
            (0..40).map(|i| (format!("part{i}.xml"), "<a/>")).collect();
        let borrowed: Vec<(&str, &str)> = parts.iter().map(|(n, b)| (n.as_str(), *b)).collect();
        let bytes = archive(&borrowed);

        let limits = Limits {
            entries: 8,
            ..Limits::default()
        };
        assert_eq!(
            to_markdown_limited(Kind::Docx, &bytes, limits),
            Err(Error::TooManyEntries(40))
        );
    }

    /// The zip bomb. A part whose compressed size is nothing and whose
    /// decompressed size is everything. The cap has to be enforced on what
    /// comes out of the decompressor, because the header is as much a lie as
    /// the rest of the file.
    #[test]
    fn a_part_that_decompresses_past_the_cap_is_refused_rather_than_allocated_for() {
        let huge = "<w:document><w:body>".to_string()
            + &"<w:p><w:r><w:t>x</w:t></w:r></w:p>".repeat(200_000)
            + "</w:body></w:document>";
        let bytes = archive(&[("word/document.xml", huge.as_str())]);
        assert!(
            bytes.len() < 200_000,
            "the test input did not actually compress, so it proves nothing"
        );

        let limits = Limits {
            part_bytes: 64 * 1024,
            ..Limits::default()
        };
        assert_eq!(
            to_markdown_limited(Kind::Docx, &bytes, limits),
            Err(Error::PartTooLarge)
        );
    }

    /// The other bomb: an entity that expands into an entity that expands
    /// into an entity. Nothing here expands a declared entity at all, so the
    /// document renders the reference as text and the attack is a non-event.
    #[test]
    fn a_declared_entity_is_not_expanded_and_the_predefined_ones_are() {
        let xml = r#"<?xml version="1.0"?>
<!DOCTYPE w:document [ <!ENTITY boom "&boom;&boom;"> ]>
<w:document xmlns:w="x"><w:body>
  <w:p><w:r><w:t>caf&#233; &amp; bar &boom;</w:t></w:r></w:p>
</w:body></w:document>"#;
        let bytes = archive(&[("word/document.xml", xml)]);
        let md = to_markdown(Kind::Docx, &bytes).unwrap();
        assert!(md.contains("café & bar"), "{md}");
        assert!(md.contains("&boom;"), "the reference was swallowed: {md}");
    }

    #[test]
    fn a_document_with_no_extractable_text_says_so_rather_than_rendering_blank() {
        let bytes = archive(&[("word/document.xml", "<w:document><w:body/></w:document>")]);
        let md = to_markdown(Kind::Docx, &bytes).unwrap();
        assert!(md.contains("no text this reader can extract"), "{md}");
    }

    #[test]
    fn the_extension_picks_the_format() {
        assert_eq!(Kind::for_path(Path::new("a/b.docx")), Some(Kind::Docx));
        assert_eq!(Kind::for_path(Path::new("REPORT.ODT")), Some(Kind::Odt));
        assert_eq!(Kind::for_path(Path::new("x.xlsx")), Some(Kind::Xlsx));
        assert_eq!(Kind::for_path(Path::new("x.pptx")), Some(Kind::Pptx));
        assert_eq!(Kind::for_path(Path::new("x.md")), None);
    }

    #[test]
    fn a_column_reference_names_a_column() {
        assert_eq!(column_index("A1"), Some(0));
        assert_eq!(column_index("C2"), Some(2));
        assert_eq!(column_index("AA10"), Some(26));
        assert_eq!(column_index("12"), None);
    }
}
