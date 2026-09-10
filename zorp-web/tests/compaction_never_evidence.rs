//! A model-written summary is never evidence.
//!
//! This is the file the amended decisions rest on. `docs/DECISIONS.md`
//! (2026-08-19 and 2026-09-03) said no model writes a summary of the
//! conversation, and one of the reasons was that a summary in the record
//! would come back as though somebody had said it. Summaries exist now, and
//! the protection is structural: they live in the `compactions` table and
//! never in `messages`.
//!
//! Four things read `messages`, and each one gets a test here. The recall
//! feed embeds user and assistant rows into the search index. The memory
//! block quotes them into a later turn and tells the model to cite them.
//! Titling reads the first pair. Branching copies them. A summary reaching
//! any of the four would be the agent's own sentence arriving back as
//! evidence, which is exactly the failure this design is arranged against.
//!
//! Each test records a compaction whose summary contains a phrase that
//! appears nowhere else, and then asks the reader in question whether it
//! can see it.

use zorp_agent::{Compaction, Message, Store};

/// A phrase that is in the summary and in nothing else, so finding it
/// anywhere is proof it leaked out of the table.
const ONLY_IN_THE_SUMMARY: &str = "zzq-summary-only-marker-9f2b";

fn summary() -> String {
    let mut out = String::new();
    for section in zorp_agent::compaction::SECTIONS {
        out.push_str(&format!("## {section}\n"));
        if *section == "All user messages" {
            out.push_str(&format!("1. {ONLY_IN_THE_SUMMARY}\n"));
        } else {
            out.push_str("None.\n");
        }
        out.push('\n');
    }
    out
}

/// A conversation with a compaction recorded over its first exchange.
fn seeded(db: &std::path::Path) -> Store {
    let mut store = Store::open_at(db).unwrap();
    store
        .create_session("conv", "deploy the billing service", "repo", "model")
        .unwrap();
    store
        .record_message("conv", 0, &Message::user("deploy the billing service"))
        .unwrap();
    store
        .record_message(
            "conv",
            1,
            &Message::assistant("the port 8642 binding times out"),
        )
        .unwrap();
    store
        .record_message("conv", 2, &Message::user("what about the weekend job"))
        .unwrap();
    store
        .record_message("conv", 3, &Message::assistant("it fails on saturdays"))
        .unwrap();
    store
        .record_compaction(
            "conv",
            &Compaction {
                id: 0,
                boundary_seq: 1,
                summary: summary(),
                focus: None,
                model: "m".to_string(),
                tokens_before: 9000,
                tokens_after: 1200,
                manual: false,
                created: 0,
            },
        )
        .unwrap();
    store
}

/// The transcript on disk is untouched, and that is the other half of the
/// promise: the summary is not in it, and neither is anything missing from
/// it.
#[test]
fn the_stored_transcript_holds_every_message_and_no_summary() {
    let dir = tempfile::tempdir().unwrap();
    let store = seeded(&dir.path().join("sessions.db"));

    let texts: Vec<String> = store
        .load_messages("conv")
        .unwrap()
        .iter()
        .map(|m| m.text().into_owned())
        .collect();

    assert_eq!(texts.len(), 4, "compaction changed the record: {texts:?}");
    assert!(
        !texts.iter().any(|t| t.contains(ONLY_IN_THE_SUMMARY)),
        "the summary is in the transcript: {texts:?}"
    );
    // And it really was recorded, so the assertions above are about a
    // conversation that has one rather than about one that has none.
    assert_eq!(store.compactions("conv").unwrap().len(), 1);
}

/// Titling reads the first question and the first answer. A summary in
/// either would be a model naming a conversation from another model's
/// sentence about it.
#[test]
fn the_titling_call_never_sees_a_summary() {
    let dir = tempfile::tempdir().unwrap();
    let store = seeded(&dir.path().join("sessions.db"));

    let messages = store.load_messages("conv").unwrap();
    let question = messages[0].text().into_owned();
    let answer = messages[1].text().into_owned();
    let prompt = zorp_web::title::prompt(&question, &answer);
    let sent: String = prompt
        .iter()
        .map(|m| m.text().into_owned())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        !sent.contains(ONLY_IN_THE_SUMMARY),
        "the titling prompt carried the summary: {sent}"
    );
}

/// A branch copies stored messages. The compaction goes with it as a
/// compaction, and never as a message the branch will replay or embed.
#[test]
fn a_branch_carries_the_summary_as_a_compaction_and_not_as_a_message() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = seeded(&dir.path().join("sessions.db"));

    assert!(store.branch_session("conv", 1, "branch").unwrap());

    let texts: Vec<String> = store
        .load_messages("branch")
        .unwrap()
        .iter()
        .map(|m| m.text().into_owned())
        .collect();
    assert!(
        !texts.iter().any(|t| t.contains(ONLY_IN_THE_SUMMARY)),
        "the branch has the summary as a message: {texts:?}"
    );
    assert_eq!(
        store.compactions("branch").unwrap().len(),
        1,
        "the branch lost the compaction it was entitled to"
    );
}

/// What the recall feed would embed. It chunks user and assistant messages
/// out of the store, and the store has no summary in those, so the index
/// cannot acquire one.
#[cfg(feature = "recall")]
#[test]
fn the_recall_feed_embeds_no_summary() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("sessions.db");
    std::env::set_var("ZORP_STATE_DB", &db);
    std::env::set_var("ZORP_RECALL_DB", dir.path().join("recall.db"));
    let store = seeded(&db);

    // The same rows `index_one` chunks: user and assistant messages, from
    // the store, and nothing else.
    let embedded: Vec<String> = store
        .load_messages("conv")
        .unwrap()
        .iter()
        .filter(|m| m.role == "user" || m.role == "assistant")
        .map(|m| m.text().into_owned())
        .collect();

    assert!(!embedded.is_empty(), "nothing would be indexed at all");
    assert!(
        !embedded.iter().any(|t| t.contains(ONLY_IN_THE_SUMMARY)),
        "the feed would embed the summary: {embedded:?}"
    );
}

/// What the memory block would quote. It is built from passages, and a
/// passage is a stored message: there is no path from `compactions` into
/// one.
#[cfg(feature = "memory")]
#[test]
fn the_memory_block_quotes_no_summary() {
    let dir = tempfile::tempdir().unwrap();
    let store = seeded(&dir.path().join("sessions.db"));

    // Every stored message, offered to the block as though every one of
    // them had matched. If a summary could reach a block, this is the
    // shape in which it would.
    let passages: Vec<zorp_recall::Passage> = store
        .load_messages("conv")
        .unwrap()
        .iter()
        .enumerate()
        .filter(|(_, m)| m.role == "user" || m.role == "assistant")
        .map(|(seq, m)| zorp_recall::Passage {
            conversation_id: "conv".to_string(),
            title: "deploy the billing service".to_string(),
            updated: 0,
            seq: seq as i64,
            role: m.role.clone(),
            text: m.text().into_owned(),
            score: 1.0,
        })
        .collect();

    let block = zorp_web::memory::assemble(&passages)
        .block
        .expect("nothing was quoted at all");
    assert!(
        !block.contains(ONLY_IN_THE_SUMMARY),
        "the memory block quoted the summary: {block}"
    );
}
