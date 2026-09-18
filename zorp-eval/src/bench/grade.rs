//! Prompts and graders, one per answer format, all of them code.
//!
//! No model is asked what it scored and no model grades another. A reply is
//! read for one thing, the answer it commits to on its last `Answer:` line,
//! and that is compared with the key. A reply that commits to nothing
//! readable is scored as wrong and counted as `unparsed`, so a table can say
//! how much of a low score is a model that did not follow the format rather
//! than one that answered badly. A reply that never arrived is not graded
//! here at all: it is unevaluable, and `client.rs` decides that before a
//! grader sees anything.

use super::dataset::{letter, Item, Key};

/// The prompt for one item. Zero-shot and the same for every runtime, which
/// is what makes the rows in one table comparable with each other, and also
/// why they are not comparable with a published number that used five shots
/// and a different grader.
pub fn prompt(item: &Item) -> String {
    match &item.key {
        Key::Choice { options, .. } => {
            let last = letter(options.len() - 1);
            let mut text = format!(
                "Answer the following multiple choice question. Think it through if you need to, \
                 then end your reply with a line of the form \"Answer: X\", where X is one of \
                 the letters A to {last}.\n\n{}\n",
                item.question
            );
            for (i, option) in options.iter().enumerate() {
                text.push_str(&format!("\n{}. {}", letter(i), option.trim()));
            }
            text
        }
        Key::Number(_) => format!(
            "Solve the following problem. Think it through if you need to, then end your reply \
             with a line of the form \"Answer: N\", where N is the final number only.\n\n{}",
            item.question
        ),
    }
}

/// What a grader made of one reply.
#[derive(Debug, Clone, PartialEq)]
pub struct Graded {
    pub correct: bool,
    /// The answer read out of the reply, as a letter or a number. `None`
    /// means nothing could be read, which is scored as wrong.
    pub extracted: Option<String>,
}

pub fn grade(item: &Item, reply: &str) -> Graded {
    match &item.key {
        Key::Choice { options, correct } => {
            let got = extract_choice(reply, options.len());
            Graded {
                correct: got == Some(*correct),
                extracted: got.map(|i| letter(i).to_string()),
            }
        }
        Key::Number(want) => {
            let got = extract_number(reply);
            let correct = match (&got, parse_number(want)) {
                (Some(got), Some(want)) => {
                    parse_number(got).is_some_and(|got| (got - want).abs() < 1e-6)
                }
                _ => false,
            };
            Graded {
                correct,
                extracted: got,
            }
        }
    }
}

/// The option a reply commits to.
///
/// The last `answer` in the reply that is followed by a letter in range wins,
/// because a model that reasons first says "the answer depends on" long
/// before it says "Answer: C". Failing that, a reply that is nothing but a
/// letter is that letter. Nothing else is guessed at: picking the last
/// capital letter in a paragraph would grade prose, not answers.
pub fn extract_choice(reply: &str, options: usize) -> Option<usize> {
    let in_range = |c: char| -> Option<usize> {
        let i = (c as u32).checked_sub('A' as u32)? as usize;
        (c.is_ascii_uppercase() && i < options).then_some(i)
    };
    for rest in after_answer_markers(reply) {
        let rest = rest.trim_start_matches(|c: char| {
            c.is_whitespace() || matches!(c, ':' | '*' | '(' | '[' | '=' | '-' | '"' | '\'')
        });
        let rest = rest.strip_prefix("is").map_or(rest, |r| {
            r.trim_start_matches(|c: char| c.is_whitespace() || matches!(c, ':' | '*' | '(' | '['))
        });
        let mut chars = rest.chars();
        if let Some(i) = chars.next().and_then(in_range) {
            if chars.next().is_none_or(|c| !c.is_ascii_alphanumeric()) {
                return Some(i);
            }
        }
    }
    let bare = reply.trim().trim_matches(|c: char| {
        c.is_whitespace() || matches!(c, '*' | '(' | ')' | '.' | '[' | ']')
    });
    let mut chars = bare.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => in_range(c),
        _ => None,
    }
}

/// The number a reply commits to: the first number after its last `answer`
/// marker that has one, then after a `####` in GSM8K's own style, and
/// failing both the last number in the reply, which is the usual flexible
/// reading for GSM8K and the one a reply that ends "... so she makes $18."
/// needs.
pub fn extract_number(reply: &str) -> Option<String> {
    for rest in after_answer_markers(reply) {
        if let Some(n) = numbers(rest).into_iter().next() {
            return Some(n);
        }
    }
    if let Some((_, rest)) = reply.rsplit_once("####") {
        if let Some(n) = numbers(rest).into_iter().next() {
            return Some(n);
        }
    }
    numbers(reply).pop()
}

/// A number as the key and the extractor both write it: commas and a leading
/// `$` dropped, a trailing `.` ignored.
pub fn parse_number(text: &str) -> Option<f64> {
    let cleaned: String = text
        .trim()
        .trim_start_matches('$')
        .trim_end_matches('.')
        .chars()
        .filter(|c| *c != ',')
        .collect();
    cleaned.parse::<f64>().ok().filter(|n| n.is_finite())
}

/// The text after each case-insensitive `answer`, last occurrence first.
fn after_answer_markers(reply: &str) -> impl Iterator<Item = &str> {
    let lower = reply.to_ascii_lowercase();
    let mut starts: Vec<usize> = lower.match_indices("answer").map(|(i, _)| i + 6).collect();
    starts.reverse();
    starts.into_iter().map(move |i| &reply[i..])
}

/// Every number in `text`, in order, normalised: `1,234.50` is `1234.50`.
fn numbers(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let starts_number = bytes[i].is_ascii_digit()
            || (bytes[i] == b'-'
                && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)
                && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric()));
        if !starts_number {
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        while i < bytes.len() {
            let b = bytes[i];
            let joins_digits = matches!(b, b',' | b'.')
                && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)
                && bytes[i - 1].is_ascii_digit();
            if b.is_ascii_digit() || joins_digits {
                i += 1;
            } else {
                break;
            }
        }
        out.push(text[start..i].replace(',', ""));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choice(correct: usize) -> Item {
        Item {
            id: "0".into(),
            question: "Which invented option is right?".into(),
            key: Key::Choice {
                options: vec!["w".into(), "x".into(), "y".into(), "z".into()],
                correct,
            },
        }
    }

    #[test]
    fn a_choice_is_read_from_the_last_answer_line() {
        assert_eq!(extract_choice("Answer: C", 4), Some(2));
        assert_eq!(
            extract_choice("The answer depends on x.\nAnswer: (B)", 4),
            Some(1)
        );
        assert_eq!(extract_choice("**Answer:** D", 4), Some(3));
        assert_eq!(extract_choice("so the answer is A.", 4), Some(0));
        assert_eq!(extract_choice("B", 4), Some(1));
        assert_eq!(
            extract_choice("(b)", 4),
            None,
            "lowercase is not a letter answer"
        );
        // Out of range, or a word that starts with a capital, is not a choice.
        assert_eq!(extract_choice("Answer: E", 4), None);
        assert_eq!(extract_choice("Answer: Because", 4), None);
        assert_eq!(extract_choice("I think it is probably C or D", 4), None);
    }

    #[test]
    fn a_choice_grader_scores_against_the_key() {
        let graded = grade(&choice(2), "Reasoning.\nAnswer: C");
        assert!(graded.correct);
        assert_eq!(graded.extracted.as_deref(), Some("C"));
        let graded = grade(&choice(2), "Answer: A");
        assert!(!graded.correct);
        let graded = grade(&choice(2), "I would rather not say.");
        assert!(!graded.correct);
        assert_eq!(graded.extracted, None);
    }

    #[test]
    fn a_number_is_read_after_the_answer_marker_then_anywhere() {
        assert_eq!(
            extract_number("9 eggs, $2 each.\nAnswer: 18").as_deref(),
            Some("18")
        );
        assert_eq!(extract_number("Answer: $1,234.").as_deref(), Some("1234"));
        assert_eq!(extract_number("so it is -3").as_deref(), Some("-3"));
        assert_eq!(extract_number("#### 72").as_deref(), Some("72"));
        assert_eq!(
            extract_number("16 - 3 - 4 = 9, times 2 is 18.").as_deref(),
            Some("18")
        );
        assert_eq!(extract_number("no idea"), None);
        assert_eq!(extract_number("item-3").as_deref(), Some("3"));
    }

    #[test]
    fn a_number_grader_compares_values_not_strings() {
        let item = Item {
            id: "0".into(),
            question: "q".into(),
            key: Key::Number("18".into()),
        };
        assert!(grade(&item, "Answer: 18.00").correct);
        assert!(grade(&item, "Answer: $18").correct);
        assert!(!grade(&item, "Answer: 17").correct);
    }

    #[test]
    fn a_prompt_lists_every_option_with_its_letter() {
        let text = prompt(&choice(0));
        assert!(text.contains("\nA. w") && text.contains("\nD. z"), "{text}");
        assert!(text.contains("A to D"), "{text}");
    }
}
