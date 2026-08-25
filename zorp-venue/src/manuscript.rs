//! A draft, parsed far enough to check it.
//!
//! The input is the markdown a zorp draft is written in: optional YAML
//! front matter, then ATX headings and prose. This is deliberately not a
//! markdown implementation. It records where things are, because a
//! conformance report that says "your name is in here somewhere" is not
//! worth reading. Every item carries the line it was found on.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The front matter of a draft: what the author declared about the paper
/// rather than wrote in it.
#[derive(Clone, Debug, Default)]
pub struct FrontMatter {
    pub title: Option<Located<String>>,
    pub authors: Vec<Located<String>>,
    pub affiliations: Vec<Located<String>>,
    pub abstract_text: Option<Located<String>>,
    /// Every scalar key seen, so a check can look for a field this struct
    /// does not name.
    pub keys: BTreeMap<String, Located<String>>,
    /// Lines the front matter block occupies, 1-based and inclusive. Empty
    /// when there is none.
    pub span: Option<(usize, usize)>,
}

/// A value and the 1-based line it was found on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Located<T> {
    pub value: T,
    pub line: usize,
}

impl<T> Located<T> {
    pub fn new(value: T, line: usize) -> Self {
        Located { value, line }
    }
}

/// One `#`-headed section of the body.
#[derive(Clone, Debug)]
pub struct Section {
    pub level: usize,
    pub title: String,
    pub line: usize,
    /// Body text under this heading, up to the next heading of any level.
    pub body: String,
}

impl Section {
    /// True when the heading contains `fragment`, case-insensitively.
    pub fn heading_matches(&self, fragment: &str) -> bool {
        self.title
            .to_lowercase()
            .contains(&fragment.to_lowercase())
    }
}

/// A figure, from a markdown image.
#[derive(Clone, Debug)]
pub struct Figure {
    pub caption: String,
    pub target: String,
    pub line: usize,
}

/// A parsed draft.
#[derive(Clone, Debug)]
pub struct Manuscript {
    pub path: Option<PathBuf>,
    pub front_matter: FrontMatter,
    pub sections: Vec<Section>,
    pub figures: Vec<Figure>,
    /// Every line of the source, 0-indexed. Line numbers reported are
    /// index + 1.
    pub lines: Vec<String>,
    /// First body line, 1-based: the line after the front matter.
    pub body_start: usize,
}

impl Manuscript {
    pub fn from_file(path: &Path) -> std::io::Result<Manuscript> {
        let text = std::fs::read_to_string(path)?;
        let mut m = Manuscript::parse(&text);
        m.path = Some(path.to_path_buf());
        Ok(m)
    }

    pub fn parse(text: &str) -> Manuscript {
        let lines: Vec<String> = text.lines().map(str::to_string).collect();
        let (front_matter, body_start) = parse_front_matter(&lines);
        let sections = parse_sections(&lines, body_start);
        let figures = parse_figures(&lines, body_start);
        Manuscript {
            path: None,
            front_matter,
            sections,
            figures,
            lines,
            body_start,
        }
    }

    /// The body, front matter excluded.
    pub fn body(&self) -> String {
        self.lines
            .iter()
            .skip(self.body_start.saturating_sub(1))
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Every body line with its 1-based number, front matter excluded.
    pub fn body_lines(&self) -> impl Iterator<Item = (usize, &str)> {
        self.lines
            .iter()
            .enumerate()
            .skip(self.body_start.saturating_sub(1))
            .map(|(i, l)| (i + 1, l.as_str()))
    }

    /// Words in the body, counting only sections whose heading does not
    /// contain any of `exclude`. Prose before the first heading always
    /// counts.
    pub fn counted_words(&self, exclude: &[String]) -> usize {
        if self.sections.is_empty() {
            return word_count(&self.body());
        }
        let mut total = 0;
        // Anything above the first heading is main text too.
        let first = self.sections[0].line;
        for (line_no, line) in self.body_lines() {
            if line_no >= first {
                break;
            }
            total += word_count(line);
        }
        for section in &self.sections {
            if exclude.iter().any(|e| section.heading_matches(e)) {
                continue;
            }
            total += word_count(&section.title) + word_count(&section.body);
        }
        total
    }

    /// Sections whose headings match any of `exclude`, by title.
    pub fn excluded_sections(&self, exclude: &[String]) -> Vec<&Section> {
        self.sections
            .iter()
            .filter(|s| exclude.iter().any(|e| s.heading_matches(e)))
            .collect()
    }

    /// Markdown pipe tables, counted by their header separator row.
    pub fn table_count(&self) -> usize {
        self.body_lines()
            .filter(|(_, l)| is_table_separator(l))
            .count()
    }

    /// The first section whose heading matches any fragment in `any_of`.
    pub fn find_section(&self, any_of: &[String]) -> Option<&Section> {
        self.sections
            .iter()
            .find(|s| any_of.iter().any(|f| s.heading_matches(f)))
    }

    /// Every pandoc-style citation key (`[@key]`, `[@a; @b]`) with its line.
    pub fn citation_keys(&self) -> Vec<Located<String>> {
        let mut out = Vec::new();
        for (line_no, line) in self.body_lines() {
            let bytes: Vec<char> = line.chars().collect();
            let mut i = 0;
            while i < bytes.len() {
                if bytes[i] == '@' && (i == 0 || bytes[i - 1] != '\\') {
                    let mut j = i + 1;
                    while j < bytes.len() && is_cite_key_char(bytes[j]) {
                        j += 1;
                    }
                    if j > i + 1 {
                        out.push(Located::new(bytes[i + 1..j].iter().collect(), line_no));
                    }
                    i = j;
                } else {
                    i += 1;
                }
            }
        }
        out
    }

    /// Every URL in the document, front matter included, with its line.
    /// Trailing markdown punctuation is trimmed so the host and path are
    /// clean enough to match against.
    pub fn urls(&self) -> Vec<Located<String>> {
        let mut out = Vec::new();
        for (i, line) in self.lines.iter().enumerate() {
            let mut rest = line.as_str();
            while let Some(at) = rest.find("http") {
                let tail = &rest[at..];
                if !(tail.starts_with("http://") || tail.starts_with("https://")) {
                    rest = &rest[at + 4..];
                    continue;
                }
                let end = tail
                    .find(|c: char| c.is_whitespace() || matches!(c, ')' | ']' | '>' | '"' | '\''))
                    .unwrap_or(tail.len());
                let url = tail[..end].trim_end_matches(['.', ',', ';', ':']);
                if !url.is_empty() {
                    out.push(Located::new(url.to_string(), i + 1));
                }
                rest = &tail[end.max(1)..];
            }
        }
        out
    }
}

fn is_cite_key_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ':' | '.' | '+' | '/')
}

fn is_table_separator(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('|')
        && t.len() > 2
        && t.chars()
            .all(|c| matches!(c, '|' | '-' | ':' | ' ' | '+' | '='))
        && t.contains('-')
}

pub fn word_count(text: &str) -> usize {
    text.split_whitespace().filter(|w| !w.is_empty()).count()
}

/// Parse a YAML front matter block. This handles the shapes a zorp draft
/// actually uses (plain scalars, `|` block scalars, `-` lists) and nothing
/// else; anything unrecognised is left in `keys` as raw text rather than
/// guessed at.
fn parse_front_matter(lines: &[String]) -> (FrontMatter, usize) {
    let mut fm = FrontMatter::default();
    if lines.first().map(|l| l.trim()) != Some("---") {
        return (fm, 1);
    }
    let Some(end) = lines
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, l)| l.trim() == "---" || l.trim() == "...")
        .map(|(i, _)| i)
    else {
        return (fm, 1);
    };
    fm.span = Some((1, end + 1));

    let mut i = 1;
    while i < end {
        let line = &lines[i];
        let line_no = i + 1;
        let Some((key, rest)) = split_key(line) else {
            i += 1;
            continue;
        };
        let rest = rest.trim();
        if rest == "|" || rest == ">" || rest == "|-" || rest == ">-" {
            let mut block = Vec::new();
            let mut j = i + 1;
            while j < end && (lines[j].trim().is_empty() || lines[j].starts_with(' ')) {
                block.push(lines[j].trim().to_string());
                j += 1;
            }
            let joined = block.join(" ").split_whitespace().collect::<Vec<_>>().join(" ");
            record(&mut fm, &key, Located::new(joined, line_no));
            i = j;
            continue;
        }
        if rest.is_empty() {
            let mut j = i + 1;
            let mut any = false;
            while j < end {
                let item = lines[j].trim();
                if let Some(v) = item.strip_prefix("- ") {
                    record(&mut fm, &key, Located::new(unquote(v), j + 1));
                    any = true;
                    j += 1;
                } else if item.is_empty() {
                    j += 1;
                } else {
                    break;
                }
            }
            if any {
                i = j;
                continue;
            }
        }
        record(&mut fm, &key, Located::new(unquote(rest), line_no));
        i += 1;
    }
    (fm, end + 2)
}

fn split_key(line: &str) -> Option<(String, &str)> {
    // Only top-level keys: an indented line belongs to the value above it.
    if line.starts_with(char::is_whitespace) {
        return None;
    }
    let colon = line.find(':')?;
    let key = line[..colon].trim();
    if key.is_empty() || !key.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
        return None;
    }
    Some((key.to_lowercase(), &line[colon + 1..]))
}

fn record(fm: &mut FrontMatter, key: &str, value: Located<String>) {
    match key {
        "title" => fm.title = Some(value.clone()),
        "author" | "authors" => fm.authors.push(value.clone()),
        "affiliation" | "affiliations" | "institute" | "institution" => {
            fm.affiliations.push(value.clone())
        }
        "abstract" | "summary" => fm.abstract_text = Some(value.clone()),
        _ => {}
    }
    fm.keys.entry(key.to_string()).or_insert(value);
}

fn unquote(text: &str) -> String {
    let t = text.trim();
    let t = t.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(t);
    let t = t.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')).unwrap_or(t);
    t.to_string()
}

fn parse_sections(lines: &[String], body_start: usize) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    let mut fenced = false;
    for (i, line) in lines.iter().enumerate() {
        let line_no = i + 1;
        if line_no < body_start {
            continue;
        }
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
        }
        if !fenced && trimmed.starts_with('#') {
            let level = trimmed.chars().take_while(|c| *c == '#').count();
            let title = trimmed[level..].trim().trim_end_matches('#').trim();
            if level <= 6 && !title.is_empty() {
                sections.push(Section {
                    level,
                    title: title.to_string(),
                    line: line_no,
                    body: String::new(),
                });
                continue;
            }
        }
        if let Some(current) = sections.last_mut() {
            current.body.push_str(line);
            current.body.push('\n');
        }
    }
    sections
}

fn parse_figures(lines: &[String], body_start: usize) -> Vec<Figure> {
    // A pandoc figure caption can wrap over several lines, so images are
    // found on the joined body rather than line by line, and the line
    // number is recovered from where the `![` opened.
    let mut figures = Vec::new();
    let mut buffer = String::new();
    let mut starts: Vec<usize> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if i + 1 < body_start {
            continue;
        }
        for _ in 0..=line.len() {
            starts.push(i + 1);
        }
        buffer.push_str(line);
        buffer.push('\n');
    }
    let chars: Vec<char> = buffer.chars().collect();
    let mut byte_of_char = Vec::with_capacity(chars.len() + 1);
    let mut acc = 0usize;
    for c in &chars {
        byte_of_char.push(acc);
        acc += c.len_utf8();
    }
    byte_of_char.push(acc);

    let mut i = 0;
    while i + 1 < chars.len() {
        if chars[i] == '!' && chars[i + 1] == '[' {
            if let Some((caption, target, next)) = read_image(&chars, i + 1) {
                let byte = byte_of_char[i];
                let line = starts.get(byte).copied().unwrap_or(body_start);
                figures.push(Figure {
                    caption: caption.split_whitespace().collect::<Vec<_>>().join(" "),
                    target,
                    line,
                });
                i = next;
                continue;
            }
        }
        i += 1;
    }
    figures
}

/// Read `[caption](target)` starting at the `[`. Returns the caption, the
/// target, and the index just past the closing paren.
fn read_image(chars: &[char], open: usize) -> Option<(String, String, usize)> {
    let mut depth = 0;
    let mut i = open;
    let mut close = None;
    while i < chars.len() {
        match chars[i] {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(i);
                    break;
                }
            }
            _ => {}
        }
        i += 1;
    }
    let close = close?;
    let caption: String = chars[open + 1..close].iter().collect();
    let mut j = close + 1;
    if chars.get(j) != Some(&'(') {
        return None;
    }
    j += 1;
    let start = j;
    while j < chars.len() && chars[j] != ')' {
        j += 1;
    }
    if j >= chars.len() {
        return None;
    }
    let target: String = chars[start..j].iter().collect();
    Some((caption, target.trim().to_string(), j + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DRAFT: &str = r#"---
title: "A Study of Things"
author: "Ada Lovelace"
abstract: |
  We did a thing.
  It worked.
---

Some opening prose.

# Introduction

Body of the introduction [@smith2020].

## A subsection

More text.

![A diagram of the
system.](figures/arch.png){width=100%}

| a | b |
|---|---|
| 1 | 2 |

# References

A reference list nobody counts.
"#;

    #[test]
    fn reads_front_matter_including_a_block_abstract() {
        let m = Manuscript::parse(DRAFT);
        assert_eq!(
            m.front_matter.title.as_ref().unwrap().value,
            "A Study of Things"
        );
        assert_eq!(m.front_matter.authors[0].value, "Ada Lovelace");
        assert_eq!(m.front_matter.authors[0].line, 3);
        assert_eq!(
            m.front_matter.abstract_text.as_ref().unwrap().value,
            "We did a thing. It worked."
        );
        assert_eq!(m.front_matter.span, Some((1, 7)));
        assert_eq!(m.body_start, 8);
    }

    #[test]
    fn a_draft_without_front_matter_is_all_body() {
        let m = Manuscript::parse("# Title\n\ntext\n");
        assert_eq!(m.body_start, 1);
        assert!(m.front_matter.title.is_none());
        assert_eq!(m.sections.len(), 1);
    }

    #[test]
    fn reads_headings_with_their_levels_and_lines() {
        let m = Manuscript::parse(DRAFT);
        let titles: Vec<&str> = m.sections.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(
            titles,
            vec!["Introduction", "A subsection", "References"],
            "should read every heading in order"
        );
        assert_eq!(m.sections[1].level, 2);
        assert_eq!(m.sections[0].line, 10);
    }

    #[test]
    fn a_hash_inside_a_fenced_block_is_not_a_heading() {
        let m = Manuscript::parse("# Real\n\n```sh\n# not a heading\n```\n\n# Also real\n");
        let titles: Vec<&str> = m.sections.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(titles, vec!["Real", "Also real"]);
    }

    #[test]
    fn counts_words_and_can_exclude_a_named_section() {
        let m = Manuscript::parse(DRAFT);
        let all = m.counted_words(&[]);
        let without_refs = m.counted_words(&["references".to_string()]);
        assert!(
            without_refs < all,
            "excluding references should lower the count: {without_refs} vs {all}"
        );
        // "References" heading plus its five words of body.
        assert_eq!(all - without_refs, 6);
    }

    #[test]
    fn reads_a_figure_whose_caption_wraps_over_lines() {
        let m = Manuscript::parse(DRAFT);
        assert_eq!(m.figures.len(), 1);
        assert_eq!(m.figures[0].caption, "A diagram of the system.");
        assert_eq!(m.figures[0].target, "figures/arch.png");
        assert_eq!(m.figures[0].line, 21);
    }

    #[test]
    fn counts_pipe_tables_by_their_separator_row() {
        let m = Manuscript::parse(DRAFT);
        assert_eq!(m.table_count(), 1);
    }

    #[test]
    fn reads_citation_keys_out_of_the_body_only() {
        let m = Manuscript::parse(DRAFT);
        let keys: Vec<&str> = m.citation_keys().iter().map(|k| k.value.as_str()).collect();
        assert_eq!(keys, vec!["smith2020"]);
    }

    #[test]
    fn reads_urls_and_trims_markdown_punctuation() {
        let m = Manuscript::parse(
            "See [the repo](https://github.com/acme/thing) and <https://example.test/a.html>.\n",
        );
        let urls: Vec<&str> = m.urls().iter().map(|u| u.value.as_str()).collect();
        assert_eq!(
            urls,
            vec![
                "https://github.com/acme/thing",
                "https://example.test/a.html"
            ]
        );
    }
}
