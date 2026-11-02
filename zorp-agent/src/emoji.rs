//! Standard emoji shortcodes for the terminal.
//!
//! `:rocket:` becomes 🚀 before a message is sent, so the model, the recall
//! index and a quote a year from now all carry the character rather than a
//! name this table might not still have. Expansion happens once, on the line
//! before it is sent (see the REPL), never over the stored transcript. A
//! shortcode inside a code span or fence is left alone, because `:smile:` in a
//! shell snippet is a shell snippet, and a name this table does not know
//! (including a custom emoji, which has no character) is left literal.
//!
//! The table (`emoji_table::SHORTCODES`) is generated from the same source as
//! the browser's `web/src/emoji-table.ts`, and `tests/emoji_drift.rs` asserts
//! the two agree.

use crate::emoji_table::SHORTCODES;
use regex::{Captures, Regex};
use std::sync::OnceLock;

/// The character for a shortcode, or `None` if this table does not know it.
pub fn lookup(name: &str) -> Option<&'static str> {
    SHORTCODES
        .binary_search_by(|(key, _)| (*key).cmp(name))
        .ok()
        .map(|i| SHORTCODES[i].1)
}

/// Shortcode names beginning with `prefix`, for the REPL's Tab completion,
/// in table (sorted) order and capped so a one-character prefix does not print
/// a wall. An empty prefix returns nothing, the way the slash completer needs
/// at least the leading character.
pub fn names_with_prefix(prefix: &str) -> Vec<&'static str> {
    if prefix.is_empty() {
        return Vec::new();
    }
    SHORTCODES
        .iter()
        .filter(|(name, _)| name.starts_with(prefix))
        .map(|(name, _)| *name)
        .take(50)
        .collect()
}

/// A fenced block or inline code span (group 1), or a `:shortcode:` (group 2).
/// Code is matched first so its contents are returned verbatim; `(?s)` lets a
/// fence span lines.
fn pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?s)(```.*?```|`[^`\n]*`)|:([a-z0-9_+-]+):").unwrap())
}

/// Expand known standard shortcodes in `text`, leaving code spans, fenced
/// blocks, and unknown or custom names exactly as they were.
pub fn expand(text: &str) -> String {
    pattern()
        .replace_all(text, |caps: &Captures| {
            if let Some(code) = caps.get(1) {
                code.as_str().to_string()
            } else {
                let name = &caps[2];
                match lookup(name) {
                    Some(ch) => ch.to_string(),
                    None => format!(":{name}:"),
                }
            }
        })
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_known_shortcode_becomes_its_character() {
        assert_eq!(expand("ship it :rocket:"), "ship it 🚀");
        assert_eq!(expand(":+1: :-1:"), "👍 👎");
    }

    #[test]
    fn an_unknown_or_custom_name_is_left_literal() {
        assert_eq!(expand("hi :notareal_emoji:"), "hi :notareal_emoji:");
        assert_eq!(expand("my :partyparrot: here"), "my :partyparrot: here");
    }

    #[test]
    fn a_code_span_is_left_alone() {
        assert_eq!(expand("use `:rocket:` here"), "use `:rocket:` here");
        assert_eq!(expand("```\n:rocket:\n```"), "```\n:rocket:\n```");
        // Outside the span still expands.
        assert_eq!(expand("`:rocket:` then :rocket:"), "`:rocket:` then 🚀");
    }

    #[test]
    fn the_table_is_sorted_so_binary_search_holds() {
        for pair in SHORTCODES.windows(2) {
            assert!(pair[0].0 < pair[1].0, "{} !< {}", pair[0].0, pair[1].0);
        }
    }
}
