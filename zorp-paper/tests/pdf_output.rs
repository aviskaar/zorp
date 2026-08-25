//! What the PDF writer has to get right. The content streams are not
//! compressed, on purpose, so a test can read the text back out of the
//! file and check what actually reached the page.

use zorp_paper::pdf::{self, PdfOptions};
use zorp_paper::{Block, Paper, PaperParts, Reference, Section};

/// Pull every literal string out of a PDF and undo the escaping, so a
/// test can assert on the words a reader would see. Doubles as a check
/// that the escaping is reversible.
fn extract_text(pdf: &[u8]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < pdf.len() {
        if pdf[i] != b'(' {
            i += 1;
            continue;
        }
        i += 1;
        let mut depth = 1usize;
        let mut bytes: Vec<u8> = Vec::new();
        while i < pdf.len() {
            match pdf[i] {
                b'\\' if i + 1 < pdf.len() => {
                    let next = pdf[i + 1];
                    if next.is_ascii_digit() && i + 3 < pdf.len() {
                        let octal = std::str::from_utf8(&pdf[i + 1..i + 4]).unwrap_or("077");
                        let byte = u8::from_str_radix(octal, 8).unwrap_or(b'?');
                        bytes.push(byte);
                        i += 4;
                    } else {
                        bytes.push(next);
                        i += 2;
                    }
                }
                b'(' => {
                    depth += 1;
                    bytes.push(b'(');
                    i += 1;
                }
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        i += 1;
                        break;
                    }
                    bytes.push(b')');
                    i += 1;
                }
                other => {
                    bytes.push(other);
                    i += 1;
                }
            }
        }
        out.push_str(&String::from_utf8_lossy(&bytes));
        out.push(' ');
    }
    out
}

fn reference(key: &str, claim: &str, source: &str) -> Reference {
    Reference {
        key: key.to_string(),
        claim: claim.to_string(),
        source: source.to_string(),
    }
}

fn sample() -> Paper {
    Paper::assemble(PaperParts {
        title: "Does caching help".to_string(),
        authors: vec!["A. Researcher".to_string()],
        date: "August 2026".to_string(),
        provenance: vec!["Track: does-caching-help".to_string()],
        abstract_text: "We measured a cached path against an uncached one.".to_string(),
        sections: vec![Section {
            title: "Findings".to_string(),
            level: 1,
            blocks: vec![
                Block::Paragraph("Latency fell by half [E1].".to_string()),
                Block::Bullet("the cache warmed in under a second".to_string()),
                Block::Code("cargo run --release".to_string()),
            ],
        }],
        references: vec![reference(
            "E1",
            "latency_ms = 42",
            "investigate, experiment exp-7",
        )],
    })
    .expect("sample assembles")
}

#[test]
fn the_file_is_a_pdf() {
    let bytes = pdf::render(&sample(), &PdfOptions::default());
    assert!(bytes.starts_with(b"%PDF-1."), "no header");
    assert!(bytes.ends_with(b"%%EOF\n"), "no trailer");
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("/Type /Catalog"));
    assert!(text.contains("/Type /Pages"));
    assert!(text.contains("/Type /Page /Parent"));
}

#[test]
fn startxref_points_at_the_cross_reference_table() {
    let bytes = pdf::render(&sample(), &PdfOptions::default());
    let text = String::from_utf8_lossy(&bytes);
    let tail = text.rsplit_once("startxref\n").expect("a startxref line").1;
    let offset: usize = tail
        .lines()
        .next()
        .expect("an offset")
        .trim()
        .parse()
        .expect("the offset is a number");
    assert!(
        bytes[offset..].starts_with(b"xref\n"),
        "startxref should point at the table, found {:?}",
        String::from_utf8_lossy(&bytes[offset..offset + 16])
    );
}

#[test]
fn the_same_paper_renders_the_same_bytes() {
    let paper = sample();
    let first = pdf::render(&paper, &PdfOptions::default());
    let second = pdf::render(&paper, &PdfOptions::default());
    assert_eq!(first, second);
}

#[test]
fn no_creation_date_unless_one_is_given() {
    let paper = sample();
    let plain = pdf::render(&paper, &PdfOptions::default());
    assert!(!String::from_utf8_lossy(&plain).contains("/CreationDate"));

    let dated = pdf::render(
        &paper,
        &PdfOptions {
            creation_date: Some("D:20260818123456Z".to_string()),
        },
    );
    assert!(String::from_utf8_lossy(&dated).contains("/CreationDate (D:20260818123456Z)"));
}

#[test]
fn the_title_the_abstract_and_the_body_all_reach_the_page() {
    let bytes = pdf::render(&sample(), &PdfOptions::default());
    let text = extract_text(&bytes);
    assert!(text.contains("Does caching help"), "{text}");
    assert!(text.contains("A. Researcher"), "{text}");
    assert!(text.contains("Track: does-caching-help"), "{text}");
    assert!(text.contains("Abstract"), "{text}");
    assert!(
        text.contains("cached path against an uncached one"),
        "{text}"
    );
    assert!(text.contains("Findings"), "{text}");
    assert!(text.contains("Latency fell by half [E1]"), "{text}");
    assert!(
        text.contains("the cache warmed in under a second"),
        "{text}"
    );
    assert!(text.contains("cargo run --release"), "{text}");
}

#[test]
fn every_reference_reaches_the_page_with_its_source() {
    let bytes = pdf::render(&sample(), &PdfOptions::default());
    let text = extract_text(&bytes);
    assert!(text.contains("References"), "{text}");
    assert!(text.contains("latency_ms = 42"), "{text}");
    assert!(text.contains("investigate, experiment exp-7"), "{text}");
}

#[test]
fn parentheses_and_backslashes_survive_the_escaping() {
    let mut parts = PaperParts {
        title: "A (parenthetical) title".to_string(),
        ..PaperParts::default()
    };
    parts.sections = vec![Section {
        title: "Body".to_string(),
        level: 1,
        blocks: vec![
            Block::Paragraph(r"a path C:\tmp\out and a (nested (group)) here".to_string()),
            // An unbalanced bracket is the case that actually breaks a
            // PDF literal, because balanced pairs are legal inside one.
            Block::Paragraph("a lone ) closer and a lone ( opener".to_string()),
        ],
    }];
    let paper = Paper::assemble(parts).unwrap();

    let bytes = pdf::render(&paper, &PdfOptions::default());
    let text = extract_text(&bytes);
    assert!(text.contains("A (parenthetical) title"), "{text}");
    assert!(text.contains(r"C:\tmp\out"), "{text}");
    assert!(text.contains("a (nested (group)) here"), "{text}");
    assert!(
        text.contains("a lone ) closer and a lone ( opener"),
        "{text}"
    );
}

#[test]
fn inline_emphasis_becomes_a_font_and_not_asterisks() {
    let mut parts = PaperParts {
        title: "Emphasis".to_string(),
        ..PaperParts::default()
    };
    parts.sections = vec![Section {
        title: "Body".to_string(),
        level: 1,
        blocks: vec![Block::Paragraph(
            "the **cached** path was *faster* than `uncached`".to_string(),
        )],
    }];
    let paper = Paper::assemble(parts).unwrap();

    let bytes = pdf::render(&paper, &PdfOptions::default());
    let text = extract_text(&bytes);
    assert!(!text.contains("**"), "asterisks reached the page: {text}");
    assert!(!text.contains('`'), "backticks reached the page: {text}");
    assert!(text.contains("cached"), "{text}");
    assert!(text.contains("faster"), "{text}");
    assert!(text.contains("uncached"), "{text}");
    // Every page lists all four fonts in its resources, so the presence
    // of a name proves nothing. What proves it is a `Tf` operator that
    // selects the face for a run of text.
    let selected = faces_selected(&String::from_utf8_lossy(&bytes));
    assert!(selected.contains(&"/F2".to_string()), "{selected:?}");
    assert!(selected.contains(&"/F3".to_string()), "{selected:?}");
    assert!(selected.contains(&"/F4".to_string()), "{selected:?}");
}

/// Every font resource name that a `Tf` operator actually selects.
fn faces_selected(pdf: &str) -> Vec<String> {
    let chunks: Vec<&str> = pdf.split(" Tf").collect();
    chunks[..chunks.len().saturating_sub(1)]
        .iter()
        .filter_map(|chunk| {
            let at = chunk.rfind("/F")?;
            Some(chunk[at..].chars().take(3).collect())
        })
        .collect()
}

#[test]
fn a_long_document_gets_more_than_one_page() {
    let mut parts = PaperParts {
        title: "A long one".to_string(),
        ..PaperParts::default()
    };
    let paragraph = "Every run of the harness recorded a latency figure, and the \
                     figures were consistent across the whole campaign, which is \
                     the only reason this paragraph exists at all."
        .to_string();
    parts.sections = vec![Section {
        title: "Body".to_string(),
        level: 1,
        blocks: (0..60)
            .map(|_| Block::Paragraph(paragraph.clone()))
            .collect(),
    }];
    let paper = Paper::assemble(parts).unwrap();

    let bytes = pdf::render(&paper, &PdfOptions::default());
    let raw = String::from_utf8_lossy(&bytes);
    let pages = raw.matches("/Type /Page /Parent").count();
    assert!(pages > 1, "expected several pages, got {pages}");
    assert!(raw.contains(&format!("/Count {pages}")), "page count wrong");
}

#[test]
fn a_word_wider_than_the_page_is_broken_rather_than_run_off_it() {
    let mut parts = PaperParts {
        title: "Long words".to_string(),
        ..PaperParts::default()
    };
    let monster = "x".repeat(400);
    parts.sections = vec![Section {
        title: "Body".to_string(),
        level: 1,
        blocks: vec![Block::Paragraph(monster.clone())],
    }];
    let paper = Paper::assemble(parts).unwrap();

    let bytes = pdf::render(&paper, &PdfOptions::default());
    let text = extract_text(&bytes);
    // Broken across lines, so the whole run is not one string, but every
    // character still made it.
    let xs = text.chars().filter(|c| *c == 'x').count();
    assert_eq!(xs, 400, "characters were dropped");
    assert!(!text.contains(&monster), "the line was never broken");
}

#[test]
fn a_character_outside_winansi_does_not_corrupt_the_file() {
    let mut parts = PaperParts {
        title: "Unicode".to_string(),
        ..PaperParts::default()
    };
    parts.sections = vec![Section {
        title: "Body".to_string(),
        level: 1,
        blocks: vec![Block::Paragraph(
            "latency \u{2014} 42 ms \u{4e2d}\u{6587}".to_string(),
        )],
    }];
    let paper = Paper::assemble(parts).unwrap();

    let bytes = pdf::render(&paper, &PdfOptions::default());
    assert!(bytes.ends_with(b"%%EOF\n"));
    // Every byte inside a literal string is either printable ASCII or an
    // escape, so no raw UTF-8 leaks into the file.
    assert!(bytes.iter().all(|b| *b < 0x80));
}
