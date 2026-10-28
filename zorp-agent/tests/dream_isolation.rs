//! Nothing a model wrote in Dreaming mode is ever read back as evidence.
//!
//! A suggestion's `wording` is the one model-authored string the feature
//! stores, and ADR 0063 is the rule it has to keep: the agent's guesses must
//! never become its own evidence. The four things that read a person's history
//! back to a model all read `messages` or `task` (the recall feed embeds them,
//! the memory block quotes them, titling reads the first pair, branching copies
//! them), so a suggestion stays out of evidence exactly by staying out of those
//! columns and out of the recall index built over them.
//!
//! This is the `there_is_one_chunker_and_one_fingerprint` of Dreaming mode:
//! one runtime check that the wording never lands in `messages` or `task`, and
//! one source scan that the recall and memory code never reads the suggestions
//! table at all.

use std::path::PathBuf;

use zorp_agent::dream;
use zorp_agent::{Message, Store, SuggestionDraft};

/// A string no real conversation would contain, so finding it anywhere a model
/// is fed would be unambiguous.
const SENTINEL: &str = "DREAM-SENTINEL-8f3a2c-model-authored-wording";

#[test]
fn suggestion_wording_never_reaches_messages_or_task() {
    let mut store = Store::open_in_memory().unwrap();
    // A conversation whose own messages are ordinary, filed as the evidence a
    // suggestion is drawn from.
    store
        .create_session("conv-a", "the monday report", "/r", "m")
        .unwrap();
    store
        .record_message("conv-a", 0, &Message::user("please run the monday report"))
        .unwrap();
    store
        .record_message("conv-a", 1, &Message::assistant("done"))
        .unwrap();

    // A suggestion whose wording carries the sentinel.
    store
        .record_suggestion(&SuggestionDraft {
            id: "sug-1".into(),
            kind: "recurring_request".into(),
            evidence_ids: vec!["conv-a".into()],
            wording: format!("Want a flavor for this? {SENTINEL}"),
        })
        .unwrap();

    // The wording is retrievable as a suggestion.
    let row = store.suggestion("sug-1").unwrap().unwrap();
    assert!(row.wording.contains(SENTINEL));

    // But it is in none of the message text the recall feed, the memory block,
    // titling and branching all read.
    let messages = store.load_messages("conv-a").unwrap();
    for message in &messages {
        assert!(
            !message.text().contains(SENTINEL),
            "the suggestion wording reached a stored message"
        );
    }
    // And not in `task`, which the recall feed indexes and the memory block
    // quotes as the conversation's title.
    for session in store.sessions().unwrap() {
        assert!(
            !session.task.contains(SENTINEL),
            "the suggestion wording reached a session's task"
        );
    }
}

/// The recall and memory code never names the suggestions table, so there is no
/// path by which a suggestion could enter the index a turn reads. A source scan
/// rather than a runtime check because the index itself needs a local embedder
/// the test host does not have, and the guarantee is structural anyway.
#[test]
fn the_recall_and_memory_code_never_reads_suggestions() {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    for module in ["recall.rs", "memory.rs"] {
        let path = src.join(module);
        if !path.exists() {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            !mentions_suggestions(&text),
            "{module} references the suggestions table; a suggestion could reach the index a turn reads"
        );
    }
}

/// True if the source reads the suggestions store in any form. Deliberately
/// broad: the table name, the row type, and the reader methods.
fn mentions_suggestions(text: &str) -> bool {
    for needle in [
        "suggestions",
        "SuggestionRow",
        "SuggestionDraft",
        "list_suggestions",
        "record_suggestion",
    ] {
        if text.contains(needle) {
            return true;
        }
    }
    false
}

/// A dismissed suggestion's pattern is not raised again, so dismissing is a
/// real decision and not a card that comes back next pass. This guards the
/// dedup the pass relies on, through the public surface.
#[test]
fn a_dismissed_pattern_does_not_come_back() {
    let store = Store::open_in_memory().unwrap();
    // Filed under a project, so the only pattern that fires is the recurring
    // request: an unfiled cluster is skipped once the conversations have a
    // project, which keeps this about one pattern and its dedup.
    let records: Vec<dream::ConvRecord> = ["a", "b", "c"]
        .iter()
        .map(|id| dream::ConvRecord {
            session_id: (*id).into(),
            created: 0,
            project_id: Some("p".into()),
            first_user_message: "weekly digest".into(),
            tool_events: vec![],
        })
        .collect();

    let first = dream::run_pass(&store, &records, &dream::TemplatePhraser, 10).unwrap();
    assert_eq!(first.len(), 1, "one recurring pattern should land");
    for id in &first {
        store.set_suggestion_state(id, "dismissed").unwrap();
    }
    let again = dream::run_pass(&store, &records, &dream::TemplatePhraser, 10).unwrap();
    assert!(again.is_empty(), "a dismissed pattern was raised again");
}
