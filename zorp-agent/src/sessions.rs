//! Finding your way back into a conversation you have already had.
//!
//! The browser and the terminal share one session store: `Store::open_default`
//! resolves the same `sessions.db` for both. So every conversation from the
//! browser sidebar is already sitting there when `zorp-agent` starts, and
//! every conversation from the terminal turns up in that sidebar.
//!
//! The browser could see that list and the terminal could not. `resume <id>`
//! was the only way back in, and nothing told you what an id was.
//!
//! This module is the part worth testing without a terminal: turning rows
//! into lines, and turning what somebody typed into the one row they meant.
//! Printing them is `main.rs`'s job.

use crate::session::SessionRow;

/// How many conversations `sessions` shows before you ask for more.
///
/// Somebody who has used this for a year should not get a year of
/// scrollback for typing four words.
pub const DEFAULT_LIMIT: usize = 20;

/// The widest a name is allowed to be on one line.
///
/// A first message can be a pasted file, and the whole point of the list is
/// that you can read down the left edge of it.
const NAME_WIDTH: usize = 56;

/// What went wrong when a prefix did not name exactly one conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// Nothing in the store starts with what was typed.
    NotFound,
    /// Several do. Carries their ids, newest first, so the caller can print
    /// them rather than guess.
    Ambiguous(Vec<String>),
}

/// The conversation somebody meant by `wanted`.
///
/// An exact id wins outright, even when it is also a prefix of a longer one.
/// Otherwise a unique prefix resolves, the way git takes a short sha, and an
/// ambiguous one is an error carrying the candidates. Guessing between two
/// conversations is the one thing this must not do: the wrong guess drops
/// somebody into a stranger's thread and the transcript looks plausible.
pub fn resolve<'a>(rows: &'a [SessionRow], wanted: &str) -> Result<&'a SessionRow, ResolveError> {
    if let Some(exact) = rows.iter().find(|row| row.id == wanted) {
        return Ok(exact);
    }
    let matches: Vec<&SessionRow> = rows
        .iter()
        .filter(|row| row.id.starts_with(wanted))
        .collect();
    match matches.len() {
        0 => Err(ResolveError::NotFound),
        1 => Ok(matches[0]),
        _ => Err(ResolveError::Ambiguous(
            matches.into_iter().map(|row| row.id.clone()).collect(),
        )),
    }
}

/// What a conversation is called on one line.
///
/// `display_title` when something has written one, and the first line of the
/// verbatim first message otherwise. The fallback is the point: a titling
/// call that failed, was declined, or was never made leaves a list that
/// reads exactly as it did before titles existed.
///
/// Never the other way round. `task` is what a person typed and
/// `display_title` may hold a sentence a model wrote, and the two are kept
/// apart everywhere else in this crate for reasons the `SessionRow` doc
/// comment gives.
pub fn name(row: &SessionRow) -> String {
    let raw = row
        .display_title
        .as_deref()
        .filter(|t| !t.trim().is_empty())
        .unwrap_or(&row.task);
    clip(first_line(raw), NAME_WIDTH)
}

/// The first line with anything on it, with the invisible characters gone.
///
/// A first message can be pasted from anywhere, so it can carry control
/// characters and the bidirectional overrides. An override on a listing
/// reorders every line drawn after it, which is a way to make one
/// conversation impersonate another in a list somebody is picking from.
fn first_line(raw: &str) -> String {
    let line = raw
        .split(['\n', '\r', '\u{2028}', '\u{2029}'])
        .map(scrub)
        .find(|l| !l.is_empty())
        .unwrap_or_default();
    if line.is_empty() {
        return "(no message yet)".to_string();
    }
    line
}

fn scrub(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut spaced = true;
    for c in line.chars() {
        if c.is_control() || is_invisible(c) {
            continue;
        }
        if c.is_whitespace() {
            if !spaced {
                out.push(' ');
                spaced = true;
            }
            continue;
        }
        out.push(c);
        spaced = false;
    }
    out.trim().to_string()
}

fn is_invisible(c: char) -> bool {
    matches!(c,
        '\u{200B}'..='\u{200F}'
        | '\u{202A}'..='\u{202E}'
        | '\u{2060}'..='\u{2064}'
        | '\u{2066}'..='\u{2069}'
        | '\u{FEFF}'
    )
}

/// Cut to `width` characters, counted as characters and not as bytes, with
/// an ellipsis standing in for what went.
fn clip(text: String, width: usize) -> String {
    if text.chars().count() <= width {
        return text;
    }
    let kept: String = text.chars().take(width.saturating_sub(3)).collect();
    format!("{}...", kept.trim_end())
}

/// How long ago, in the shortest form that is still true.
///
/// `updated` is epoch milliseconds and is the only clock the store has. A
/// row written by a build with no clock reads as `unknown` rather than as
/// 1970, because a date nobody recorded is not a date.
pub fn when(updated: i64, now: i64) -> String {
    if updated <= 0 {
        return "unknown".to_string();
    }
    let seconds = (now - updated) / 1000;
    if seconds < 0 {
        // A row from the future is a clock that moved, not a conversation
        // that has not happened. Saying "just now" beats negative minutes.
        return "just now".to_string();
    }
    match seconds {
        s if s < 60 => "just now".to_string(),
        s if s < 3600 => format!("{}m ago", s / 60),
        s if s < 86_400 => format!("{}h ago", s / 3600),
        s if s < 86_400 * 30 => format!("{}d ago", s / 86_400),
        s => format!("{}mo ago", s / (86_400 * 30)),
    }
}

/// One conversation, as a line.
///
/// The id first, because the id is what the next command takes. Eight
/// characters of it, which is what `resume` accepts as a prefix, with the
/// full id available from `/status` inside the session for anybody who
/// needs it.
pub fn line(row: &SessionRow, now: i64) -> String {
    format!(
        "{:<10} {:>9}  {:<9} {}",
        short(&row.id),
        when(row.updated, now),
        row.status,
        name(row)
    )
}

/// The prefix shown in a listing, and the length `resume` is expected to
/// take. Ids are time ordered and process unique, so eight hex characters
/// separate them for any history a person actually has.
pub fn short(id: &str) -> String {
    id.chars().take(8).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, task: &str, title: Option<&str>, updated: i64) -> SessionRow {
        SessionRow {
            id: id.to_string(),
            task: task.to_string(),
            repo: "/repo".to_string(),
            model: "m".to_string(),
            status: "done".to_string(),
            display_title: title.map(str::to_string),
            updated,
            project_id: None,
        }
    }

    #[test]
    fn an_exact_id_resolves() {
        let rows = vec![row("abc123", "ask", None, 1), row("def456", "ask", None, 2)];
        assert_eq!(resolve(&rows, "abc123").unwrap().id, "abc123");
    }

    #[test]
    fn a_unique_prefix_resolves() {
        let rows = vec![row("abc123", "ask", None, 1), row("def456", "ask", None, 2)];
        assert_eq!(resolve(&rows, "ab").unwrap().id, "abc123");
    }

    /// Guessing between two conversations drops somebody into a stranger's
    /// thread, and the transcript looks perfectly plausible when it happens.
    #[test]
    fn an_ambiguous_prefix_names_the_candidates() {
        let rows = vec![
            row("abc123", "ask", None, 1),
            row("abc999", "ask", None, 2),
            row("def456", "ask", None, 3),
        ];
        match resolve(&rows, "abc") {
            Err(ResolveError::Ambiguous(ids)) => {
                assert_eq!(ids, vec!["abc123".to_string(), "abc999".to_string()]);
            }
            Ok(row) => panic!("an ambiguous prefix resolved to {}", row.id),
            Err(other) => panic!("expected an ambiguous prefix, got {other:?}"),
        }
    }

    /// An id that is also a prefix of a longer one is still that id. Without
    /// this, a conversation becomes unreachable the moment a later one
    /// happens to extend its id.
    #[test]
    fn an_exact_id_wins_over_being_a_prefix() {
        let rows = vec![row("abc", "ask", None, 1), row("abcdef", "ask", None, 2)];
        assert_eq!(resolve(&rows, "abc").unwrap().id, "abc");
    }

    #[test]
    fn an_unknown_prefix_is_not_found() {
        let rows = vec![row("abc123", "ask", None, 1)];
        assert_eq!(resolve(&rows, "zz").err(), Some(ResolveError::NotFound));
    }

    #[test]
    fn an_empty_store_finds_nothing_rather_than_everything() {
        assert_eq!(resolve(&[], "abc").err(), Some(ResolveError::NotFound));
    }

    /* ---------------------------------------------------------------- */
    /* names                                                             */
    /* ---------------------------------------------------------------- */

    #[test]
    fn a_title_is_preferred_and_the_first_message_is_the_fallback() {
        assert_eq!(
            name(&row("a", "write hello.txt", Some("Writing hello.txt"), 1)),
            "Writing hello.txt"
        );
        assert_eq!(
            name(&row("a", "write hello.txt", None, 1)),
            "write hello.txt"
        );
        // A title that is only whitespace is not a title.
        assert_eq!(
            name(&row("a", "write hello.txt", Some("   "), 1)),
            "write hello.txt"
        );
    }

    #[test]
    fn a_long_name_is_clipped_by_characters_not_bytes() {
        let long = "é".repeat(200);
        let clipped = name(&row("a", &long, None, 1));
        assert_eq!(clipped.chars().count(), NAME_WIDTH);
        assert!(clipped.ends_with("..."));
    }

    #[test]
    fn a_multi_line_first_message_shows_only_its_first_line() {
        assert_eq!(
            name(&row(
                "a",
                "fix the parser\n\nhere is the whole file\n...",
                None,
                1
            )),
            "fix the parser"
        );
    }

    /// A bidirectional override in a listing reorders every line after it,
    /// which is how one conversation impersonates another in a list somebody
    /// is picking from.
    #[test]
    fn control_and_bidirectional_characters_never_reach_the_line() {
        let hostile = "\u{202E}drop\u{0007} the\u{200B} database";
        let shown = name(&row("a", hostile, None, 1));
        assert!(!shown.contains('\u{202E}'), "{shown:?}");
        assert!(!shown.contains('\u{0007}'), "{shown:?}");
        assert!(!shown.contains('\u{200B}'), "{shown:?}");
        assert_eq!(shown, "drop the database");
    }

    #[test]
    fn a_conversation_with_no_message_yet_says_so() {
        assert_eq!(name(&row("a", "   ", None, 1)), "(no message yet)");
    }

    /* ---------------------------------------------------------------- */
    /* when                                                              */
    /* ---------------------------------------------------------------- */

    #[test]
    fn the_clock_reads_in_the_shortest_true_unit() {
        let now = 1_700_000_000_000i64;
        assert_eq!(when(now - 5_000, now), "just now");
        assert_eq!(when(now - 120_000, now), "2m ago");
        assert_eq!(when(now - 7_200_000, now), "2h ago");
        assert_eq!(when(now - 86_400_000 * 3, now), "3d ago");
        assert_eq!(when(now - 86_400_000 * 90, now), "3mo ago");
    }

    /// A date nobody recorded is not a date. A build with no clock wrote
    /// zero here, and reading that back as 1970 would be inventing one.
    #[test]
    fn an_unrecorded_time_says_unknown_rather_than_1970() {
        assert_eq!(when(0, 1_700_000_000_000), "unknown");
    }

    #[test]
    fn a_row_from_the_future_is_a_clock_that_moved() {
        let now = 1_700_000_000_000i64;
        assert_eq!(when(now + 60_000, now), "just now");
    }

    #[test]
    fn a_line_leads_with_the_id_the_next_command_takes() {
        let now = 1_700_000_000_000i64;
        let text = line(
            &row("abc12345def", "write hello.txt", None, now - 120_000),
            now,
        );
        assert!(text.starts_with("abc12345"), "{text}");
        assert!(text.contains("2m ago"), "{text}");
        assert!(text.contains("write hello.txt"), "{text}");
    }
}
