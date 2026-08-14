use chrono::Utc;
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_SLUG_CHARS: usize = 60;

/// Process-wide counter appended to timestamp-based row ids so two
/// inserts landing in the same millisecond still get distinct ids
/// instead of colliding on a primary key. Shared by checkpoints,
/// experiments, metrics, and validations.
static NEXT_SEQ: AtomicU64 = AtomicU64::new(0);

pub(crate) fn next_seq() -> u64 {
    NEXT_SEQ.fetch_add(1, Ordering::SeqCst)
}

/// Generate a date-prefixed, lowercase, hyphenated slug from hypothesis
/// text, e.g. "Adaptive Memory Consolidation!" becomes
/// "2026-08-09-adaptive-memory-consolidation".
pub fn track_id(hypothesis: &str) -> String {
    let date = Utc::now().format("%Y-%m-%d");
    let slug = slugify(hypothesis);
    format!("{date}-{slug}")
}

fn slugify(text: &str) -> String {
    let mut slug = String::new();
    let mut last_was_hyphen = true; // suppress a leading hyphen
    for ch in text.to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            last_was_hyphen = false;
        } else if !last_was_hyphen {
            slug.push('-');
            last_was_hyphen = true;
        }
        if slug.chars().count() >= MAX_SLUG_CHARS {
            break;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        "untitled".to_string()
    } else {
        slug
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_is_lowercase_and_hyphenated() {
        let id = track_id("Adaptive Memory Consolidation!");
        assert!(id.ends_with("-adaptive-memory-consolidation"), "got: {id}");
    }

    #[test]
    fn slug_has_todays_date_prefix() {
        let id = track_id("anything");
        let today = Utc::now().format("%Y-%m-%d").to_string();
        assert!(id.starts_with(&today), "got: {id}");
    }

    #[test]
    fn slug_collapses_repeated_punctuation() {
        let id = track_id("a --- b   c!!!d");
        assert!(id.ends_with("-a-b-c-d"), "got: {id}");
    }

    #[test]
    fn slug_truncates_long_text() {
        let long = "word ".repeat(30);
        let id = track_id(&long);
        let slug_part = &id[11..]; // strip the fixed "YYYY-MM-DD-" prefix
        assert!(slug_part.chars().count() <= MAX_SLUG_CHARS, "got: {slug_part}");
    }

    #[test]
    fn empty_hypothesis_falls_back_to_untitled() {
        let id = track_id("   ...   ");
        assert!(id.ends_with("-untitled"), "got: {id}");
    }
}
