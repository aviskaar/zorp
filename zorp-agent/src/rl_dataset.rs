//! Turning RL Studio work logs into training data.
//!
//! The one rule this is arranged around (ADR 0063): the signal comes from the
//! person or from code, never from a model grading its own work. So the only
//! inputs here are a person's thumbs and the "better answer" they typed, plus
//! the verbatim prompt and answer. Nothing asks a model which answer is
//! better, and nothing here composes a sentence about the work: it selects and
//! pairs rows a person labelled, and that is all.
//!
//! Two shapes come out. An SFT set is `{prompt, completion}` pairs, where the
//! completion is the answer a person kept or the better answer they wrote. A
//! preference set is `{prompt, chosen, rejected}` triples, which need two
//! answers to the same prompt, so they come only from a row a person edited:
//! their edit is `chosen` and the model's original answer is `rejected`.

use crate::session::WorkLogRow;
use serde::{Deserialize, Serialize};

/// One supervised example: train the model to produce `completion` for
/// `prompt`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SftExample {
    pub prompt: String,
    pub completion: String,
}

/// One preference example: for `prompt`, `chosen` is preferred over
/// `rejected`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreferenceExample {
    pub prompt: String,
    pub chosen: String,
    pub rejected: String,
}

/// Both datasets a work log can produce, with the rows a person can read
/// before training on any of them.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DatasetBuild {
    pub sft: Vec<SftExample>,
    pub preference: Vec<PreferenceExample>,
}

impl DatasetBuild {
    pub fn sft_count(&self) -> usize {
        self.sft.len()
    }
    pub fn preference_count(&self) -> usize {
        self.preference.len()
    }
}

/// Build both datasets from work-log rows.
///
/// SFT: a row is kept when the person thumbed it up or edited it. The
/// completion is the edited answer when there is one, otherwise the original.
/// A thumbs-down with no edit is left out: it says the answer was wrong, not
/// what the right one was, and inventing one is exactly the model-authored
/// signal this avoids.
///
/// Preference: a row is a pair only when the person edited it and the edit
/// differs from the original, which is the one case where two answers to the
/// same prompt both exist and a person ranked them. The edit is `chosen`, the
/// original is `rejected`.
pub fn build(rows: &[WorkLogRow]) -> DatasetBuild {
    let mut out = DatasetBuild::default();
    for row in rows {
        let edited = row
            .edited_answer
            .as_deref()
            .map(str::trim)
            .filter(|e| !e.is_empty());
        let thumbs_up = row.feedback.as_deref() == Some("up");

        if let Some(edit) = edited {
            out.sft.push(SftExample {
                prompt: row.prompt.clone(),
                completion: edit.to_string(),
            });
            if edit != row.answer.trim() {
                out.preference.push(PreferenceExample {
                    prompt: row.prompt.clone(),
                    chosen: edit.to_string(),
                    rejected: row.answer.clone(),
                });
            }
        } else if thumbs_up {
            out.sft.push(SftExample {
                prompt: row.prompt.clone(),
                completion: row.answer.clone(),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::WorkLogToolCall;

    fn row(prompt: &str, answer: &str, feedback: Option<&str>, edited: Option<&str>) -> WorkLogRow {
        WorkLogRow {
            id: "x".into(),
            session_id: "s".into(),
            model: "m".into(),
            prompt: prompt.into(),
            answer: answer.into(),
            tool_calls: vec![WorkLogToolCall {
                name: "n".into(),
                status: "ok".into(),
            }],
            feedback: feedback.map(str::to_string),
            edited_answer: edited.map(str::to_string),
            created: 0,
        }
    }

    #[test]
    fn a_thumbs_up_becomes_an_sft_example_and_no_pair() {
        let build = build(&[row("q", "good answer", Some("up"), None)]);
        assert_eq!(
            build.sft,
            vec![SftExample {
                prompt: "q".into(),
                completion: "good answer".into()
            }]
        );
        assert!(build.preference.is_empty(), "one answer is not a pair");
    }

    #[test]
    fn an_edit_becomes_an_sft_completion_and_a_preference_pair() {
        let build = build(&[row("q", "meh answer", Some("down"), Some("great answer"))]);
        assert_eq!(build.sft[0].completion, "great answer");
        assert_eq!(build.preference.len(), 1);
        assert_eq!(build.preference[0].chosen, "great answer");
        assert_eq!(build.preference[0].rejected, "meh answer");
    }

    #[test]
    fn a_thumbs_down_with_no_edit_produces_nothing() {
        // It says the answer was wrong, not what the right one was. Inventing
        // one would be the model-authored signal ADR 0063 forbids.
        let build = build(&[row("q", "wrong", Some("down"), None)]);
        assert!(build.sft.is_empty());
        assert!(build.preference.is_empty());
    }

    #[test]
    fn an_edit_equal_to_the_answer_is_not_a_pair() {
        let build = build(&[row("q", "same", None, Some(" same "))]);
        assert_eq!(build.sft.len(), 1, "still a supervised example");
        assert!(build.preference.is_empty(), "no disagreement, no pair");
    }

    #[test]
    fn unlabelled_rows_are_ignored() {
        let build = build(&[row("q", "a", None, None)]);
        assert!(build.sft.is_empty());
        assert!(build.preference.is_empty());
    }
}
