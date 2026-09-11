//! A conversation started in the terminal gets a name.
//!
//! `sessions.display_title` used to be written in exactly one place, and
//! that place was `zorp-web`. A conversation you had in the terminal never
//! got a name, forever, including later in the browser sidebar, where it
//! showed its raw first message instead. The store is shared, so that was
//! not two products with two conventions. It was one column that only one
//! of its two writers filled in.
//!
//! The three rules the 2026-08-22 decision turns on are tested in
//! `zorp-agent/src/title.rs`, next to the code that enforces them. What is
//! worth driving through the real binary is that the CLI writes the column
//! at all, and that it still leaves `task` alone when it does.

mod common;

use common::mock_script;
use std::process::Command;
use zorp_agent::{Message, Store};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_zorp-agent")
}

/// An answer, then a title. `mock_script` hands them out in order, so the
/// second request the binary makes is the titling call.
fn answer_then_title(answer: &str, title: &str) -> String {
    let body = |text: &str| {
        format!(
            r#"{{"choices":[{{"message":{{"content":{}}},"finish_reason":"stop"}}]}}"#,
            serde_json::to_string(text).unwrap()
        )
    };
    let bodies = [body(answer), body(title)];
    let borrowed: Vec<&str> = bodies.iter().map(String::as_str).collect();
    mock_script(borrowed)
}

/// A conversation with one complete exchange already in the store, which is
/// what titling needs.
fn seeded(db: &std::path::Path, id: &str) {
    let mut store = Store::open_at(db).unwrap();
    store
        .create_session(id, "write hello.txt", "/repo", "m")
        .unwrap();
    store
        .record_message(id, 0, &Message::user("write hello.txt"))
        .unwrap();
    store
        .record_message(id, 1, &Message::assistant("Done."))
        .unwrap();
}

/// Continue a conversation, which is where the CLI titles one outside the
/// REPL.
///
/// Deliberately not the one-shot path. A one-shot is one command whose
/// whole value is that it answers and exits, and a second model call for a
/// label would double what it costs and how long it takes.
fn run_resume(db: &std::path::Path, base: &str, id: &str) -> std::process::Output {
    Command::new(bin())
        .args(["resume", id])
        .env("ZORP_BASE_URL", base)
        .env("ZORP_MODEL", "m")
        .env("ZORP_STATE_DB", db)
        .env_remove("ZORP_API_KEY")
        .env_remove("ZORP_SYSTEM")
        .env_remove("ZORP_SESSION_TITLES")
        .output()
        .unwrap()
}

fn row(db: &std::path::Path, id: &str) -> zorp_agent::SessionRow {
    Store::open_at(db)
        .unwrap()
        .sessions()
        .unwrap()
        .into_iter()
        .find(|r| r.id == id)
        .expect("the session")
}

#[test]
fn a_terminal_conversation_gets_a_name() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("s.db");
    seeded(&db, "conv");

    run_resume(
        &db,
        &answer_then_title("Done.", "Writing hello.txt"),
        "conv",
    );

    let stored = row(&db, "conv");
    assert_eq!(stored.display_title.as_deref(), Some("Writing hello.txt"));
    // And the verbatim first message is exactly as it was typed. `task` is
    // read into the recall index and quoted into later turns by memory, so
    // a generated sentence in it is the agent's own guess coming back as
    // evidence.
    assert_eq!(stored.task, "write hello.txt");
}

/// The same switch the browser has, and the same default.
#[test]
fn titles_are_off_when_the_env_var_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("s.db");
    seeded(&db, "conv");
    let base = answer_then_title("Done.", "Writing hello.txt");

    let _ = Command::new(bin())
        .args(["resume", "conv"])
        .env("ZORP_BASE_URL", &base)
        .env("ZORP_MODEL", "m")
        .env("ZORP_STATE_DB", &db)
        .env("ZORP_SESSION_TITLES", "0")
        .env_remove("ZORP_API_KEY")
        .env_remove("ZORP_SYSTEM")
        .output()
        .unwrap();

    let stored = row(&db, "conv");
    assert_eq!(stored.display_title, None);
    assert_eq!(stored.task, "write hello.txt");
}

/// The model is given a way to say the opening is too thin to name, and
/// taking it writes nothing. The first message keeps showing, which is what
/// a list read like before titles existed.
#[test]
fn a_declined_title_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("s.db");
    seeded(&db, "conv");

    run_resume(&db, &answer_then_title("Done.", "unclear"), "conv");

    let stored = row(&db, "conv");
    assert_eq!(stored.display_title, None, "a decline reached the column");
    assert_eq!(stored.task, "write hello.txt");
}

/// Whatever comes back is clamped on the one path to the column, so a model
/// that answers with a paragraph cannot put a paragraph in a sidebar. It is
/// cut to the cap rather than refused, which is the behaviour that moved
/// here unchanged.
#[test]
fn a_long_reply_is_cut_to_the_cap_before_it_reaches_the_column() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("s.db");
    seeded(&db, "conv");
    let base = answer_then_title(
        "Done.",
        "I'm sorry, but I cannot title this conversation because it is not clear \
         to me what the user was asking about, and I would need more context \
         before offering a name for it.",
    );

    run_resume(&db, &base, "conv");

    let title = row(&db, "conv").display_title.expect("a title");
    assert!(
        title.chars().count() <= zorp_agent::title::MAX_CHARS,
        "{title:?} is longer than the cap"
    );
    assert!(
        title.split_whitespace().count() <= zorp_agent::title::MAX_WORDS,
        "{title:?} is more words than the cap"
    );
}

/// One call per conversation, not one per turn. The store read that decides
/// it survives a restart because it asks the store rather than remembering,
/// and this is a second process entirely.
#[test]
fn a_conversation_that_already_has_a_name_is_not_renamed() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("s.db");
    seeded(&db, "conv");
    run_resume(
        &db,
        &answer_then_title("Done.", "Writing hello.txt"),
        "conv",
    );

    // A second run against the same session, with a script that would name
    // it something else if anything asked.
    run_resume(
        &db,
        &answer_then_title("Done again.", "A completely different name"),
        "conv",
    );

    assert_eq!(
        row(&db, "conv").display_title.as_deref(),
        Some("Writing hello.txt")
    );
}

/// The whole point of moving the module: a terminal conversation writes the
/// same column the browser sidebar reads, because it is one column in one
/// store.
#[test]
fn the_name_lands_in_the_column_the_sidebar_reads() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("s.db");
    seeded(&db, "conv");

    run_resume(
        &db,
        &answer_then_title("Done.", "Writing hello.txt"),
        "conv",
    );

    // Both readers agree, because there is only one of them.
    let store = Store::open_at(&db).unwrap();
    assert_eq!(
        store.display_title("conv").unwrap().as_deref(),
        Some("Writing hello.txt")
    );
    assert_eq!(
        row(&db, "conv").display_title.as_deref(),
        Some("Writing hello.txt")
    );
}

/// A one-shot answers and exits, and does not spend a second model call on
/// a label. That is the trade being made, so it is pinned here rather than
/// left to be rediscovered.
#[test]
fn a_one_shot_does_not_spend_a_second_call_on_a_name() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("s.db");
    // One reply in the script. A titling call would have nothing to answer
    // it; the point is that it is never made.
    let base = mock_script(vec![
        r#"{"choices":[{"message":{"content":"42"},"finish_reason":"stop"}]}"#,
    ]);

    let out = Command::new(bin())
        .arg("what is 6x7")
        .env("ZORP_BASE_URL", &base)
        .env("ZORP_MODEL", "m")
        .env("ZORP_STATE_DB", &db)
        .env_remove("ZORP_API_KEY")
        .env_remove("ZORP_SYSTEM")
        .env_remove("ZORP_SESSION_TITLES")
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "42\n");

    let stored = Store::open_at(&db).unwrap().sessions().unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].display_title, None, "a one-shot paid for a title");
}
