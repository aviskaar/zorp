//! Rendering a paper to PDF.
//!
//! There is no typesetting dependency here and no external binary. The
//! document shape is fixed and narrow, so the parts of typesetting that
//! are genuinely hard, and that a real engine exists to solve, are the
//! parts this document does not have: no figures, no tables, no maths,
//! no floats, no footnotes. What is left is one column of text in the
//! PDF base-14 fonts, which is a page-description problem rather than a
//! layout problem.
//!
//! Nothing in here reads the clock. `PdfOptions::creation_date` is the
//! only value that could vary between runs, and the caller supplies it,
//! so the same paper is the same bytes.

pub mod layout;
pub mod metrics;
mod writer;

use crate::{Block, Paper};
use layout::Run;
use metrics::{Face, ALL_FACES};
use std::fmt::Write as _;
use writer::Pdf;

/// Knobs that would otherwise make two runs of the same input differ.
#[derive(Debug, Clone, Default)]
pub struct PdfOptions {
    /// `/CreationDate`, in PDF date syntax. `None` omits the key, which
    /// is what keeps the default output byte-identical between runs.
    pub creation_date: Option<String>,
}

const PAGE_WIDTH: f32 = 612.0;
const PAGE_HEIGHT: f32 = 792.0;
const MARGIN: f32 = 72.0;
const COLUMN: f32 = PAGE_WIDTH - 2.0 * MARGIN;
const TOP: f32 = PAGE_HEIGHT - MARGIN;
const BOTTOM: f32 = MARGIN;
const FOOTER_BASELINE: f32 = 40.0;

const TITLE_SIZE: f32 = 17.0;
const BYLINE_SIZE: f32 = 11.0;
const PROVENANCE_SIZE: f32 = 9.0;
const BODY_SIZE: f32 = 10.5;
const BODY_LEADING: f32 = 13.5;
const FOOTER_SIZE: f32 = 9.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Align {
    Left,
    Center,
}

/// Escape one string for a PDF literal, mapping what it can to WinAnsi.
///
/// Everything the writer emits is ASCII: bytes above 126 go out as octal
/// escapes, so the file never carries raw UTF-8 that a viewer would read
/// under a single-byte encoding and get wrong.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    for c in text.chars() {
        match c {
            '\\' => out.push_str(r"\\"),
            '(' => out.push_str(r"\("),
            ')' => out.push_str(r"\)"),
            '\n' | '\r' | '\t' => out.push(' '),
            c if (c as u32) < 0x20 => out.push(' '),
            c if (c as u32) <= 0x7E => out.push(c),
            c => {
                let _ = write!(out, "\\{:03o}", win_ansi(c));
            }
        }
    }
    out
}

/// The WinAnsi byte for a character, or `?` for anything the encoding
/// has no glyph for. Substituting is the honest failure here: a document
/// that silently dropped the character would read as if the author had
/// never written it.
fn win_ansi(c: char) -> u8 {
    match c as u32 {
        0x2018 => 0x91,
        0x2019 => 0x92,
        0x201C => 0x93,
        0x201D => 0x94,
        0x2013 => 0x96,
        0x2014 => 0x97,
        0x2026 => 0x85,
        0x2022 => 0x95,
        0x20AC => 0x80,
        0x2122 => 0x99,
        // Latin-1 and WinAnsi agree from 0xA0 up.
        code @ 0xA0..=0xFF => code as u8,
        _ => b'?',
    }
}

struct Composer {
    pages: Vec<String>,
    current: String,
    y: f32,
}

impl Composer {
    fn new() -> Composer {
        Composer {
            pages: Vec::new(),
            current: String::new(),
            y: TOP,
        }
    }

    fn break_page(&mut self) {
        let number = self.pages.len() + 1;
        let label = number.to_string();
        let width = metrics::text_width(Face::Roman, &label, FOOTER_SIZE);
        let x = (PAGE_WIDTH - width) / 2.0;
        let _ = write!(
            self.current,
            "BT\n1 0 0 1 {x:.2} {FOOTER_BASELINE:.2} Tm\n/F1 {FOOTER_SIZE:.2} Tf\n({}) Tj\nET\n",
            escape(&label)
        );
        self.pages.push(std::mem::take(&mut self.current));
        self.y = TOP;
    }

    /// Reserve the next baseline, starting a page if this line would sit
    /// below the bottom margin.
    fn baseline(&mut self, leading: f32) -> f32 {
        if self.y - leading < BOTTOM {
            self.break_page();
        }
        self.y -= leading;
        self.y
    }

    fn gap(&mut self, amount: f32) {
        self.y -= amount;
    }

    fn draw(&mut self, line: &[Run], size: f32, x: f32, baseline: f32) {
        if line.is_empty() {
            return;
        }
        let _ = write!(self.current, "BT\n1 0 0 1 {x:.2} {baseline:.2} Tm\n");
        for run in line {
            let _ = write!(
                self.current,
                "/{} {size:.2} Tf\n({}) Tj\n",
                run.face.resource(),
                escape(&run.text)
            );
        }
        self.current.push_str("ET\n");
    }

    /// Lay a paragraph of inline markdown into the column and draw it.
    #[allow(clippy::too_many_arguments)]
    fn paragraph(
        &mut self,
        text: &str,
        base: Face,
        size: f32,
        leading: f32,
        x: f32,
        width: f32,
        align: Align,
    ) {
        let runs = layout::parse_inline(text, base);
        for line in layout::wrap(&runs, size, width) {
            let baseline = self.baseline(leading);
            let start = match align {
                Align::Left => x,
                Align::Center => x + (width - layout::line_width(&line, size)) / 2.0,
            };
            self.draw(&line, size, start, baseline);
        }
    }

    /// A heading, kept with at least one line of what follows it. A
    /// heading alone at the foot of a page is the one pagination fault
    /// worth spending code on here.
    fn heading(&mut self, text: &str, size: f32, leading: f32) {
        self.gap(leading * 0.6);
        if self.y - leading - BODY_LEADING < BOTTOM {
            self.break_page();
        }
        self.paragraph(text, Face::Bold, size, leading, MARGIN, COLUMN, Align::Left);
        self.gap(2.0);
    }

    /// A marker in the left margin and a wrapped block hanging off it.
    /// Both list items and reference entries have this shape, and a
    /// reference whose second line ran back to the margin would be hard
    /// to tell from the start of the next one.
    fn hanging(&mut self, marker: &str, text: &str, indent: f32) {
        let runs = layout::parse_inline(text, Face::Roman);
        for (i, line) in layout::wrap(&runs, BODY_SIZE, COLUMN - indent)
            .into_iter()
            .enumerate()
        {
            let baseline = self.baseline(BODY_LEADING);
            if i == 0 {
                let label = vec![Run::new(marker, Face::Roman)];
                self.draw(&label, BODY_SIZE, MARGIN, baseline);
            }
            self.draw(&line, BODY_SIZE, MARGIN + indent, baseline);
        }
    }

    fn bullet(&mut self, text: &str) {
        self.hanging("\u{2022}", text, 14.0);
    }

    fn code(&mut self, text: &str) {
        self.gap(4.0);
        for raw in text.lines() {
            let runs = vec![Run::new(raw, Face::Mono)];
            for line in layout::wrap(&runs, BODY_SIZE - 1.0, COLUMN - 14.0) {
                let baseline = self.baseline(BODY_LEADING);
                self.draw(&line, BODY_SIZE - 1.0, MARGIN + 14.0, baseline);
            }
        }
        self.gap(4.0);
    }

    fn finish(mut self) -> Vec<String> {
        if !self.current.is_empty() || self.pages.is_empty() {
            self.break_page();
        }
        self.pages
    }
}

/// Section numbers, computed once so the markdown and the PDF cannot
/// disagree about which section is which.
fn numbered(sections: &[crate::Section]) -> Vec<(String, u8, &crate::Section)> {
    let mut counters = [0usize; 3];
    let mut out = Vec::with_capacity(sections.len());
    for section in sections {
        let level = section.level.clamp(1, 3);
        let depth = (level - 1) as usize;
        counters[depth] += 1;
        for counter in counters.iter_mut().skip(depth + 1) {
            *counter = 0;
        }
        let label = counters[..=depth]
            .iter()
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(".");
        out.push((label, level, section));
    }
    out
}

fn heading_size(level: u8) -> (f32, f32) {
    match level {
        1 => (13.0, 16.0),
        2 => (11.5, 14.0),
        _ => (10.5, 13.0),
    }
}

pub fn render(paper: &Paper, options: &PdfOptions) -> Vec<u8> {
    let parts = paper.parts();
    let mut page = Composer::new();

    page.gap(18.0);
    page.paragraph(
        &parts.title,
        Face::Bold,
        TITLE_SIZE,
        TITLE_SIZE * 1.25,
        MARGIN,
        COLUMN,
        Align::Center,
    );
    page.gap(6.0);
    if !parts.authors.is_empty() {
        page.paragraph(
            &parts.authors.join(", "),
            Face::Roman,
            BYLINE_SIZE,
            BYLINE_SIZE * 1.3,
            MARGIN,
            COLUMN,
            Align::Center,
        );
    }
    if !parts.date.is_empty() {
        page.paragraph(
            &parts.date,
            Face::Roman,
            PROVENANCE_SIZE,
            PROVENANCE_SIZE * 1.3,
            MARGIN,
            COLUMN,
            Align::Center,
        );
    }
    for line in &parts.provenance {
        page.paragraph(
            line,
            Face::Italic,
            PROVENANCE_SIZE,
            PROVENANCE_SIZE * 1.3,
            MARGIN,
            COLUMN,
            Align::Center,
        );
    }
    page.gap(14.0);

    if !parts.abstract_text.trim().is_empty() {
        page.heading("Abstract", 11.5, 14.0);
        page.paragraph(
            parts.abstract_text.trim(),
            Face::Roman,
            BODY_SIZE,
            BODY_LEADING,
            MARGIN,
            COLUMN,
            Align::Left,
        );
        page.gap(6.0);
    }

    for (label, level, section) in numbered(&parts.sections) {
        if !section.title.trim().is_empty() {
            let (size, leading) = heading_size(level);
            page.heading(&format!("{label}  {}", section.title.trim()), size, leading);
        }
        for block in &section.blocks {
            match block {
                Block::Paragraph(text) => {
                    page.paragraph(
                        text.trim(),
                        Face::Roman,
                        BODY_SIZE,
                        BODY_LEADING,
                        MARGIN,
                        COLUMN,
                        Align::Left,
                    );
                    page.gap(4.0);
                }
                Block::Bullet(text) => page.bullet(text.trim()),
                Block::Code(text) => page.code(text),
            }
        }
    }

    page.heading("References", 13.0, 16.0);
    if parts.references.is_empty() {
        page.paragraph(
            "The evidence record for this track is empty, so this paper has no references.",
            Face::Roman,
            BODY_SIZE,
            BODY_LEADING,
            MARGIN,
            COLUMN,
            Align::Left,
        );
    } else {
        for (i, reference) in parts.references.iter().enumerate() {
            page.hanging(
                &format!("[{}]", i + 1),
                &format!(
                    "{}: {} ({})",
                    reference.key,
                    reference.claim.trim(),
                    reference.source.trim()
                ),
                26.0,
            );
            page.gap(3.0);
        }
    }

    assemble(page.finish(), &parts.title, options)
}

fn assemble(streams: Vec<String>, title: &str, options: &PdfOptions) -> Vec<u8> {
    let mut pdf = Pdf::new();
    let catalog = pdf.reserve();
    let pages = pdf.reserve();
    let fonts: Vec<usize> = ALL_FACES.iter().map(|_| pdf.reserve()).collect();
    let info = pdf.reserve();
    let page_ids: Vec<usize> = streams.iter().map(|_| pdf.reserve()).collect();
    let content_ids: Vec<usize> = streams.iter().map(|_| pdf.reserve()).collect();

    pdf.put(catalog, format!("<< /Type /Catalog /Pages {pages} 0 R >>"));

    let kids = page_ids
        .iter()
        .map(|id| format!("{id} 0 R"))
        .collect::<Vec<_>>()
        .join(" ");
    pdf.put(
        pages,
        format!(
            "<< /Type /Pages /Kids [{kids}] /Count {} >>",
            page_ids.len()
        ),
    );

    for (face, id) in ALL_FACES.iter().zip(&fonts) {
        pdf.put(
            *id,
            format!(
                "<< /Type /Font /Subtype /Type1 /BaseFont /{} /Encoding /WinAnsiEncoding >>",
                face.base_font()
            ),
        );
    }

    let mut info_dict = format!("<< /Producer (zorp-paper) /Title ({}) ", escape(title));
    if let Some(date) = &options.creation_date {
        let _ = write!(info_dict, "/CreationDate ({}) ", escape(date));
    }
    info_dict.push_str(">>");
    pdf.put(info, info_dict);

    let resources = ALL_FACES
        .iter()
        .zip(&fonts)
        .map(|(face, id)| format!("/{} {id} 0 R", face.resource()))
        .collect::<Vec<_>>()
        .join(" ");

    for ((page_id, content_id), stream) in page_ids.iter().zip(&content_ids).zip(&streams) {
        pdf.put(
            *page_id,
            format!(
                "<< /Type /Page /Parent {pages} 0 R /MediaBox [0 0 {PAGE_WIDTH:.0} {PAGE_HEIGHT:.0}] \
                 /Resources << /Font << {resources} >> >> /Contents {content_id} 0 R >>"
            ),
        );
        pdf.put(*content_id, Pdf::stream(stream));
    }

    pdf.finish(catalog, info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PaperParts, Section};

    #[test]
    fn escaping_covers_the_three_characters_that_can_break_a_literal() {
        assert_eq!(escape(r"a(b)c\d"), r"a\(b\)c\\d");
    }

    #[test]
    fn a_newline_inside_a_run_becomes_a_space() {
        assert_eq!(escape("a\nb"), "a b");
    }

    #[test]
    fn non_ascii_leaves_as_an_octal_escape() {
        assert_eq!(escape("\u{2014}"), "\\227");
        assert_eq!(escape("\u{e9}"), "\\351");
    }

    #[test]
    fn a_character_winansi_cannot_hold_becomes_a_question_mark() {
        assert_eq!(escape("\u{4e2d}"), "\\077");
    }

    #[test]
    fn section_numbers_nest_and_reset() {
        let sections = vec![
            Section {
                title: "One".into(),
                level: 1,
                blocks: vec![],
            },
            Section {
                title: "One point one".into(),
                level: 2,
                blocks: vec![],
            },
            Section {
                title: "Two".into(),
                level: 1,
                blocks: vec![],
            },
            Section {
                title: "Two point one".into(),
                level: 2,
                blocks: vec![],
            },
        ];
        let labels: Vec<String> = numbered(&sections)
            .into_iter()
            .map(|(label, _, _)| label)
            .collect();
        assert_eq!(labels, vec!["1", "1.1", "2", "2.1"]);
    }

    #[test]
    fn a_paper_with_nothing_in_it_still_produces_one_page() {
        let paper = crate::Paper::assemble(PaperParts {
            title: "Bare".into(),
            ..PaperParts::default()
        })
        .unwrap();
        let bytes = render(&paper, &PdfOptions::default());
        let text = String::from_utf8_lossy(&bytes);
        assert_eq!(text.matches("/Type /Page /Parent").count(), 1);
    }
}
