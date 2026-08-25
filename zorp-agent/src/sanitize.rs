//! Text sanitization for the artifacts zorp writes.
//!
//! Model output routinely carries characters that break tooling and that
//! this repo's own style rules forbid. Two narrow jobs, done at the point
//! an artifact is written:
//!
//! 1. Remove invisible and formatting-only characters. They break `grep`,
//!    corrupt diffs, make two identical-looking strings compare unequal,
//!    and, in the case of the bidirectional overrides, let text render in
//!    an order that differs from its byte order.
//! 2. Normalize typographic punctuation to ASCII, which is the house
//!    style rule in `CLAUDE.md` applied mechanically instead of by review.
//!
//! The hard constraint is that legitimate non-ASCII text must survive
//! byte-identical. Every character this module touches is named in one of
//! the tables below; nothing here works off a range like "non-ASCII" or a
//! Unicode category. Two rules keep it honest:
//!
//! * A joiner or a variation selector is removed only where it cannot be
//!   doing orthographic work (see `joiner_is_load_bearing` and
//!   `selector_is_load_bearing`). Persian, Urdu and Devanagari spelling
//!   depend on joiners, and emoji sequences are built out of them.
//! * A dash, a curly quote or an ellipsis is normalized only where the
//!   text around it is ASCII (see `ascii_context`). The same characters
//!   are correct punctuation in other languages, where rewriting them
//!   would change how a sentence reads.
//!
//! Fenced code blocks are left alone entirely. Anything may legitimately
//! appear in one, and rewriting a quote or a dash there can turn working
//! code into broken code. Invisibles found inside a fence are counted and
//! reported rather than removed, so a hidden character in a code block is
//! still surfaced to the human.

/// Invisible formatting characters with no orthographic role in any
/// script. Removed wherever they appear outside a code block.
const ALWAYS_INVISIBLE: [char; 4] = [
    '\u{00AD}', // SOFT HYPHEN, a presentational hyphenation hint
    '\u{200B}', // ZERO WIDTH SPACE
    '\u{2060}', // WORD JOINER
    '\u{FEFF}', // ZERO WIDTH NO-BREAK SPACE, i.e. the BOM
];

/// Bidirectional embeddings, overrides and isolates: the set that lets
/// rendered order diverge from byte order (CVE-2021-42574, "trojan
/// source"). LRM, RLM and ALM are deliberately not here; see
/// `docs/DECISIONS.md`.
const BIDI_CONTROLS: [char; 9] = [
    '\u{202A}', // LEFT-TO-RIGHT EMBEDDING
    '\u{202B}', // RIGHT-TO-LEFT EMBEDDING
    '\u{202C}', // POP DIRECTIONAL FORMATTING
    '\u{202D}', // LEFT-TO-RIGHT OVERRIDE
    '\u{202E}', // RIGHT-TO-LEFT OVERRIDE
    '\u{2066}', // LEFT-TO-RIGHT ISOLATE
    '\u{2067}', // RIGHT-TO-LEFT ISOLATE
    '\u{2068}', // FIRST STRONG ISOLATE
    '\u{2069}', // POP DIRECTIONAL ISOLATE
];

/// VARIATION SELECTOR-1 through -16. Enumerated rather than written as a
/// range so the set stays auditable, and so it is obvious that the
/// variation selectors supplement (U+E0100 and up), which carries
/// Japanese ideographic variation sequences, is not included.
const VARIATION_SELECTORS: [char; 16] = [
    '\u{FE00}', '\u{FE01}', '\u{FE02}', '\u{FE03}', '\u{FE04}', '\u{FE05}', '\u{FE06}', '\u{FE07}',
    '\u{FE08}', '\u{FE09}', '\u{FE0A}', '\u{FE0B}', '\u{FE0C}', '\u{FE0D}', '\u{FE0E}', '\u{FE0F}',
];

const ZWNJ: char = '\u{200C}';
const ZWJ: char = '\u{200D}';
const EN_DASH: char = '\u{2013}';
const EM_DASH: char = '\u{2014}';
const ELLIPSIS: char = '\u{2026}';
const NBSP: char = '\u{00A0}';
const LEFT_SINGLE_QUOTE: char = '\u{2018}';
const RIGHT_SINGLE_QUOTE: char = '\u{2019}';
const LEFT_DOUBLE_QUOTE: char = '\u{201C}';
const RIGHT_DOUBLE_QUOTE: char = '\u{201D}';

/// How much of the sanitization pass to run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SanitizeMode {
    /// Strip invisible characters and normalize typographic punctuation.
    #[default]
    Full,
    /// Strip invisible characters only. Punctuation is left alone.
    Invisible,
    /// Do nothing.
    Off,
}

/// What a sanitization pass changed, counted per category.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SanitizeReport {
    pub invisible_removed: usize,
    pub bidi_removed: usize,
    pub dashes_normalized: usize,
    pub quotes_normalized: usize,
    pub ellipses_normalized: usize,
    pub nbsp_normalized: usize,
    pub invisible_left_in_code: usize,
}

impl SanitizeReport {
    /// A one-line, per-category account of what happened, or `None` when
    /// there is nothing to tell the human. Rewriting someone's document
    /// silently is not acceptable even when the rewrite is right, so every
    /// caller that writes an artifact reports this.
    pub fn summary(&self) -> Option<String> {
        let parts: Vec<String> = [
            (self.invisible_removed, "invisible removed"),
            (self.bidi_removed, "bidi controls removed"),
            (self.dashes_normalized, "dashes normalized"),
            (self.quotes_normalized, "quotes normalized"),
            (self.ellipses_normalized, "ellipses normalized"),
            (self.nbsp_normalized, "no-break spaces normalized"),
            (
                self.invisible_left_in_code,
                "invisible left inside code blocks",
            ),
        ]
        .iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, label)| format!("{n} {label}"))
        .collect();
        if parts.is_empty() {
            None
        } else {
            Some(parts.join(", "))
        }
    }
}

impl std::fmt::Display for SanitizeMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            SanitizeMode::Full => "full",
            SanitizeMode::Invisible => "invisible",
            SanitizeMode::Off => "off",
        })
    }
}

impl std::str::FromStr for SanitizeMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "full" => Ok(SanitizeMode::Full),
            "invisible" => Ok(SanitizeMode::Invisible),
            "off" => Ok(SanitizeMode::Off),
            other => Err(format!(
                "unknown sanitize mode \"{other}\", expected full, invisible, or off"
            )),
        }
    }
}

/// Sanitized text plus the report of what changed to produce it.
#[derive(Clone, Debug)]
pub struct Sanitized {
    pub text: String,
    pub report: SanitizeReport,
}

/// Sanitize `input`. Pure: no I/O, no globals, no allocation the caller
/// cannot see.
pub fn sanitize(input: &str, mode: SanitizeMode) -> Sanitized {
    let mut report = SanitizeReport::default();
    if mode == SanitizeMode::Off {
        return Sanitized {
            text: input.to_string(),
            report,
        };
    }

    let mut out = String::with_capacity(input.len());
    let mut open_fence: Option<Fence> = None;

    // split_inclusive keeps each line's terminator attached, so `\r\n` and
    // a missing final newline both survive a round trip.
    for line in input.split_inclusive('\n') {
        let (content, terminator) = split_terminator(line);
        match &open_fence {
            Some(fence) => {
                if closes_fence(content, fence) {
                    open_fence = None;
                }
                report.invisible_left_in_code += count_invisibles(content);
                out.push_str(line);
            }
            None => match opens_fence(content) {
                Some(fence) => {
                    open_fence = Some(fence);
                    report.invisible_left_in_code += count_invisibles(content);
                    out.push_str(line);
                }
                None => {
                    sanitize_line(content, mode, &mut out, &mut report);
                    out.push_str(terminator);
                }
            },
        }
    }

    Sanitized { text: out, report }
}

/// Split a line into its content and its line terminator.
fn split_terminator(line: &str) -> (&str, &str) {
    if let Some(rest) = line.strip_suffix("\r\n") {
        (rest, "\r\n")
    } else if let Some(rest) = line.strip_suffix('\n') {
        (rest, "\n")
    } else {
        (line, "")
    }
}

/// An open code fence: which marker opened it, and how long it was. A
/// fence closes only on a run of the same marker that is at least as long.
struct Fence {
    marker: char,
    len: usize,
}

/// The fence a line would be, if it is one at all.
fn fence_at(content: &str) -> Option<Fence> {
    let body = content.trim_start_matches(' ');
    if content.len() - body.len() > 3 {
        return None;
    }
    let marker = body.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let len = body.chars().take_while(|c| *c == marker).count();
    if len < 3 {
        return None;
    }
    Some(Fence { marker, len })
}

fn opens_fence(content: &str) -> Option<Fence> {
    let fence = fence_at(content)?;
    // CommonMark: a backtick fence's info string may not contain a
    // backtick, which is what keeps `` `a` `` from opening a block.
    if fence.marker == '`' && content.trim_start_matches(' ')[fence.len..].contains('`') {
        return None;
    }
    Some(fence)
}

fn closes_fence(content: &str, open: &Fence) -> bool {
    match fence_at(content) {
        Some(fence) => {
            fence.marker == open.marker
                && fence.len >= open.len
                && content.trim_start_matches(' ')[fence.len..]
                    .trim()
                    .is_empty()
        }
        None => false,
    }
}

/// Invisibles a normal line would have removed. Inside a fence they stay
/// put, but the human still gets told they are there.
fn count_invisibles(content: &str) -> usize {
    let chars: Vec<char> = content.chars().collect();
    (0..chars.len())
        .filter(|i| invisible_at(&chars, *i).is_some())
        .count()
}

/// Which counter an invisible character belongs to.
enum Invisible {
    Plain,
    Bidi,
}

/// Is the character at `i` an invisible with no work left to do here?
fn invisible_at(chars: &[char], i: usize) -> Option<Invisible> {
    let c = chars[i];
    if BIDI_CONTROLS.contains(&c) {
        return Some(Invisible::Bidi);
    }
    if ALWAYS_INVISIBLE.contains(&c) {
        return Some(Invisible::Plain);
    }
    if (c == ZWJ || c == ZWNJ) && !joiner_is_load_bearing(chars, i) {
        return Some(Invisible::Plain);
    }
    if VARIATION_SELECTORS.contains(&c) && !selector_is_load_bearing(chars, i) {
        return Some(Invisible::Plain);
    }
    None
}

/// A character this pass removes unconditionally. Neighbour lookups skip
/// these, because a decision that depended on a character about to
/// disappear would come out differently on a second pass.
fn always_removed(c: char) -> bool {
    ALWAYS_INVISIBLE.contains(&c) || BIDI_CONTROLS.contains(&c)
}

fn neighbour_before(chars: &[char], i: usize) -> Option<char> {
    chars[..i]
        .iter()
        .rev()
        .find(|c| !always_removed(**c))
        .copied()
}

fn neighbour_after(chars: &[char], i: usize) -> Option<char> {
    chars[i + 1..]
        .iter()
        .find(|c| !always_removed(**c))
        .copied()
}

/// A zero-width joiner or non-joiner joins the characters on either side
/// of it. That is real orthography in Persian, Urdu, Pashto and the Indic
/// scripts, and it is how emoji sequences are assembled, so it is kept
/// whenever both sides are non-ASCII. An ASCII neighbour means there is no
/// joining to do and the character is stray formatting.
fn joiner_is_load_bearing(chars: &[char], i: usize) -> bool {
    match (neighbour_before(chars, i), neighbour_after(chars, i)) {
        (Some(before), Some(after)) => !before.is_ascii() && !after.is_ascii(),
        _ => false,
    }
}

/// A variation selector modifies the single character before it. Emoji and
/// CJK bases are non-ASCII; the one family of ASCII bases is the keycap
/// sequences, `0`-`9`, `#` and `*`. Anywhere else it is stray formatting.
fn selector_is_load_bearing(chars: &[char], i: usize) -> bool {
    match neighbour_before(chars, i) {
        Some(before) => !before.is_ascii() || matches!(before, '#' | '*' | '0'..='9'),
        None => false,
    }
}

/// Punctuation is normalized only where the text around it is ASCII. The
/// same code points are correct punctuation elsewhere: a Russian sentence
/// uses an em dash where English uses "is", and Chinese uses the curly
/// double quotes as its own quotation marks. Rewriting those changes how
/// the sentence reads, which is a worse failure than leaving a dash alone.
fn ascii_context(chars: &[char], i: usize) -> bool {
    let ascii_side = |c: Option<char>| match c {
        Some(c) => c.is_ascii(),
        // Start or end of a line: nothing there to be non-ASCII.
        None => true,
    };
    ascii_side(prose_before(chars, i)) && ascii_side(prose_after(chars, i))
}

/// Characters the punctuation pass rewrites to ASCII.
fn normalization_target(c: char) -> bool {
    matches!(
        c,
        EN_DASH
            | EM_DASH
            | ELLIPSIS
            | NBSP
            | LEFT_SINGLE_QUOTE
            | RIGHT_SINGLE_QUOTE
            | LEFT_DOUBLE_QUOTE
            | RIGHT_DOUBLE_QUOTE
    )
}

/// Transparent when looking for the text around a mark: spacing, anything
/// this pass removes, and the other marks. Skipping the other marks is
/// what stops `he said "a lot"...` from protecting itself, where each mark
/// would otherwise see only its non-ASCII neighbour and none of them would
/// be normalized. They are all on their way to ASCII anyway.
fn transparent_to_context(c: char) -> bool {
    always_removed(c) || c == ' ' || c == '\t' || normalization_target(c)
}

fn prose_before(chars: &[char], i: usize) -> Option<char> {
    chars[..i]
        .iter()
        .rev()
        .find(|c| !transparent_to_context(**c))
        .copied()
}

fn prose_after(chars: &[char], i: usize) -> Option<char> {
    chars[i + 1..]
        .iter()
        .find(|c| !transparent_to_context(**c))
        .copied()
}

/// Is this dash one of a run of two or more? A doubled dash is a mark in
/// its own right, the CJK two-em dash or a hand-typed double, and it is
/// not the clause punctuation the house rule is about.
fn dash_in_a_run(chars: &[char], i: usize) -> bool {
    let is_dash = |c: Option<char>| matches!(c, Some(EN_DASH) | Some(EM_DASH));
    is_dash(neighbour_before(chars, i)) || is_dash(neighbour_after(chars, i))
}

/// What an en or em dash turns into.
enum DashForm {
    Hyphen,
    Period,
    Comma,
}

fn dash_form(chars: &[char], i: usize) -> DashForm {
    let spacing = |c: &char| *c == ' ' || *c == '\t';
    // Line-initial: an attribution or a hand-written bullet, not clause
    // punctuation. A hyphen is what that line meant.
    if chars[..i].iter().all(spacing) {
        return DashForm::Hyphen;
    }
    // Line-final: a trailing dash ends the line, so end it with a period.
    if chars[i + 1..].iter().all(spacing) {
        return DashForm::Period;
    }
    let before = chars[i - 1];
    let after = chars[i + 1];
    if before.is_ascii_digit() && after.is_ascii_digit() {
        return DashForm::Hyphen; // a range: 2019-2021
    }
    if chars[i] == EN_DASH && before.is_ascii_alphanumeric() && after.is_ascii_alphanumeric() {
        return DashForm::Hyphen; // a compound: Sino-Soviet
    }
    // Everything left is a dash separating clauses, where the house rule
    // asks for plain punctuation. A comma is the one substitution that
    // never needs the next word recapitalized.
    DashForm::Comma
}

fn trim_trailing_spacing(out: &mut String) {
    while out.ends_with(' ') || out.ends_with('\t') {
        out.pop();
    }
}

/// Sanitize one line of prose, appending to `out`.
fn sanitize_line(content: &str, mode: SanitizeMode, out: &mut String, report: &mut SanitizeReport) {
    let normalize_punctuation = mode == SanitizeMode::Full;
    let chars: Vec<char> = content.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        if let Some(kind) = invisible_at(&chars, i) {
            match kind {
                Invisible::Plain => report.invisible_removed += 1,
                Invisible::Bidi => report.bidi_removed += 1,
            }
            i += 1;
            continue;
        }

        if !normalize_punctuation {
            out.push(c);
            i += 1;
            continue;
        }

        // A no-break space needs no context check: it renders exactly like
        // a normal space, so a reader cannot tell them apart, and leaving
        // it in is the option that hides a difference.
        if c == NBSP {
            out.push(' ');
            report.nbsp_normalized += 1;
            i += 1;
            continue;
        }

        let contextual = matches!(
            c,
            EN_DASH
                | EM_DASH
                | ELLIPSIS
                | LEFT_SINGLE_QUOTE
                | RIGHT_SINGLE_QUOTE
                | LEFT_DOUBLE_QUOTE
                | RIGHT_DOUBLE_QUOTE
        );
        let is_dash = c == EN_DASH || c == EM_DASH;
        if !contextual || !ascii_context(&chars, i) || (is_dash && dash_in_a_run(&chars, i)) {
            out.push(c);
            i += 1;
            continue;
        }

        match c {
            EN_DASH | EM_DASH => {
                match dash_form(&chars, i) {
                    DashForm::Hyphen => out.push('-'),
                    DashForm::Period => {
                        trim_trailing_spacing(out);
                        out.push('.');
                    }
                    DashForm::Comma => {
                        trim_trailing_spacing(out);
                        out.push_str(", ");
                        while matches!(chars.get(i + 1), Some(' ') | Some('\t')) {
                            i += 1;
                        }
                    }
                }
                report.dashes_normalized += 1;
            }
            ELLIPSIS => {
                out.push_str("...");
                report.ellipses_normalized += 1;
            }
            LEFT_SINGLE_QUOTE | RIGHT_SINGLE_QUOTE => {
                out.push('\'');
                report.quotes_normalized += 1;
            }
            LEFT_DOUBLE_QUOTE | RIGHT_DOUBLE_QUOTE => {
                out.push('"');
                report.quotes_normalized += 1;
            }
            _ => unreachable!("contextual set and match arms are the same list"),
        }
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full(input: &str) -> Sanitized {
        sanitize(input, SanitizeMode::Full)
    }

    /// Assert the text came back exactly as it went in and nothing was
    /// counted. This is the shape every multilingual test uses.
    fn assert_untouched(input: &str) {
        let out = full(input);
        assert_eq!(
            out.text, input,
            "text must survive byte-identical, got {:?}",
            out.text
        );
        assert_eq!(
            out.report,
            SanitizeReport::default(),
            "nothing should have been counted for {input:?}"
        );
    }

    // ---- invisible and formatting-only characters ----

    #[test]
    fn zero_width_space_is_removed() {
        let out = full("hel\u{200B}lo");
        assert_eq!(out.text, "hello");
        assert_eq!(out.report.invisible_removed, 1);
    }

    #[test]
    fn word_joiner_soft_hyphen_and_bom_are_removed() {
        let out = full("\u{FEFF}co\u{00AD}oper\u{2060}ate");
        assert_eq!(out.text, "cooperate");
        assert_eq!(out.report.invisible_removed, 3);
    }

    #[test]
    fn stray_joiners_in_ascii_text_are_removed() {
        let out = full("gr\u{200D}ep and gr\u{200C}ep");
        assert_eq!(out.text, "grep and grep");
        assert_eq!(out.report.invisible_removed, 2);
    }

    #[test]
    fn stray_variation_selector_after_a_letter_is_removed() {
        let out = full("plain\u{FE0F} text");
        assert_eq!(out.text, "plain text");
        assert_eq!(out.report.invisible_removed, 1);
    }

    // ---- bidirectional controls ----

    #[test]
    fn bidi_overrides_embeddings_and_isolates_are_removed() {
        // The trojan-source set: LRE RLE PDF LRO RLO LRI RLI FSI PDI.
        let input =
            "a\u{202A}b\u{202B}c\u{202C}d\u{202D}e\u{202E}f\u{2066}g\u{2067}h\u{2068}i\u{2069}j";
        let out = full(input);
        assert_eq!(out.text, "abcdefghij");
        assert_eq!(out.report.bidi_removed, 9);
        assert_eq!(out.report.invisible_removed, 0);
    }

    #[test]
    fn bidi_marks_are_kept_because_they_cannot_reorder_text() {
        // LRM, RLM and ALM only resolve the direction of neighbouring
        // neutrals. Removing them visibly breaks correct mixed-direction
        // text, and they cannot produce the reordering hazard.
        assert_untouched("\u{200E}(x)\u{200F} \u{061C}");
    }

    // ---- typographic punctuation ----

    #[test]
    fn spaced_em_dash_between_clauses_becomes_a_comma() {
        let out = full("It worked \u{2014} eventually.");
        assert_eq!(out.text, "It worked, eventually.");
        assert_eq!(out.report.dashes_normalized, 1);
    }

    #[test]
    fn unspaced_em_dash_between_clauses_becomes_a_comma() {
        let out = full("He left\u{2014}and never came back.");
        assert_eq!(out.text, "He left, and never came back.");
    }

    #[test]
    fn en_dash_in_a_numeric_range_becomes_a_hyphen() {
        let out = full("pages 3\u{2013}4 of 2019\u{2013}2021");
        assert_eq!(out.text, "pages 3-4 of 2019-2021");
        assert_eq!(out.report.dashes_normalized, 2);
    }

    #[test]
    fn en_dash_in_a_compound_becomes_a_hyphen() {
        let out = full("the Sino\u{2013}Soviet split");
        assert_eq!(out.text, "the Sino-Soviet split");
    }

    #[test]
    fn line_initial_dash_becomes_a_hyphen() {
        let out = full("A quote.\n\u{2014} Someone\n");
        assert_eq!(out.text, "A quote.\n- Someone\n");
    }

    #[test]
    fn line_final_dash_becomes_a_period() {
        let out = full("He started to say \u{2014}\nthen stopped.");
        assert_eq!(out.text, "He started to say.\nthen stopped.");
    }

    #[test]
    fn curly_quotes_become_straight() {
        let out = full("\u{201C}don\u{2019}t\u{201D} said \u{2018}x\u{2019}");
        assert_eq!(out.text, "\"don't\" said 'x'");
        assert_eq!(out.report.quotes_normalized, 5);
    }

    #[test]
    fn ellipsis_becomes_three_periods() {
        let out = full("Wait\u{2026} what?");
        assert_eq!(out.text, "Wait... what?");
        assert_eq!(out.report.ellipses_normalized, 1);
    }

    #[test]
    fn no_break_space_becomes_a_normal_space() {
        let out = full("10\u{00A0}ms");
        assert_eq!(out.text, "10 ms");
        assert_eq!(out.report.nbsp_normalized, 1);
    }

    #[test]
    fn adjacent_marks_do_not_protect_each_other() {
        // Each mark's nearest neighbour is another mark, which is not
        // ASCII. Looking past them is what keeps this from being a run of
        // marks that all decline to be normalized.
        let out = full("It improved \u{2014} \u{201C}a lot\u{201D}\u{2026}");
        assert_eq!(out.text, "It improved, \"a lot\"...");
        assert_eq!(out.report.dashes_normalized, 1);
        assert_eq!(out.report.quotes_normalized, 2);
        assert_eq!(out.report.ellipses_normalized, 1);
    }

    #[test]
    fn a_doubled_dash_is_left_alone() {
        // Two dashes in a row are a mark in their own right, not the
        // clause punctuation the house rule is about.
        assert_untouched("a\u{2014}\u{2014}b and c \u{2013}\u{2013} d");
    }

    #[test]
    fn other_dashes_and_the_minus_sign_are_left_alone() {
        // Figure dash, horizontal bar, minus sign: not in the enumerated
        // set, and the minus sign is arithmetic, not punctuation.
        assert_untouched("5 \u{2212} 3, \u{2012}, \u{2015}");
    }

    #[test]
    fn language_specific_quotes_are_left_alone() {
        // Guillemets, German low quotes, CJK corner brackets: these are
        // the correct mark in some language, not a curled ASCII quote.
        assert_untouched("\u{00AB}x\u{00BB} \u{201E}y\u{201F} \u{300C}z\u{300D}");
    }

    // ---- multilingual preservation ----

    #[test]
    fn hindi_survives_byte_identical() {
        assert_untouched("नमस्ते दुनिया। यह एक परीक्षण है।");
    }

    #[test]
    fn devanagari_joiner_survives() {
        // ZWJ after a virama forces the joined conjunct form. Removing it
        // spells the word differently.
        assert_untouched("क्\u{200D}ष and क्\u{200C}ष");
    }

    #[test]
    fn chinese_survives_byte_identical() {
        assert_untouched("这是一个测试，用于验证文本处理。");
    }

    #[test]
    fn chinese_quotes_and_dashes_are_left_alone() {
        assert_untouched("他说\u{201C}你好\u{201D}\u{2014}\u{2014}然后走了。");
    }

    #[test]
    fn arabic_survives_byte_identical() {
        assert_untouched("مرحبا بالعالم، هذا اختبار.");
    }

    #[test]
    fn persian_zwnj_survives() {
        // Persian orthography requires ZWNJ here. Strip it and the words
        // are misspelled.
        assert_untouched("می\u{200C}رود و کتاب\u{200C}ها");
    }

    #[test]
    fn accented_french_survives_byte_identical() {
        assert_untouched("Déjà vu: une élève très âgée est allée à l'hôtel.");
    }

    #[test]
    fn cyrillic_survives_byte_identical() {
        assert_untouched("Привет, мир! Это проверка.");
    }

    #[test]
    fn russian_copula_dash_is_left_alone() {
        // The dash stands in for the verb here. A comma would change the
        // sentence.
        assert_untouched("Москва \u{2014} столица России.");
    }

    #[test]
    fn emoji_zwj_sequences_survive() {
        assert_untouched("👨\u{200D}👩\u{200D}👧 and 🏳\u{FE0F}\u{200D}🌈 and ❤\u{FE0F}");
    }

    #[test]
    fn keycap_emoji_survives() {
        assert_untouched("1\u{FE0F}\u{20E3} #\u{FE0F}\u{20E3} *\u{FE0F}\u{20E3}");
    }

    #[test]
    fn mathematical_symbols_survive() {
        assert_untouched("∑ x² ≤ 5 ± 1, π ≈ 3.14, ∀x ∈ ℝ");
    }

    // ---- fenced code blocks ----

    #[test]
    fn fenced_code_block_is_not_sanitized() {
        let input = "before \u{2014} after\n```rust\nlet s = \"a\u{2014}b\u{201C}c\";\n```\nafter \u{2014} here\n";
        let out = full(input);
        assert_eq!(
            out.text,
            "before, after\n```rust\nlet s = \"a\u{2014}b\u{201C}c\";\n```\nafter, here\n"
        );
        assert_eq!(out.report.dashes_normalized, 2);
        assert_eq!(out.report.quotes_normalized, 0);
    }

    #[test]
    fn tilde_fence_is_not_sanitized() {
        let input = "~~~\na \u{2014} b\n~~~\nc \u{2014} d\n";
        let out = full(input);
        assert_eq!(out.text, "~~~\na \u{2014} b\n~~~\nc, d\n");
        assert_eq!(out.report.dashes_normalized, 1);
    }

    #[test]
    fn a_backtick_fence_does_not_close_a_tilde_fence() {
        let input = "~~~\na \u{2014} b\n```\nc \u{2014} d\n";
        let out = full(input);
        assert_eq!(out.text, input);
    }

    #[test]
    fn unterminated_fence_protects_the_rest_of_the_document() {
        let input = "ok \u{2014} yes\n```\nnever \u{2014} closed\n";
        let out = full(input);
        assert_eq!(out.text, "ok, yes\n```\nnever \u{2014} closed\n");
    }

    #[test]
    fn invisible_characters_inside_code_are_left_but_reported() {
        let input = "```\nlet x = \"a\u{200B}b\";\nlet y = \"\u{202E}oof\";\n```\n";
        let out = full(input);
        assert_eq!(out.text, input, "code must not be rewritten");
        assert_eq!(out.report.invisible_left_in_code, 2);
        assert_eq!(out.report.invisible_removed, 0);
        assert_eq!(out.report.bidi_removed, 0);
    }

    // ---- properties ----

    /// Inputs that exercise every branch, including the ones where a
    /// removal could change a later decision.
    fn corpus() -> Vec<String> {
        [
            "",
            "plain ascii text with no targets at all",
            "hel\u{200B}lo",
            "\u{FEFF}\u{00AD}\u{2060}",
            "a\u{202E}b\u{2069}c",
            "It worked \u{2014} eventually.",
            "pages 3\u{2013}4",
            "\u{201C}don\u{2019}t\u{201D}",
            "Wait\u{2026} what?",
            "10\u{00A0}ms",
            "Москва \u{2014} столица России.",
            "这是\u{2014}\u{2014}一个测试",
            "می\u{200C}رود",
            "👨\u{200D}👩\u{200D}👧",
            "1\u{FE0F}\u{20E3}",
            // A joiner whose neighbour only looks non-ASCII because an
            // unconditionally removed character sits between it and an
            // ASCII letter.
            "क\u{200D}\u{200B}a",
            "a\u{200D}\u{200B}क",
            // A dash whose neighbour is separated by an invisible.
            "Москва \u{200B}\u{2014} столица",
            "a \u{200B}\u{2014} b",
            "\u{2014} leading\n\u{2014}\ntrailing \u{2014}",
            "```\nfenced \u{2014} code\n```\nprose \u{2014} here",
            "~~~\nfenced \u{2014} code\n",
            "a\u{2014}\u{2014}b",
            "a\u{2014}\u{200B}\u{2014}b",
            "It improved \u{2014} \u{201C}a lot\u{201D}\u{2026}",
            "中 \u{2014} \u{201C}b\u{201D}",
            "a \u{201C} \u{201D} 中",
            "10\u{00A0}\u{2014} ms",
            "\u{2014}",
            " \u{2014} ",
            "\u{FE0F}",
            "#\u{FE0F}\u{20E3}",
            "line one\r\nline \u{2014} two\r\n",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    #[test]
    fn sanitizing_twice_equals_sanitizing_once() {
        for input in corpus() {
            for mode in [SanitizeMode::Full, SanitizeMode::Invisible] {
                let once = sanitize(&input, mode);
                let twice = sanitize(&once.text, mode);
                assert_eq!(
                    twice.text, once.text,
                    "not idempotent for {input:?} in {mode:?}"
                );
            }
        }
    }

    #[test]
    fn a_second_pass_changes_nothing_and_counts_nothing() {
        for input in corpus() {
            let cleaned = full(&input).text;
            // A pass leaves no targets behind outside code blocks, so a
            // second pass must be the identity on everything it can touch.
            let again = full(&cleaned);
            assert_eq!(again.text, cleaned, "identity failed for {input:?}");
            assert_eq!(again.report.invisible_removed, 0, "for {input:?}");
            assert_eq!(again.report.bidi_removed, 0, "for {input:?}");
            assert_eq!(again.report.dashes_normalized, 0, "for {input:?}");
            assert_eq!(again.report.quotes_normalized, 0, "for {input:?}");
            assert_eq!(again.report.ellipses_normalized, 0, "for {input:?}");
            assert_eq!(again.report.nbsp_normalized, 0, "for {input:?}");
        }
    }

    #[test]
    fn line_endings_and_trailing_newline_are_preserved() {
        let out = full("a \u{2014} b\r\nc\r\n");
        assert_eq!(out.text, "a, b\r\nc\r\n");
        assert_eq!(full("no trailing newline").text, "no trailing newline");
    }

    // ---- modes and reporting ----

    #[test]
    fn off_mode_is_the_identity() {
        let input = "a \u{2014} b\u{200B}c\u{202E}d";
        let out = sanitize(input, SanitizeMode::Off);
        assert_eq!(out.text, input);
        assert_eq!(out.report, SanitizeReport::default());
    }

    #[test]
    fn invisible_mode_strips_but_leaves_punctuation() {
        let input = "a \u{2014} b\u{200B}c\u{202E}d \u{201C}q\u{201D}";
        let out = sanitize(input, SanitizeMode::Invisible);
        assert_eq!(out.text, "a \u{2014} bcd \u{201C}q\u{201D}");
        assert_eq!(out.report.invisible_removed, 1);
        assert_eq!(out.report.bidi_removed, 1);
        assert_eq!(out.report.dashes_normalized, 0);
        assert_eq!(out.report.quotes_normalized, 0);
    }

    #[test]
    fn a_report_with_nothing_in_it_has_no_summary() {
        assert!(SanitizeReport::default().summary().is_none());
    }

    #[test]
    fn modes_round_trip_through_their_names() {
        for mode in [
            SanitizeMode::Full,
            SanitizeMode::Invisible,
            SanitizeMode::Off,
        ] {
            assert_eq!(mode.to_string().parse::<SanitizeMode>(), Ok(mode));
        }
        assert!("loud".parse::<SanitizeMode>().is_err());
        assert_eq!("  OFF ".parse::<SanitizeMode>(), Ok(SanitizeMode::Off));
    }

    #[test]
    fn a_summary_names_every_category_that_fired() {
        let out = full("a\u{200B} \u{2014} b \u{201C}c\u{201D} d\u{2026} e\u{00A0}f\u{202E}g");
        let summary = out.report.summary().expect("something changed");
        assert!(summary.contains("invisible"), "{summary}");
        assert!(summary.contains("bidi"), "{summary}");
        assert!(summary.contains("dash"), "{summary}");
        assert!(summary.contains("quote"), "{summary}");
        assert!(summary.contains("ellips"), "{summary}");
        assert!(summary.contains("no-break space"), "{summary}");
    }
}
