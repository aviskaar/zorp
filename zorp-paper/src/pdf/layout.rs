//! Turning a paragraph of markdown into lines that fit a column.

use super::metrics::{text_width, width, Face};

/// A stretch of text in one face.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    pub face: Face,
}

impl Run {
    pub fn new(text: impl Into<String>, face: Face) -> Run {
        Run {
            text: text.into(),
            face,
        }
    }
}

/// Split inline markdown into faced runs. Supports `**bold**`,
/// `*italic*` and `` `code` ``, which is the whole set a paper drafted
/// from an evidence record actually uses. Everything else, including an
/// unclosed marker, stays literal text rather than becoming an error:
/// refusing to typeset a document over a stray asterisk would be the
/// wrong trade.
pub fn parse_inline(text: &str, base: Face) -> Vec<Run> {
    let chars: Vec<char> = text.chars().collect();
    let mut runs: Vec<Run> = Vec::new();
    let mut plain = String::new();
    let mut i = 0;

    while i < chars.len() {
        let (marker, face, len) = if chars[i] == '*' && chars.get(i + 1) == Some(&'*') {
            ("**", Face::Bold, 2)
        } else if chars[i] == '*' {
            ("*", Face::Italic, 1)
        } else if chars[i] == '`' {
            ("`", Face::Mono, 1)
        } else {
            plain.push(chars[i]);
            i += 1;
            continue;
        };

        match find_close(&chars, i + len, marker) {
            Some(end) => {
                if !plain.is_empty() {
                    runs.push(Run::new(std::mem::take(&mut plain), base));
                }
                let inner: String = chars[i + len..end].iter().collect();
                // Emphasis inside code is not emphasis, it is code.
                if face == Face::Mono {
                    runs.push(Run::new(inner, Face::Mono));
                } else {
                    runs.extend(parse_inline(&inner, face));
                }
                i = end + len;
            }
            None => {
                plain.push(chars[i]);
                i += 1;
            }
        }
    }
    if !plain.is_empty() {
        runs.push(Run::new(plain, base));
    }
    runs
}

/// The next occurrence of `marker` at or after `from`, or `None`. An
/// empty span (`**` immediately followed by `**`) does not count as a
/// close, so `****` stays literal instead of vanishing.
fn find_close(chars: &[char], from: usize, marker: &str) -> Option<usize> {
    let marker: Vec<char> = marker.chars().collect();
    let mut i = from;
    while i + marker.len() <= chars.len() {
        if chars[i..i + marker.len()] == marker[..] && i > from {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// One word, carrying the face it was written in.
#[derive(Debug, Clone)]
struct Word {
    text: String,
    face: Face,
}

fn words(runs: &[Run]) -> Vec<Word> {
    let mut out = Vec::new();
    for run in runs {
        for word in run.text.split_whitespace() {
            out.push(Word {
                text: word.to_string(),
                face: run.face,
            });
        }
    }
    out
}

/// Break `word` into pieces that each fit `max_width`. Only reached by a
/// token with no break opportunity in it, such as a long identifier or a
/// URL, which would otherwise run off the page.
fn hard_break(word: &Word, size: f32, max_width: f32) -> Vec<Word> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut current_width = 0.0f32;
    for c in word.text.chars() {
        let w = f32::from(width(word.face, c)) * size / 1000.0;
        if current_width + w > max_width && !current.is_empty() {
            out.push(Word {
                text: std::mem::take(&mut current),
                face: word.face,
            });
            current_width = 0.0;
        }
        current.push(c);
        current_width += w;
    }
    if !current.is_empty() {
        out.push(Word {
            text: current,
            face: word.face,
        });
    }
    out
}

/// Greedy line breaking. Ragged right, no hyphenation, no justification.
pub fn wrap(runs: &[Run], size: f32, max_width: f32) -> Vec<Vec<Run>> {
    let mut lines: Vec<Vec<Run>> = Vec::new();
    let mut line: Vec<Word> = Vec::new();
    let mut line_width = 0.0f32;

    let mut queue: Vec<Word> = Vec::new();
    for word in words(runs) {
        if text_width(word.face, &word.text, size) > max_width {
            queue.extend(hard_break(&word, size, max_width));
        } else {
            queue.push(word);
        }
    }

    for word in queue {
        let word_width = text_width(word.face, &word.text, size);
        let space = if line.is_empty() {
            0.0
        } else {
            text_width(word.face, " ", size)
        };
        if !line.is_empty() && line_width + space + word_width > max_width {
            lines.push(join(&line));
            line.clear();
            line_width = 0.0;
        }
        line_width += if line.is_empty() { 0.0 } else { space };
        line_width += word_width;
        line.push(word);
    }
    if !line.is_empty() {
        lines.push(join(&line));
    }
    lines
}

/// Words back into runs, merging neighbours that share a face so the
/// content stream carries one string per face change and not one per
/// word.
fn join(line: &[Word]) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    for (i, word) in line.iter().enumerate() {
        let separator = if i == 0 { "" } else { " " };
        match out.last_mut() {
            Some(last) if last.face == word.face => {
                last.text.push_str(separator);
                last.text.push_str(&word.text);
            }
            _ => {
                let mut text = String::from(separator);
                text.push_str(&word.text);
                out.push(Run::new(text, word.face));
            }
        }
    }
    out
}

/// How wide a laid-out line is, for centring.
pub fn line_width(line: &[Run], size: f32) -> f32 {
    line.iter().map(|r| text_width(r.face, &r.text, size)).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_one_run() {
        assert_eq!(
            parse_inline("hello there", Face::Roman),
            vec![Run::new("hello there", Face::Roman)]
        );
    }

    #[test]
    fn bold_wins_over_italic_on_a_double_asterisk() {
        assert_eq!(
            parse_inline("a **b** c", Face::Roman),
            vec![
                Run::new("a ", Face::Roman),
                Run::new("b", Face::Bold),
                Run::new(" c", Face::Roman),
            ]
        );
    }

    #[test]
    fn italic_and_code_each_get_a_face() {
        assert_eq!(
            parse_inline("a *b* `c`", Face::Roman),
            vec![
                Run::new("a ", Face::Roman),
                Run::new("b", Face::Italic),
                Run::new(" ", Face::Roman),
                Run::new("c", Face::Mono),
            ]
        );
    }

    #[test]
    fn an_unclosed_marker_stays_literal() {
        assert_eq!(
            parse_inline("2 * 3 is six", Face::Roman),
            vec![Run::new("2 * 3 is six", Face::Roman)]
        );
    }

    #[test]
    fn emphasis_inside_code_is_code() {
        assert_eq!(
            parse_inline("`a * b`", Face::Roman),
            vec![Run::new("a * b", Face::Mono)]
        );
    }

    #[test]
    fn nested_emphasis_keeps_the_inner_face() {
        let runs = parse_inline("**bold and *both* back**", Face::Roman);
        assert!(runs.iter().any(|r| r.face == Face::Bold));
        assert!(runs.iter().any(|r| r.face == Face::Italic));
    }

    #[test]
    fn wrapping_fills_lines_up_to_the_width() {
        let runs = vec![Run::new("one two three four five six seven", Face::Roman)];
        let lines = wrap(&runs, 10.0, 60.0);
        assert!(lines.len() > 1);
        for line in &lines {
            assert!(line_width(line, 10.0) <= 60.0, "{line:?}");
        }
    }

    #[test]
    fn wrapping_keeps_every_word() {
        let runs = vec![Run::new("one two three four five six seven", Face::Roman)];
        let lines = wrap(&runs, 10.0, 60.0);
        let joined: String = lines
            .iter()
            .map(|l| {
                l.iter()
                    .map(|r| r.text.as_str())
                    .collect::<String>()
                    .trim()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(joined, "one two three four five six seven");
    }

    #[test]
    fn an_oversized_token_is_broken_to_fit() {
        let runs = vec![Run::new("x".repeat(200), Face::Roman)];
        let lines = wrap(&runs, 10.0, 100.0);
        assert!(lines.len() > 1);
        for line in &lines {
            assert!(line_width(line, 10.0) <= 100.0);
        }
    }

    #[test]
    fn an_empty_paragraph_lays_out_as_no_lines() {
        assert!(wrap(&[Run::new("   ", Face::Roman)], 10.0, 100.0).is_empty());
    }

    #[test]
    fn a_line_merges_neighbouring_runs_of_the_same_face() {
        let runs = vec![Run::new("one ", Face::Roman), Run::new("two", Face::Roman)];
        let lines = wrap(&runs, 10.0, 500.0);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].len(), 1);
        assert_eq!(lines[0][0].text, "one two");
    }
}
