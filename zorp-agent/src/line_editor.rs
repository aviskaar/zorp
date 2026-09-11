//! The editable input line, and the history behind it.
//!
//! The chat REPL's input loop handled five keys: Enter, Backspace, Ctrl-C,
//! Ctrl-D and Ctrl-V for a pasted image. You could not move the cursor
//! left. Typing a long message and spotting a typo in the middle of it left
//! backspacing to the mistake and retyping the rest, and pressing Up put an
//! escape sequence in your text.
//!
//! This is the part worth testing without a terminal: where the cursor is,
//! what each key does to the buffer, what history remembers, and what Tab
//! completes. The crossterm event loop in `main.rs` reads keys and calls
//! into here.
//!
//! # Why not a crate
//!
//! `reedline` and `rustyline` both do the standard keys well, and neither
//! can carry the two things that are not standard here.
//!
//! The buffer is not a `String`. It is a run of [`Segment`]s, because one
//! message can carry typed text, a bracketed paste shown as
//! `[pasted +N characters]` rather than inline, and an image pasted from
//! the clipboard. Both crates hand back a `String` from their own event
//! loop, and a paste is merged into it as text, so the paste marker and the
//! image path would both have to happen outside that loop. Outside their
//! event loop is outside the crate.
//!
//! So the loop grew instead, which the issue that asked for this named as a
//! fine answer if the crates could not carry those two, and the keys below
//! are its checklist.
//!
//! # Raw mode
//!
//! Not this module's business. It holds no terminal and writes nothing.
//! `main.rs` enters raw mode for the input line and leaves it around a
//! turn, which is what it did before, so a crash cannot strand somebody in
//! a terminal that does not echo.

use std::path::PathBuf;

/// One piece of an input line.
///
/// A message can carry typed text, a paste shown as a marker rather than
/// inline, and an image. The editor moves a cursor through the text and
/// leaves the other two alone: there is nothing to edit inside an image,
/// and a paste is deleted whole because deleting it a character at a time
/// is what nobody wants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    Text(String),
    Paste(String),
    Image {
        data: Vec<u8>,
        mime_type: String,
        index: usize,
    },
}

/// How many messages the history keeps.
///
/// Capped, because this is a file recording what somebody asked an agent
/// about their own files.
pub const MAX_HISTORY: usize = 500;

/// Set to `0` to keep no history at all.
///
/// Opt out rather than opt in, matching `ZORP_SESSION_TITLES` and
/// `ZORP_STREAM`, but the variable exists and is documented because a
/// history of what somebody asked an agent about their own machine is a
/// sensitive file to start writing without saying so.
pub const HISTORY_ENV: &str = "ZORP_HISTORY";

/// Whether history is kept.
pub fn history_enabled() -> bool {
    std::env::var(HISTORY_ENV).map(|v| v != "0").unwrap_or(true)
}

/// Where the history file lives.
///
/// Beside the other state files, through the same helper `ZORP_STATE_DB`
/// and `ZORP_TRUST_FILE` use, so it lands in `$XDG_STATE_HOME/zorp/` like
/// everything else rather than somewhere of its own.
pub fn history_path() -> PathBuf {
    crate::trust::state_path("ZORP_HISTORY_FILE", "history")
}

/// The input line: segments, and where the cursor is in the text.
///
/// The cursor is a character offset into the flattened *editable* text,
/// which is the concatenation of the `Text` segments. A paste and an image
/// each occupy one position, so the cursor can sit either side of one and
/// delete it whole.
#[derive(Debug, Default)]
pub struct Line {
    segments: Vec<Segment>,
    /// Character offset into `flat()`.
    cursor: usize,
}

impl Line {
    pub fn new() -> Line {
        Line {
            segments: vec![Segment::Text(String::new())],
            cursor: 0,
        }
    }

    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// The editable text, with a paste and an image each standing in as one
    /// character so the cursor has somewhere to be beside them.
    pub fn flat(&self) -> String {
        let mut out = String::new();
        for segment in &self.segments {
            match segment {
                Segment::Text(t) => out.push_str(t),
                Segment::Paste(_) | Segment::Image { .. } => out.push('\u{FFFC}'),
            }
        }
        out
    }

    pub fn len(&self) -> usize {
        self.flat().chars().count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether the line has nothing on it at all, so Enter can be ignored
    /// rather than sending an empty message.
    pub fn is_blank(&self) -> bool {
        self.segments.iter().all(|s| match s {
            Segment::Text(t) => t.trim().is_empty(),
            _ => false,
        })
    }

    /// Replace everything, putting the cursor at the end. What recalling a
    /// history entry does.
    pub fn set_text(&mut self, text: &str) {
        self.segments = vec![Segment::Text(text.to_string())];
        self.cursor = text.chars().count();
    }

    pub fn clear(&mut self) {
        self.segments = vec![Segment::Text(String::new())];
        self.cursor = 0;
    }

    /// Rebuild the segment list from a flat string plus the non-text
    /// segments in their original order.
    ///
    /// Editing works on the flat text and this puts the pieces back, so
    /// every motion and deletion below is written once against a string
    /// rather than once per segment kind.
    fn rebuild(&mut self, flat: &str) {
        let opaque: Vec<Segment> = self
            .segments
            .iter()
            .filter(|s| !matches!(s, Segment::Text(_)))
            .cloned()
            .collect();
        let mut rebuilt = Vec::new();
        let mut text = String::new();
        let mut next_opaque = 0usize;
        for c in flat.chars() {
            if c == '\u{FFFC}' {
                rebuilt.push(Segment::Text(std::mem::take(&mut text)));
                if let Some(segment) = opaque.get(next_opaque) {
                    rebuilt.push(segment.clone());
                }
                next_opaque += 1;
                continue;
            }
            text.push(c);
        }
        rebuilt.push(Segment::Text(text));
        self.segments = rebuilt;
    }

    fn apply(&mut self, flat: String, cursor: usize) {
        self.rebuild(&flat);
        self.cursor = cursor.min(flat.chars().count());
    }

    pub fn insert(&mut self, c: char) {
        let mut chars: Vec<char> = self.flat().chars().collect();
        let at = self.cursor.min(chars.len());
        chars.insert(at, c);
        self.apply(chars.into_iter().collect(), at + 1);
    }

    pub fn insert_str(&mut self, text: &str) {
        for c in text.chars() {
            self.insert(c);
        }
    }

    /// Add a segment that is not text, at the cursor.
    pub fn push_opaque(&mut self, segment: Segment) {
        debug_assert!(!matches!(segment, Segment::Text(_)));
        let mut chars: Vec<char> = self.flat().chars().collect();
        let at = self.cursor.min(chars.len());
        chars.insert(at, '\u{FFFC}');
        let flat: String = chars.into_iter().collect();
        // Insert into the opaque run at the right position, which is how
        // many opaque characters precede the cursor.
        let before = flat.chars().take(at).filter(|c| *c == '\u{FFFC}').count();
        let mut opaque: Vec<Segment> = self
            .segments
            .iter()
            .filter(|s| !matches!(s, Segment::Text(_)))
            .cloned()
            .collect();
        opaque.insert(before.min(opaque.len()), segment);
        // `rebuild` reads the opaque segments off `self.segments`, so put
        // the new list there first.
        let mut staged: Vec<Segment> = vec![Segment::Text(String::new())];
        staged.extend(opaque);
        self.segments = staged;
        self.apply(flat, at + 1);
    }

    /// Delete the character before the cursor. A paste or an image goes
    /// whole, because deleting one a character at a time is what nobody
    /// wants.
    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let mut chars: Vec<char> = self.flat().chars().collect();
        let at = self.cursor - 1;
        chars.remove(at);
        self.apply(chars.into_iter().collect(), at);
    }

    pub fn delete_forward(&mut self) {
        let mut chars: Vec<char> = self.flat().chars().collect();
        if self.cursor >= chars.len() {
            return;
        }
        let at = self.cursor;
        chars.remove(at);
        self.apply(chars.into_iter().collect(), at);
    }

    pub fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.len());
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.len();
    }

    /// The start of the word behind the cursor, skipping the whitespace
    /// between them first, so pressing it in the gap between two words
    /// lands on the earlier one rather than not moving.
    fn word_start(&self) -> usize {
        let chars: Vec<char> = self.flat().chars().collect();
        let mut at = self.cursor;
        while at > 0 && chars[at - 1].is_whitespace() {
            at -= 1;
        }
        while at > 0 && !chars[at - 1].is_whitespace() {
            at -= 1;
        }
        at
    }

    fn word_end(&self) -> usize {
        let chars: Vec<char> = self.flat().chars().collect();
        let mut at = self.cursor;
        while at < chars.len() && chars[at].is_whitespace() {
            at += 1;
        }
        while at < chars.len() && !chars[at].is_whitespace() {
            at += 1;
        }
        at
    }

    pub fn word_left(&mut self) {
        self.cursor = self.word_start();
    }

    pub fn word_right(&mut self) {
        self.cursor = self.word_end();
    }

    /// Ctrl-W: delete the word behind the cursor.
    pub fn delete_word_back(&mut self) {
        let start = self.word_start();
        if start == self.cursor {
            return;
        }
        let chars: Vec<char> = self.flat().chars().collect();
        let flat: String = chars[..start]
            .iter()
            .chain(chars[self.cursor..].iter())
            .collect();
        self.apply(flat, start);
    }

    /// Ctrl-U: clear everything before the cursor.
    pub fn delete_to_start(&mut self) {
        let chars: Vec<char> = self.flat().chars().collect();
        let flat: String = chars[self.cursor.min(chars.len())..].iter().collect();
        self.apply(flat, 0);
    }

    /// Ctrl-K: clear everything from the cursor on.
    pub fn delete_to_end(&mut self) {
        let chars: Vec<char> = self.flat().chars().collect();
        let at = self.cursor.min(chars.len());
        let flat: String = chars[..at].iter().collect();
        self.apply(flat, at);
    }

    /// Whether Enter should continue the line rather than send it.
    ///
    /// A trailing backslash, which works in every terminal, unlike
    /// Shift-Enter, which most of them do not report as distinct from
    /// Enter. The backslash goes away and a newline takes its place.
    pub fn wants_continuation(&self) -> bool {
        self.flat().ends_with('\\')
    }

    /// Turn the trailing backslash into a newline.
    pub fn continue_line(&mut self) {
        if !self.wants_continuation() {
            return;
        }
        self.backspace();
        self.insert('\n');
    }
}

/// What Tab completes: the slash commands, and the capsule names.
///
/// From the same list `parse_command` is given, so a capsule that exists is
/// completable and one that does not is not.
pub fn complete(line: &str, commands: &[&str], capsules: &[String]) -> Vec<String> {
    // Only a slash command, and only the first word of one. Completing
    // arbitrary text would guess at what somebody is writing.
    let Some(rest) = line.strip_prefix('/') else {
        return Vec::new();
    };
    if rest.contains(char::is_whitespace) {
        return Vec::new();
    }
    let mut out: Vec<String> = commands
        .iter()
        .filter(|c| c.starts_with(rest))
        .map(|c| format!("/{c}"))
        .collect();
    out.extend(
        capsules
            .iter()
            .filter(|c| c.starts_with(rest))
            .map(|c| format!("/{c}")),
    );
    out.sort();
    out.dedup();
    out
}

/// The longest prefix every candidate shares, so Tab fills in as much as it
/// can before offering a list.
pub fn common_prefix(candidates: &[String]) -> String {
    let Some(first) = candidates.first() else {
        return String::new();
    };
    let mut end = first.chars().count();
    for candidate in &candidates[1..] {
        let shared = first
            .chars()
            .zip(candidate.chars())
            .take_while(|(a, b)| a == b)
            .count();
        end = end.min(shared);
    }
    first.chars().take(end).collect()
}

/// What Up and Down walk.
///
/// Newest last, the way a shell's is, with the cursor one past the end when
/// nothing has been recalled yet. Consecutive duplicates are not stored,
/// because pressing Up four times to find the message before the four
/// identical ones is not history, it is an obstacle.
#[derive(Debug, Default)]
pub struct History {
    entries: Vec<String>,
    /// Where Up and Down are, as an index into `entries`. `None` means the
    /// line being typed now.
    at: Option<usize>,
    /// What was on the line before Up was first pressed, so Down comes back
    /// to it rather than to an empty line.
    draft: Option<String>,
}

impl History {
    pub fn new(entries: Vec<String>) -> History {
        History {
            entries,
            at: None,
            draft: None,
        }
    }

    /// Read the history file, or nothing when there is none or it is off.
    pub fn load() -> History {
        if !history_enabled() {
            return History::default();
        }
        let text = std::fs::read_to_string(history_path()).unwrap_or_default();
        let entries: Vec<String> = text
            .lines()
            .map(|l| l.replace("\\n", "\n"))
            .filter(|l| !l.trim().is_empty())
            .collect();
        History::new(entries)
    }

    pub fn entries(&self) -> &[String] {
        &self.entries
    }

    /// Remember one message, and write the file.
    ///
    /// A newline is escaped rather than written, so one entry is one line
    /// and a multi line message does not become several entries on the next
    /// read.
    pub fn remember(&mut self, message: &str) {
        self.at = None;
        self.draft = None;
        let message = message.trim();
        if message.is_empty() || self.entries.last().map(String::as_str) == Some(message) {
            return;
        }
        self.entries.push(message.to_string());
        if self.entries.len() > MAX_HISTORY {
            let excess = self.entries.len() - MAX_HISTORY;
            self.entries.drain(0..excess);
        }
        if !history_enabled() {
            return;
        }
        let path = history_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let text: String = self
            .entries
            .iter()
            .map(|e| format!("{}\n", e.replace('\n', "\\n")))
            .collect();
        // The same write the trust file gets: a temp file created owner
        // only, then renamed over. Writing and then chmod-ing would leave
        // everything somebody typed readable by anyone on the machine for
        // the length of the write, and a crash partway would truncate the
        // history rather than leave the previous one. Best effort even so:
        // a history file that could not be written is not a reason to stop
        // somebody talking to the agent.
        let _ = crate::trust::atomic_write(&path, text.as_bytes());
    }

    /// The previous message, or `None` at the far end.
    pub fn previous(&mut self, current: &str) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }
        let next = match self.at {
            None => {
                self.draft = Some(current.to_string());
                self.entries.len() - 1
            }
            Some(0) => return None,
            Some(at) => at - 1,
        };
        self.at = Some(next);
        self.entries.get(next).cloned()
    }

    /// The next message, or the draft that was on the line before Up.
    ///
    /// Named `forward` rather than `next` because a `next` on something
    /// that is not an iterator reads like one and is not.
    pub fn forward(&mut self) -> Option<String> {
        match self.at {
            None => None,
            Some(at) if at + 1 < self.entries.len() => {
                self.at = Some(at + 1);
                self.entries.get(at + 1).cloned()
            }
            Some(_) => {
                self.at = None;
                Some(self.draft.take().unwrap_or_default())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line_with(text: &str) -> Line {
        let mut line = Line::new();
        line.insert_str(text);
        line
    }

    #[test]
    fn typing_and_moving_puts_a_character_where_the_cursor_is() {
        let mut line = line_with("helo world");
        // The whole point of the issue: fix a typo in the middle without
        // retyping the rest.
        line.home();
        for _ in 0..3 {
            line.right();
        }
        line.insert('l');

        assert_eq!(line.flat(), "hello world");
        assert_eq!(line.cursor(), 4);
    }

    #[test]
    fn home_and_end_go_to_the_ends() {
        let mut line = line_with("hello");
        line.home();
        assert_eq!(line.cursor(), 0);
        line.end();
        assert_eq!(line.cursor(), 5);
        // And neither runs off.
        line.left();
        line.home();
        line.left();
        assert_eq!(line.cursor(), 0);
        line.end();
        line.right();
        assert_eq!(line.cursor(), 5);
    }

    #[test]
    fn backspace_and_delete_take_one_character_each() {
        let mut line = line_with("hello");
        line.backspace();
        assert_eq!(line.flat(), "hell");

        line.home();
        line.delete_forward();
        assert_eq!(line.flat(), "ell");
        assert_eq!(line.cursor(), 0);

        // Neither does anything at the wrong end.
        line.home();
        line.backspace();
        assert_eq!(line.flat(), "ell");
        line.end();
        line.delete_forward();
        assert_eq!(line.flat(), "ell");
    }

    #[test]
    fn word_motion_skips_the_gap_between_words() {
        let mut line = line_with("fix the parser bug");
        line.word_left();
        assert_eq!(line.cursor(), 15, "should be at the start of 'bug'");
        line.word_left();
        assert_eq!(line.cursor(), 8, "should be at the start of 'parser'");
        line.word_right();
        assert_eq!(line.cursor(), 14, "should be at the end of 'parser'");
    }

    #[test]
    fn ctrl_w_deletes_the_word_behind_the_cursor() {
        let mut line = line_with("fix the parser bug");
        line.delete_word_back();
        assert_eq!(line.flat(), "fix the parser ");
        line.delete_word_back();
        assert_eq!(line.flat(), "fix the ");
    }

    #[test]
    fn ctrl_u_clears_to_the_start_and_ctrl_k_to_the_end() {
        let mut line = line_with("fix the parser bug");
        line.home();
        line.word_right();
        line.delete_to_start();
        assert_eq!(line.flat(), " the parser bug");
        assert_eq!(line.cursor(), 0);

        let mut line = line_with("fix the parser bug");
        line.home();
        line.word_right();
        line.delete_to_end();
        assert_eq!(line.flat(), "fix");
    }

    /* ---------------------------------------------------------------- */
    /* the two things a crate could not carry                            */
    /* ---------------------------------------------------------------- */

    /// A paste is one thing on the line, shown as a marker, and it deletes
    /// whole. Deleting a two thousand character paste one character at a
    /// time is what nobody wants.
    #[test]
    fn a_paste_is_one_position_and_deletes_whole() {
        let mut line = line_with("look at ");
        line.push_opaque(Segment::Paste("a".repeat(2000)));
        line.insert_str(" please");

        assert_eq!(line.len(), "look at ".len() + 1 + " please".len());
        assert!(line
            .segments()
            .iter()
            .any(|s| matches!(s, Segment::Paste(p) if p.len() == 2000)));

        // Back over " please", then one more takes the whole paste.
        for _ in 0.." please".len() {
            line.backspace();
        }
        line.backspace();
        assert_eq!(line.flat(), "look at ");
        assert!(!line
            .segments()
            .iter()
            .any(|s| matches!(s, Segment::Paste(_))));
    }

    #[test]
    fn an_image_survives_editing_around_it() {
        let mut line = line_with("what is in ");
        line.push_opaque(Segment::Image {
            data: vec![0x89, 0x50, 0x4E, 0x47],
            mime_type: "image/png".to_string(),
            index: 1,
        });
        line.insert_str(" exactly");
        // Edit before the image without disturbing it.
        line.home();
        line.word_right();
        line.insert_str(" precisely");

        let images: Vec<&Segment> = line
            .segments()
            .iter()
            .filter(|s| matches!(s, Segment::Image { .. }))
            .collect();
        assert_eq!(images.len(), 1);
        assert!(line.flat().contains("precisely"));
        assert!(line.flat().contains("exactly"));
    }

    /* ---------------------------------------------------------------- */
    /* multi line                                                        */
    /* ---------------------------------------------------------------- */

    /// A trailing backslash, because Shift-Enter is not reported as
    /// distinct from Enter by most terminals.
    #[test]
    fn a_trailing_backslash_continues_the_line() {
        let mut line = line_with("first part \\");
        assert!(line.wants_continuation());
        line.continue_line();
        assert_eq!(line.flat(), "first part \n");
        assert!(!line.wants_continuation());

        line.insert_str("second part");
        assert_eq!(line.flat(), "first part \nsecond part");
    }

    #[test]
    fn a_line_without_a_backslash_is_sent() {
        assert!(!line_with("just a message").wants_continuation());
    }

    /* ---------------------------------------------------------------- */
    /* history                                                           */
    /* ---------------------------------------------------------------- */

    #[test]
    fn up_walks_back_and_down_returns_to_the_draft() {
        let mut history = History::new(vec!["first".to_string(), "second".to_string()]);

        assert_eq!(history.previous("half typed").as_deref(), Some("second"));
        assert_eq!(history.previous("half typed").as_deref(), Some("first"));
        // The far end is the far end.
        assert_eq!(history.previous("half typed"), None);

        assert_eq!(history.forward().as_deref(), Some("second"));
        // And back to what was being typed, not to an empty line.
        assert_eq!(history.forward().as_deref(), Some("half typed"));
        assert_eq!(history.forward(), None);
    }

    /// Pressing Up four times to get past four identical messages is not
    /// history, it is an obstacle.
    #[test]
    fn consecutive_duplicates_are_not_stored() {
        let mut history = History::new(Vec::new());
        std::env::set_var(HISTORY_ENV, "0");
        history.remember("same");
        history.remember("same");
        history.remember("different");
        history.remember("same");
        std::env::remove_var(HISTORY_ENV);

        assert_eq!(history.entries(), ["same", "different", "same"]);
    }

    #[test]
    fn an_empty_message_is_not_remembered() {
        let mut history = History::new(Vec::new());
        std::env::set_var(HISTORY_ENV, "0");
        history.remember("   ");
        history.remember("");
        std::env::remove_var(HISTORY_ENV);

        assert!(history.entries().is_empty());
    }

    #[test]
    fn history_is_capped() {
        let mut history = History::new(Vec::new());
        std::env::set_var(HISTORY_ENV, "0");
        for i in 0..(MAX_HISTORY + 50) {
            history.remember(&format!("message {i}"));
        }
        std::env::remove_var(HISTORY_ENV);

        assert_eq!(history.entries().len(), MAX_HISTORY);
        // The oldest went, not the newest.
        assert_eq!(
            history.entries().last().map(String::as_str),
            Some(format!("message {}", MAX_HISTORY + 49).as_str())
        );
    }

    /* ---------------------------------------------------------------- */
    /* completion                                                        */
    /* ---------------------------------------------------------------- */

    #[test]
    fn tab_completes_a_slash_command_and_a_capsule_name() {
        let commands = ["help", "diff", "status", "exit"];
        let capsules = vec!["reviewer".to_string(), "researcher".to_string()];

        assert_eq!(complete("/he", &commands, &capsules), vec!["/help"]);
        assert_eq!(
            complete("/re", &commands, &capsules),
            vec!["/researcher", "/reviewer"]
        );
        // Everything, for a bare slash.
        assert_eq!(complete("/", &commands, &capsules).len(), 6);
    }

    #[test]
    fn nothing_completes_outside_a_leading_slash_word() {
        let commands = ["help"];
        let capsules = vec![];

        assert!(complete("he", &commands, &capsules).is_empty());
        // Past the first word, this would be guessing at prose.
        assert!(complete("/help me", &commands, &capsules).is_empty());
        assert!(complete("", &commands, &capsules).is_empty());
    }

    #[test]
    fn tab_fills_in_as_much_as_every_candidate_shares() {
        let candidates = vec!["/researcher".to_string(), "/reviewer".to_string()];
        assert_eq!(common_prefix(&candidates), "/re");

        assert_eq!(common_prefix(&["/help".to_string()]), "/help");
        assert_eq!(common_prefix(&[]), "");
    }
}
