//! Rendering a paper to markdown. This is the source-of-truth artifact:
//! the PDF is a rendering of what this produces, not the other way round.
//!
//! The front matter is the pandoc shape the repository's own paper uses,
//! so the same file can be handed to a LaTeX toolchain by anyone who
//! wants one. The body stays readable without any toolchain at all.

use crate::{Block, Paper};
use std::fmt::Write as _;

/// Escape a value going into a double-quoted YAML scalar. Only two
/// characters can end the scalar early, and both of them get a
/// backslash.
fn yaml_scalar(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        if c == '\\' || c == '"' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

fn heading_hashes(level: u8) -> &'static str {
    match level {
        0 | 1 => "#",
        2 => "##",
        _ => "###",
    }
}

pub fn render(paper: &Paper) -> String {
    let parts = paper.parts();
    let mut out = String::new();

    out.push_str("---\n");
    let _ = writeln!(out, "title: {}", yaml_scalar(&parts.title));
    if !parts.authors.is_empty() {
        let _ = writeln!(out, "author: {}", yaml_scalar(&parts.authors.join(", ")));
    }
    if !parts.date.is_empty() {
        let _ = writeln!(out, "date: {}", yaml_scalar(&parts.date));
    }
    out.push_str("---\n\n");

    for line in &parts.provenance {
        let _ = writeln!(out, "*{line}*  ");
    }
    if !parts.provenance.is_empty() {
        out.push('\n');
    }

    if !parts.abstract_text.trim().is_empty() {
        out.push_str("# Abstract\n\n");
        out.push_str(parts.abstract_text.trim());
        out.push_str("\n\n");
    }

    for section in &parts.sections {
        if !section.title.trim().is_empty() {
            let _ = writeln!(
                out,
                "{} {}\n",
                heading_hashes(section.level),
                section.title.trim()
            );
        }
        for block in &section.blocks {
            match block {
                Block::Paragraph(text) => {
                    out.push_str(text.trim_end());
                    out.push_str("\n\n");
                }
                Block::Bullet(text) => {
                    let _ = writeln!(out, "- {}", text.trim());
                }
                Block::Code(text) => {
                    out.push_str("```\n");
                    out.push_str(text.trim_end());
                    out.push_str("\n```\n\n");
                }
            }
        }
        // A run of bullets ends without a blank line of its own, so add
        // one before whatever comes next.
        if matches!(section.blocks.last(), Some(Block::Bullet(_))) {
            out.push('\n');
        }
    }

    out.push_str("# References\n\n");
    if parts.references.is_empty() {
        out.push_str(
            "The evidence record for this track is empty, so this paper has no references.\n",
        );
    } else {
        for (i, reference) in parts.references.iter().enumerate() {
            let _ = writeln!(
                out,
                "{}. [{}] {} ({})",
                i + 1,
                reference.key,
                reference.claim.trim(),
                reference.source.trim()
            );
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PaperParts, Reference, Section};

    fn paper_with(parts: PaperParts) -> Paper {
        Paper::assemble(parts).expect("test parts should assemble")
    }

    fn base() -> PaperParts {
        PaperParts {
            title: "Does caching help".to_string(),
            date: "August 2026".to_string(),
            abstract_text: "Short.".to_string(),
            ..PaperParts::default()
        }
    }

    #[test]
    fn front_matter_carries_the_title_and_date() {
        let out = render(&paper_with(base()));
        assert!(out.starts_with("---\n"), "{out}");
        assert!(out.contains("title: \"Does caching help\""), "{out}");
        assert!(out.contains("date: \"August 2026\""), "{out}");
    }

    #[test]
    fn a_quote_in_the_title_cannot_end_the_yaml_scalar() {
        let mut parts = base();
        parts.title = "The \"cache\" question".to_string();
        let out = render(&paper_with(parts));
        assert!(out.contains(r#"title: "The \"cache\" question""#), "{out}");
    }

    #[test]
    fn no_author_line_when_there_are_no_authors() {
        let out = render(&paper_with(base()));
        assert!(!out.contains("author:"), "{out}");
    }

    #[test]
    fn an_empty_reference_list_says_so_rather_than_inventing_one() {
        let out = render(&paper_with(base()));
        let refs = out.split_once("# References").unwrap().1;
        assert!(
            refs.contains("evidence record for this track is empty"),
            "{out}"
        );
    }

    #[test]
    fn a_section_renders_at_its_level() {
        let mut parts = base();
        parts.sections = vec![
            Section {
                title: "Method".into(),
                level: 1,
                blocks: vec![Block::Paragraph("We ran it.".into())],
            },
            Section {
                title: "Detail".into(),
                level: 2,
                blocks: vec![Block::Bullet("one".into()), Block::Bullet("two".into())],
            },
        ];
        let out = render(&paper_with(parts));
        assert!(out.contains("\n# Method\n"), "{out}");
        assert!(out.contains("\n## Detail\n"), "{out}");
        assert!(out.contains("\n- one\n- two\n"), "{out}");
    }

    #[test]
    fn a_level_past_three_is_clamped_rather_than_refused() {
        let mut parts = base();
        parts.sections = vec![Section {
            title: "Deep".into(),
            level: 9,
            blocks: vec![],
        }];
        let out = render(&paper_with(parts));
        assert!(out.contains("\n### Deep\n"), "{out}");
    }

    #[test]
    fn rendering_is_the_same_every_time() {
        let mut parts = base();
        parts.references = vec![Reference {
            key: "E1".into(),
            claim: "latency_ms = 42".into(),
            source: "investigate, experiment exp-7".into(),
        }];
        let paper = paper_with(parts);
        assert_eq!(render(&paper), render(&paper));
    }
}
