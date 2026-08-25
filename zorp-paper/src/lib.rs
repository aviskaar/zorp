//! Paper-shaped documents, rendered to markdown and to PDF.
//!
//! The one invariant this crate exists to hold: a `Paper` cannot be
//! built whose prose cites a reference the reference list does not have.
//! `Paper::assemble` is the only constructor and it refuses. Callers
//! build the reference list from an evidence record, so a reference that
//! is in the document is a reference that was in the record, and a
//! citation that resolves is a citation to one of those.
//!
//! Rendering is deterministic. Nothing in here reads the clock, the
//! environment, or the filesystem. A date, if the document shows one, is
//! passed in.

pub mod citation;
pub mod date;
pub mod markdown;
pub mod pdf;

/// One entry in the reference list, carrying where it came from.
///
/// `key` is the short handle prose cites (`E1`). `claim` is what the
/// record actually says. `source` is the provenance line, which is the
/// part that makes the reference checkable by a human.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    pub key: String,
    pub claim: String,
    pub source: String,
}

/// A run of body content inside a section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Paragraph(String),
    Bullet(String),
    /// A verbatim block, rendered in a fixed-width font and never
    /// scanned for citations.
    Code(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub title: String,
    /// 1, 2 or 3. Anything larger is clamped at render time rather than
    /// rejected: a too-deep heading is a formatting problem, not a
    /// reason to refuse to produce the document.
    pub level: u8,
    pub blocks: Vec<Block>,
}

/// Everything a paper is made of, before the citation check.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PaperParts {
    pub title: String,
    pub authors: Vec<String>,
    /// Rendered as written. This crate never formats a date itself; see
    /// `date` for the helper that turns epoch milliseconds into one.
    pub date: String,
    /// Free-form provenance shown under the byline, for instance the
    /// track id the document was built from.
    pub provenance: Vec<String>,
    pub abstract_text: String,
    pub sections: Vec<Section>,
    pub references: Vec<Reference>,
}

/// A document whose citations have been checked against its references.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paper {
    parts: PaperParts,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaperError {
    /// Prose cites something the reference list does not have. This is
    /// the invented-citation case, and it is fatal on purpose.
    UnknownCitations(Vec<String>),
    DuplicateReferenceKey(String),
    EmptyTitle,
}

impl std::fmt::Display for PaperError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PaperError::UnknownCitations(keys) => write!(
                f,
                "the draft cites {} that the evidence record does not contain: {}",
                if keys.len() == 1 {
                    "a reference"
                } else {
                    "references"
                },
                keys.join(", ")
            ),
            PaperError::DuplicateReferenceKey(key) => {
                write!(f, "two references share the key {key}")
            }
            PaperError::EmptyTitle => write!(f, "a paper needs a title"),
        }
    }
}

impl std::error::Error for PaperError {}

impl Paper {
    /// The only way to build a `Paper`. Refuses a document whose prose
    /// cites a reference that is not in `parts.references`.
    pub fn assemble(parts: PaperParts) -> Result<Paper, PaperError> {
        if parts.title.trim().is_empty() {
            return Err(PaperError::EmptyTitle);
        }
        let mut seen: Vec<&str> = Vec::with_capacity(parts.references.len());
        for reference in &parts.references {
            if seen.contains(&reference.key.as_str()) {
                return Err(PaperError::DuplicateReferenceKey(reference.key.clone()));
            }
            seen.push(&reference.key);
        }

        let mut unknown: Vec<String> = Vec::new();
        for text in prose(&parts) {
            for marker in citation::markers(text) {
                if resolves(&marker, &parts.references) {
                    continue;
                }
                let written = marker.as_written();
                if !unknown.contains(&written) {
                    unknown.push(written);
                }
            }
        }
        if !unknown.is_empty() {
            return Err(PaperError::UnknownCitations(unknown));
        }

        Ok(Paper { parts })
    }

    pub fn parts(&self) -> &PaperParts {
        &self.parts
    }
}

/// Every span of the document a human would read as prose, in reading
/// order. Code blocks are not in here: a bracket in code is a language
/// construct, not a citation.
fn prose(parts: &PaperParts) -> Vec<&str> {
    let mut out = vec![parts.title.as_str(), parts.abstract_text.as_str()];
    for section in &parts.sections {
        out.push(section.title.as_str());
        for block in &section.blocks {
            match block {
                Block::Paragraph(t) | Block::Bullet(t) => out.push(t.as_str()),
                Block::Code(_) => {}
            }
        }
    }
    out
}

fn resolves(marker: &citation::Marker, references: &[Reference]) -> bool {
    match marker {
        citation::Marker::Key(key) => references.iter().any(|r| &r.key == key),
        citation::Marker::Index(i) => *i >= 1 && *i <= references.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal() -> PaperParts {
        PaperParts {
            title: "A title".to_string(),
            ..PaperParts::default()
        }
    }

    #[test]
    fn a_paper_needs_a_title() {
        let mut parts = minimal();
        parts.title = "   ".to_string();
        assert_eq!(Paper::assemble(parts).unwrap_err(), PaperError::EmptyTitle);
    }

    #[test]
    fn unknown_citations_read_as_a_list_in_the_message() {
        let e = PaperError::UnknownCitations(vec!["E9".into(), "E4".into()]);
        let msg = e.to_string();
        assert!(msg.contains("E9"), "{msg}");
        assert!(msg.contains("E4"), "{msg}");
        assert!(msg.contains("references"), "{msg}");
    }

    #[test]
    fn one_unknown_citation_reads_as_singular() {
        let msg = PaperError::UnknownCitations(vec!["E9".into()]).to_string();
        assert!(msg.contains("a reference"), "{msg}");
    }
}
