//! Finding citation markers in prose.
//!
//! The grammar is deliberately narrow. A marker is a bracket group at a
//! word boundary whose contents are all evidence keys (`E1`, `REF12`) or
//! all bare numbers (`1`, `2`). Anything else inside brackets is prose
//! and is left alone, so `[see the appendix]` is not a citation and
//! neither is `samples[0]`, which is an index expression and has no
//! boundary before its bracket.
//!
//! Bare numbers count because that is how a model asked for a paper will
//! write citations, and a numeric citation nobody can resolve is exactly
//! the failure worth catching. They resolve against reference positions,
//! so `[1]` is the first reference and `[9]` with two references is an
//! error rather than a footnote.

/// A citation marker as it appeared in the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Marker {
    /// `[E1]`: cites a reference by key.
    Key(String),
    /// `[1]`: cites a reference by its position in the list, 1-based.
    Index(usize),
}

impl Marker {
    /// How the marker should be named back to a human when it does not
    /// resolve. This is the text they will find if they search the draft.
    pub fn as_written(&self) -> String {
        match self {
            Marker::Key(k) => k.clone(),
            Marker::Index(i) => i.to_string(),
        }
    }
}

/// A key is one to four letters followed by at least one digit. Four is
/// enough for the prefixes people actually use (E, R, S, REF) and short
/// enough that an ordinary bracketed word cannot pass for one.
fn is_key(token: &str) -> bool {
    let letters = token
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .count();
    if letters == 0 || letters > 4 {
        return false;
    }
    let digits = &token[letters..];
    !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())
}

/// A bracket only opens a citation at a word boundary. Without this rule
/// `samples[0]` reads as a citation to reference zero, which is both
/// wrong and impossible to satisfy.
fn opens_at_boundary(before: Option<char>) -> bool {
    match before {
        None => true,
        Some(c) => c.is_whitespace() || matches!(c, '(' | '[' | '{' | '"' | '\'' | '/'),
    }
}

/// Blank out inline code spans so their contents are never read as
/// prose. Backticks and the text between them become spaces, which keeps
/// byte offsets stable for anything scanning the result.
fn blank_code_spans(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_span = false;
    for c in text.chars() {
        if c == '`' {
            in_span = !in_span;
            out.push(' ');
        } else if in_span && c != '\n' {
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    out
}

/// Every citation marker in `text`, in order, with duplicates kept.
pub fn markers(text: &str) -> Vec<Marker> {
    let scanned = blank_code_spans(text);
    let chars: Vec<char> = scanned.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '[' {
            i += 1;
            continue;
        }
        if !opens_at_boundary(i.checked_sub(1).map(|p| chars[p])) {
            i += 1;
            continue;
        }
        // A marker never spans a line, and never nests.
        let mut j = i + 1;
        while j < chars.len() && chars[j] != ']' && chars[j] != '\n' && chars[j] != '[' {
            j += 1;
        }
        if j >= chars.len() || chars[j] != ']' {
            i += 1;
            continue;
        }
        let contents: String = chars[i + 1..j].iter().collect();
        match parse_group(&contents) {
            Some(mut found) => {
                out.append(&mut found);
                i = j + 1;
            }
            None => i += 1,
        }
    }
    out
}

/// A bracket group is a citation only if every token in it is one, so a
/// mixed group like `[E1, see below]` stays prose.
fn parse_group(contents: &str) -> Option<Vec<Marker>> {
    let tokens: Vec<&str> = contents.split(&[',', ';'][..]).map(str::trim).collect();
    if tokens.iter().any(|t| t.is_empty()) {
        return None;
    }
    if tokens.iter().all(|t| t.chars().all(|c| c.is_ascii_digit())) {
        return tokens
            .iter()
            .map(|t| t.parse::<usize>().ok().map(Marker::Index))
            .collect();
    }
    if tokens.iter().all(|t| is_key(t)) {
        return Some(tokens.iter().map(|t| Marker::Key(t.to_string())).collect());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_key_is_a_marker() {
        assert_eq!(markers("cited [E1]."), vec![Marker::Key("E1".into())]);
    }

    #[test]
    fn a_group_yields_one_marker_per_token() {
        assert_eq!(
            markers("cited [E1, E2]."),
            vec![Marker::Key("E1".into()), Marker::Key("E2".into())]
        );
    }

    #[test]
    fn numbers_are_index_markers() {
        assert_eq!(markers("cited [3]."), vec![Marker::Index(3)]);
    }

    #[test]
    fn a_mixed_group_is_prose() {
        assert!(markers("cited [E1, see below].").is_empty());
    }

    #[test]
    fn an_index_expression_is_not_a_marker() {
        assert!(markers("samples[0] and rows[12]").is_empty());
    }

    #[test]
    fn a_marker_does_not_span_a_line() {
        assert!(markers("open [E1\n] close").is_empty());
    }

    #[test]
    fn a_code_span_hides_its_brackets() {
        assert!(markers("call `f([1])` first").is_empty());
    }

    #[test]
    fn a_long_prefix_is_not_a_key() {
        assert!(markers("see [ABCDE12]").is_empty());
    }

    #[test]
    fn a_markdown_link_label_stays_prose() {
        assert!(markers("see [the docs](https://example.invalid)").is_empty());
    }

    #[test]
    fn an_empty_group_is_prose() {
        assert!(markers("nothing [] here").is_empty());
        assert!(markers("nothing [E1,] here").is_empty());
    }

    #[test]
    fn a_marker_at_the_start_of_the_text_is_found() {
        assert_eq!(
            markers("[E1] opens the line"),
            vec![Marker::Key("E1".into())]
        );
    }

    #[test]
    fn as_written_is_what_a_reader_would_search_for() {
        assert_eq!(Marker::Key("E9".into()).as_written(), "E9");
        assert_eq!(Marker::Index(4).as_written(), "4");
    }
}
