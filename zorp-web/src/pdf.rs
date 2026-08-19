//! Pulling readable text out of a PDF, as markdown.
//!
//! A PDF is not a document in the sense the office formats are. It is a
//! program that places glyphs at coordinates, and the closest thing to a
//! paragraph in it is a run of glyphs that happen to sit next to each other.
//! So this module recovers text and nothing else: no headings, no lists, no
//! tables, because none of those are in the file to recover. Same scope as
//! `crate::documents`, for the same reason. This is for reading what a run
//! produced, not for rendering a document.
//!
//! The reading itself is `pdf-extract`'s, not ours. What is ours is deciding
//! that the text is all we want, tidying what comes back, and treating the
//! input as hostile: a model wrote this file or downloaded it, so the caller
//! runs the extraction where a panic in it cannot take anything else down.
//! See `crate::api::read_artifact`.

/// Why a PDF could not be read. One variant, because from the pane's point of
/// view there is one outcome that matters: the reader could not get text out,
/// and it has to say so rather than show a blank pane.
#[derive(Debug, Eq, PartialEq)]
pub struct Unreadable(pub String);

impl std::fmt::Display for Unreadable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "this file is not a readable PDF: {}", self.0)
    }
}

impl std::error::Error for Unreadable {}

/// Turn one PDF into the markdown the pane renders.
///
/// The markdown is paragraphs and nothing else. That is not a shortcut, it is
/// what the format supports: a PDF records where each glyph was drawn, so a
/// heading in one is text that happened to be set in a larger font, and
/// promoting it to `#` would be inventing a structure the file does not
/// carry. What does survive is the text and the breaks between blocks, which
/// is what somebody opening the pane is asking for.
pub fn to_markdown(bytes: &[u8]) -> Result<String, Unreadable> {
    let text = pdf_extract::extract_text_from_mem(bytes).map_err(|e| Unreadable(e.to_string()))?;
    let text = tidy(&text);
    if text.is_empty() {
        // The common way to reach here is a scan: pictures of words, and no
        // words. Saying so beats a pane that looks broken.
        return Ok("*This PDF has no text this reader can extract. A scanned page holds pictures of words, not words.*".to_string());
    }
    Ok(text)
}

/// Make extracted text into blocks a markdown renderer will lay out.
///
/// Three things come back from the extractor that are page furniture rather
/// than content. Lines are padded out to the width of the page, pages are
/// separated by a form feed, and the run of empty lines around both is as
/// wide as the layout happened to be. None of that is in the document, so
/// none of it is passed on. What is left is blocks separated by one blank
/// line, which is what the renderer reflows into paragraphs.
fn tidy(text: &str) -> String {
    let mut blocks: Vec<String> = Vec::new();
    let mut block: Vec<&str> = Vec::new();

    // A form feed is a page break, and a page break is at least a paragraph
    // break, so it becomes one rather than being dropped into the middle of
    // a sentence.
    let text = text.replace('\r', "\n").replace('\u{c}', "\n\n");
    for line in text.split('\n') {
        let line = line.trim_end();
        if line.trim().is_empty() {
            if !block.is_empty() {
                blocks.push(block.join("\n"));
                block.clear();
            }
            continue;
        }
        block.push(line);
    }
    if !block.is_empty() {
        blocks.push(block.join("\n"));
    }
    blocks.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one page PDF holding the given content stream, built by hand.
    ///
    /// In code rather than checked in as a binary, for the same reason
    /// `crate::documents`' tests build their zips in code: a fixture you can
    /// read is a fixture you can tell is wrong.
    fn pdf(content: &str) -> Vec<u8> {
        let objects = [
            "<</Type/Catalog/Pages 2 0 R>>".to_string(),
            "<</Type/Pages/Kids[3 0 R]/Count 1>>".to_string(),
            "<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]\
             /Resources<</Font<</F1 4 0 R>>>>/Contents 5 0 R>>"
                .to_string(),
            "<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>".to_string(),
            format!(
                "<</Length {}>>\nstream\n{content}\nendstream",
                content.len() + 1
            ),
        ];

        let mut out = Vec::from(&b"%PDF-1.4\n"[..]);
        let mut offsets = Vec::new();
        for (index, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", index + 1).as_bytes());
        }

        // Every cross reference entry is exactly twenty bytes, which is the
        // one part of this a reader is entitled to insist on.
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        out.extend_from_slice(b"0000000000 65535 f \n");
        for offset in &offsets {
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<</Size {}/Root 1 0 R>>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        out
    }

    /// Two lines of a page turn into two readable blocks. This is the whole
    /// point of the module and the case the pane was showing a broken
    /// document icon for.
    #[test]
    fn a_pdf_becomes_the_text_it_holds() {
        let bytes = pdf("BT /F1 18 Tf 72 720 Td (Findings) Tj ET\n\
             BT /F1 12 Tf 72 700 Td (Latency fell by 40 percent.) Tj ET");
        let md = to_markdown(&bytes).unwrap();

        assert!(md.contains("Findings"), "{md:?}");
        assert!(md.contains("Latency fell by 40 percent."), "{md:?}");
        assert!(
            !md.contains("%PDF"),
            "the file itself came through instead of its text: {md:?}"
        );
    }

    /// A scanned PDF holds pictures of words and no words, so there is
    /// nothing to extract. An empty pane and a page with nothing readable on
    /// it look identical and are not the same thing, so the reader says which
    /// one this is.
    #[test]
    fn a_pdf_with_no_text_in_it_says_so_rather_than_rendering_blank() {
        let md = to_markdown(&pdf("BT ET")).unwrap();
        assert!(
            md.contains("no text this reader can extract"),
            "a textless PDF rendered as nothing: {md:?}"
        );
    }

    /// A model wrote this file, or downloaded it, so "it is not really a PDF"
    /// is an ordinary case and gets a sentence.
    #[test]
    fn something_that_is_not_a_pdf_is_a_readable_refusal() {
        let error = to_markdown(b"this is just text").unwrap_err();
        assert!(error.to_string().contains("not a readable PDF"), "{error}");
    }

    /// Extracted text arrives padded to the page width and broken by form
    /// feeds, and both would reach the renderer as structure that is not in
    /// the document: trailing runs of spaces, and page gaps wide enough to
    /// look deliberate. Neither is content, so neither survives.
    #[test]
    fn page_padding_does_not_reach_the_renderer_as_structure() {
        let raw = "\n\n\nFindings          \n\n\n\n\nLatency fell.   \n\u{c}\n\n\nNext page.\n\n\n";
        let clean = tidy(raw);

        assert_eq!(clean, "Findings\n\nLatency fell.\n\nNext page.");
    }

    /// A PDF with only whitespace in it has no text in it either. The check
    /// is on what is left after tidying, not on whether the reader returned
    /// a string.
    #[test]
    fn a_pdf_holding_only_whitespace_counts_as_holding_no_text() {
        let md = to_markdown(&pdf("BT /F1 12 Tf 72 720 Td (   ) Tj ET")).unwrap();
        assert!(
            md.contains("no text this reader can extract"),
            "whitespace was served as if it were a document: {md:?}"
        );
    }
}
