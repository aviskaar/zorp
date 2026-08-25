//! The tests that matter most. A paper with a reference nobody gathered
//! is exactly the failure zorp exists to prevent, so `Paper::assemble`
//! has to refuse one, and every reference that reaches the page has to
//! have come from the caller's evidence list rather than from prose.

use zorp_paper::{markdown, Block, Paper, PaperError, PaperParts, Reference, Section};

fn reference(key: &str, claim: &str, source: &str) -> Reference {
    Reference {
        key: key.to_string(),
        claim: claim.to_string(),
        source: source.to_string(),
    }
}

fn parts_with(body: &str, references: Vec<Reference>) -> PaperParts {
    PaperParts {
        title: "Does caching help".to_string(),
        authors: vec![],
        date: "August 2026".to_string(),
        provenance: vec!["Track: does-caching-help".to_string()],
        abstract_text: "A short abstract.".to_string(),
        sections: vec![Section {
            title: "Findings".to_string(),
            level: 1,
            blocks: vec![Block::Paragraph(body.to_string())],
        }],
        references,
    }
}

#[test]
fn a_citation_with_no_matching_reference_is_refused() {
    let parts = parts_with(
        "Latency fell by half [E9].",
        vec![reference(
            "E1",
            "latency_ms = 42",
            "investigate, experiment x",
        )],
    );

    let err = Paper::assemble(parts).unwrap_err();

    assert_eq!(err, PaperError::UnknownCitations(vec!["E9".to_string()]));
}

#[test]
fn the_error_names_every_unknown_citation_once() {
    let parts = parts_with(
        "Latency fell [E9], throughput rose [E4], and [E9] again.",
        vec![reference(
            "E1",
            "latency_ms = 42",
            "investigate, experiment x",
        )],
    );

    let err = Paper::assemble(parts).unwrap_err();

    assert_eq!(
        err,
        PaperError::UnknownCitations(vec!["E9".to_string(), "E4".to_string()])
    );
}

#[test]
fn a_citation_that_matches_a_reference_is_accepted() {
    let parts = parts_with(
        "Latency fell by half [E1].",
        vec![reference(
            "E1",
            "latency_ms = 42",
            "investigate, experiment x",
        )],
    );

    let paper = Paper::assemble(parts).expect("E1 is in the reference list");

    assert_eq!(paper.parts().references.len(), 1);
}

#[test]
fn a_numeric_citation_resolves_by_position_and_out_of_range_is_refused() {
    let ok = parts_with(
        "Latency fell by half [1].",
        vec![reference(
            "E1",
            "latency_ms = 42",
            "investigate, experiment x",
        )],
    );
    Paper::assemble(ok).expect("[1] is the first reference");

    let bad = parts_with(
        "Latency fell by half [2].",
        vec![reference(
            "E1",
            "latency_ms = 42",
            "investigate, experiment x",
        )],
    );
    let err = Paper::assemble(bad).unwrap_err();
    assert_eq!(err, PaperError::UnknownCitations(vec!["2".to_string()]));
}

#[test]
fn citations_in_the_abstract_and_in_headings_are_checked_too() {
    let mut parts = parts_with("No citation here.", vec![]);
    parts.abstract_text = "We show a speedup [E1].".to_string();
    let err = Paper::assemble(parts).unwrap_err();
    assert_eq!(err, PaperError::UnknownCitations(vec!["E1".to_string()]));

    let mut parts = parts_with("No citation here.", vec![]);
    parts.sections[0].title = "Findings [E2]".to_string();
    let err = Paper::assemble(parts).unwrap_err();
    assert_eq!(err, PaperError::UnknownCitations(vec!["E2".to_string()]));
}

#[test]
fn a_bullet_is_prose_and_gets_checked() {
    let mut parts = parts_with("No citation here.", vec![]);
    parts.sections[0]
        .blocks
        .push(Block::Bullet("cache hits rose [E3]".to_string()));

    let err = Paper::assemble(parts).unwrap_err();

    assert_eq!(err, PaperError::UnknownCitations(vec!["E3".to_string()]));
}

#[test]
fn a_code_block_is_not_prose_and_is_not_scanned() {
    let mut parts = parts_with("No citation here.", vec![]);
    parts.sections[0]
        .blocks
        // An array literal, whose bracket does sit at a word boundary, so
        // only the code exclusion keeps this from reading as a citation.
        .push(Block::Code("let samples = [1, 2, 3];".to_string()));

    Paper::assemble(parts).expect("an array literal in code is not a citation");
}

#[test]
fn an_index_expression_in_prose_is_not_a_citation() {
    let parts = parts_with("The run reads samples[0] before it starts.", vec![]);

    Paper::assemble(parts).expect("samples[0] is an index expression, not a citation");
}

#[test]
fn prose_in_brackets_is_not_a_citation() {
    let parts = parts_with(
        "The result [see the appendix] held across runs [Smith 2020].",
        vec![],
    );

    Paper::assemble(parts).expect("bracketed prose is not a citation marker");
}

#[test]
fn two_references_cannot_share_a_key() {
    let parts = parts_with(
        "No citation here.",
        vec![
            reference("E1", "latency_ms = 42", "investigate, experiment x"),
            reference("E1", "latency_ms = 43", "investigate, experiment y"),
        ],
    );

    let err = Paper::assemble(parts).unwrap_err();

    assert_eq!(err, PaperError::DuplicateReferenceKey("E1".to_string()));
}

#[test]
fn every_rendered_reference_came_from_the_reference_list() {
    let parts = parts_with(
        "Latency fell by half [E1].",
        vec![reference(
            "E1",
            "latency_ms = 42",
            "investigate, experiment exp-7",
        )],
    );
    let paper = Paper::assemble(parts).unwrap();

    let rendered = markdown::render(&paper);
    let references_section = rendered
        .split_once("# References")
        .expect("a paper has a references section")
        .1;

    assert!(references_section.contains("latency_ms = 42"));
    assert!(references_section.contains("investigate, experiment exp-7"));
    // Nothing else may appear there. One reference in, one reference out.
    let entries = references_section
        .lines()
        .filter(|l| l.trim_start().starts_with("1.") || l.trim_start().starts_with("2."))
        .count();
    assert_eq!(entries, 1, "rendered:\n{rendered}");
}
