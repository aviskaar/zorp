//! Projects, from the terminal.
//!
//! Projects landed in #195 and every part of them was in the store, which
//! the terminal shares. None of it was reachable from the terminal, so a
//! conversation started with `zorp-agent chat` was unfiled forever unless
//! somebody went to the browser to file it.
//!
//! A project is a label. Nothing about a conversation changes when it joins
//! one, and **deleting a project deletes no conversation**, which is the
//! thing a person is afraid of and the thing that does not happen.

use std::process::{Command, Output};
use zorp_agent::{Message, Store};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_zorp-agent")
}

fn run(db: &std::path::Path, args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .env("ZORP_STATE_DB", db)
        .env_remove("ZORP_API_KEY")
        .env_remove("ZORP_BASE_URL")
        .output()
        .unwrap()
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn seed(db: &std::path::Path, ids: &[&str]) {
    let mut store = Store::open_at(db).unwrap();
    for id in ids {
        store
            .create_session(id, &format!("ask {id}"), "/repo", "m")
            .unwrap();
        store.record_message(id, 0, &Message::user("ask")).unwrap();
        store
            .record_message(id, 1, &Message::assistant("done"))
            .unwrap();
    }
}

/// The id of the one project, for a test that just made it.
fn only_project(db: &std::path::Path) -> String {
    Store::open_at(db).unwrap().projects().unwrap()[0]
        .id
        .clone()
}

#[test]
fn an_empty_store_says_how_to_make_one() {
    let dir = tempfile::tempdir().unwrap();
    let o = run(&dir.path().join("s.db"), &["projects"]);

    assert!(o.status.success(), "{}", err(&o));
    assert!(out(&o).contains("No projects yet"), "{}", out(&o));
    assert!(out(&o).contains("projects new"), "{}", out(&o));
}

#[test]
fn a_project_can_be_made_and_listed_with_its_count() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("s.db");
    seed(&db, &["aaaa1111", "bbbb2222"]);

    let made = run(&db, &["projects", "new", "Kitchen rebuild"]);
    assert!(made.status.success(), "{}", err(&made));
    assert!(out(&made).contains("Kitchen rebuild"), "{}", out(&made));

    let id = only_project(&db);
    Store::open_at(&db)
        .unwrap()
        .set_session_project("aaaa1111", Some(&id))
        .unwrap();

    let listed = out(&run(&db, &["projects"]));
    assert!(listed.contains("Kitchen rebuild"), "{listed}");
    // The count of what is in it, which is the reason to list them.
    assert!(listed.contains("  1  "), "{listed}");
}

/// The same rules the browser applies, from the same function rather than
/// a second copy. An override in a listing reorders every row after it.
#[test]
fn a_name_goes_through_the_shared_scrub_and_the_same_limit() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("s.db");

    let blank = run(&db, &["projects", "new", "   "]);
    assert_eq!(blank.status.code(), Some(2));
    assert!(err(&blank).contains("needs a name"), "{}", err(&blank));

    let hostile = run(&db, &["projects", "new", "\u{202E}drop\u{0007} it"]);
    assert!(hostile.status.success(), "{}", err(&hostile));
    let listed = out(&run(&db, &["projects"]));
    assert!(!listed.contains('\u{202E}'), "{listed:?}");
    assert!(!listed.contains('\u{0007}'), "{listed:?}");
    assert!(listed.contains("drop it"), "{listed}");

    let long = "x".repeat(81);
    let too_long = run(&db, &["projects", "new", &long]);
    assert_eq!(too_long.status.code(), Some(2));
    assert!(
        err(&too_long).contains("80 characters"),
        "{}",
        err(&too_long)
    );
}

/// **Deleting a project deletes no conversation**, and the output says so,
/// because that is the thing a person is afraid of.
#[test]
fn removing_a_project_keeps_its_conversations_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("s.db");
    seed(&db, &["aaaa1111", "bbbb2222"]);
    run(&db, &["projects", "new", "Roof"]);
    let id = only_project(&db);
    {
        let mut store = Store::open_at(&db).unwrap();
        store.set_session_project("aaaa1111", Some(&id)).unwrap();
        store.set_session_project("bbbb2222", Some(&id)).unwrap();
    }

    let removed = run(&db, &["projects", "rm", &id]);
    assert!(removed.status.success(), "{}", err(&removed));
    let text = out(&removed);
    assert!(text.contains("removed the project 'Roof'"), "{text}");
    assert!(text.contains("2 conversations were kept"), "{text}");

    // And they really are there, and unfiled.
    let store = Store::open_at(&db).unwrap();
    assert_eq!(store.sessions().unwrap().len(), 2);
    assert!(store
        .sessions()
        .unwrap()
        .iter()
        .all(|s| s.project_id.is_none()));
}

/// A person reading a listing has the name in front of them and would
/// otherwise have to go and copy an id.
#[test]
fn a_project_can_be_named_by_id_prefix_or_name() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("s.db");
    seed(&db, &["aaaa1111"]);
    run(&db, &["projects", "new", "Kitchen rebuild"]);
    let id = only_project(&db);

    for wanted in [id.as_str(), &id[..8], "Kitchen rebuild", "kitchen rebuild"] {
        let o = run(&db, &["sessions", "--project", wanted]);
        assert!(o.status.success(), "{wanted}: {}", err(&o));
    }

    let unknown = run(&db, &["sessions", "--project", "nope"]);
    assert!(!unknown.status.success());
    assert!(
        err(&unknown).contains("no project matching"),
        "{}",
        err(&unknown)
    );
}

#[test]
fn the_listing_filters_by_project_and_shows_which_one_otherwise() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("s.db");
    seed(&db, &["aaaa1111", "bbbb2222"]);
    run(&db, &["projects", "new", "Kitchen"]);
    let id = only_project(&db);
    Store::open_at(&db)
        .unwrap()
        .set_session_project("aaaa1111", Some(&id))
        .unwrap();

    // Unfiltered: the project is named beside the conversation in it.
    let all = out(&run(&db, &["sessions"]));
    assert!(all.contains("aaaa1111"), "{all}");
    assert!(all.contains("[Kitchen]"), "{all}");
    assert!(all.contains("bbbb2222"), "{all}");

    // Filtered: only that one, and no redundant label on every row.
    let filtered = out(&run(&db, &["sessions", "--project", "Kitchen"]));
    assert!(filtered.contains("aaaa1111"), "{filtered}");
    assert!(!filtered.contains("bbbb2222"), "{filtered}");
    assert!(!filtered.contains("[Kitchen]"), "{filtered}");
}

#[test]
fn an_empty_project_says_so_rather_than_printing_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("s.db");
    seed(&db, &["aaaa1111"]);
    run(&db, &["projects", "new", "Empty"]);

    let o = run(&db, &["sessions", "--project", "Empty"]);

    assert!(o.status.success(), "{}", err(&o));
    assert!(
        out(&o).contains("No conversations in 'Empty'"),
        "{}",
        out(&o)
    );
}

/// A branch lands in its parent's project, which #195 made true in the
/// store. This is the CLI seeing it.
#[test]
fn a_branch_made_in_the_terminal_stays_in_its_parents_project() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("s.db");
    seed(&db, &["aaaa1111"]);
    run(&db, &["projects", "new", "Kitchen"]);
    let id = only_project(&db);
    {
        let mut store = Store::open_at(&db).unwrap();
        store.set_session_project("aaaa1111", Some(&id)).unwrap();
        store.set_status("aaaa1111", "done").unwrap();
    }

    let branched = run(&db, &["branch", "aaaa1111"]);
    assert!(branched.status.success(), "{}", err(&branched));
    let new_id = out(&branched).trim().to_string();

    let filtered = out(&run(&db, &["sessions", "--project", "Kitchen"]));
    assert!(
        filtered.contains(&zorp_agent::sessions::short(&new_id)),
        "{filtered}"
    );
}
