//! `zorp-agent sessions`, and `resume` finding its way to one row.
//!
//! The browser and the terminal share one store, so this list is one list.
//! What is worth driving through the real binary rather than through the
//! library is the shape of the answers: what an empty store prints, what a
//! limit does, and above all that an ambiguous prefix refuses instead of
//! picking. The wrong pick drops somebody into a stranger's conversation
//! and the transcript that follows looks perfectly plausible.

use std::path::Path;
use std::process::{Command, Output};
use zorp_agent::{Message, Store};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_zorp-agent")
}

fn run(db: &Path, args: &[&str]) -> Output {
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

/// `count` conversations with ids that share a prefix, so the prefix rules
/// have something to be wrong about.
fn seed(db: &Path, ids: &[(&str, &str, Option<&str>)]) {
    let mut store = Store::open_at(db).unwrap();
    for (id, task, title) in ids {
        store.create_session(id, task, "/repo", "m").unwrap();
        store.record_message(id, 0, &Message::user(*task)).unwrap();
        store
            .record_message(id, 1, &Message::assistant("done"))
            .unwrap();
        if let Some(title) = title {
            store.set_display_title(id, title).unwrap();
        }
    }
}

/// Nothing there is not an error and not a blank screen. Somebody who has
/// just installed this needs to be told that is what they are looking at.
#[test]
fn an_empty_store_says_so_and_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("s.db");

    let o = run(&db, &["sessions"]);

    assert!(o.status.success(), "{}", err(&o));
    assert!(out(&o).contains("No conversations yet"), "{}", out(&o));
}

#[test]
fn conversations_are_listed_newest_first_with_a_name() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("s.db");
    seed(
        &db,
        &[
            ("aaaa1111", "write the deploy script", Some("Deploy script")),
            ("bbbb2222", "why does billing fail", None),
        ],
    );

    let o = run(&db, &["sessions"]);
    let text = out(&o);
    assert!(o.status.success(), "{}", err(&o));

    // Newest first, which is the order the store hands them back in.
    let first = text.lines().next().unwrap();
    assert!(first.starts_with("bbbb2222"), "{text}");
    // A generated title when there is one, and the verbatim first message
    // when there is not.
    assert!(text.contains("Deploy script"), "{text}");
    assert!(text.contains("why does billing fail"), "{text}");
}

#[test]
fn the_limit_caps_the_list_and_says_what_is_left() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("s.db");
    let ids: Vec<String> = (0..25).map(|i| format!("id{i:04}")).collect();
    let rows: Vec<(&str, &str, Option<&str>)> =
        ids.iter().map(|id| (id.as_str(), "ask", None)).collect();
    seed(&db, &rows);

    let capped = run(&db, &["sessions", "--limit", "5"]);
    let text = out(&capped);
    let listed = text.lines().filter(|l| l.starts_with("id")).count();
    assert_eq!(listed, 5, "{text}");
    assert!(text.contains("20 more"), "{text}");

    let all = run(&db, &["sessions", "--all"]);
    let text = out(&all);
    assert_eq!(text.lines().filter(|l| l.starts_with("id")).count(), 25);
    assert!(!text.contains("more."), "{text}");
}

/// The default exists so a year of use does not print a year of scrollback.
#[test]
fn the_default_limit_is_twenty() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("s.db");
    let ids: Vec<String> = (0..25).map(|i| format!("id{i:04}")).collect();
    let rows: Vec<(&str, &str, Option<&str>)> =
        ids.iter().map(|id| (id.as_str(), "ask", None)).collect();
    seed(&db, &rows);

    let text = out(&run(&db, &["sessions"]));
    assert_eq!(
        text.lines().filter(|l| l.starts_with("id")).count(),
        20,
        "{text}"
    );
}

/// An id nobody has is a readable refusal that says where to look, not a
/// stack trace and not silence.
#[test]
fn an_unknown_id_says_where_to_look() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("s.db");
    seed(&db, &[("aaaa1111", "ask", None)]);

    let o = run(&db, &["resume", "zzzz"]);

    assert!(!o.status.success());
    assert!(err(&o).contains("no session 'zzzz'"), "{}", err(&o));
    assert!(err(&o).contains("zorp-agent sessions"), "{}", err(&o));
}

/// Never a guess. This is the one that matters: two conversations, one
/// prefix, and the answer is both ids and a non-zero exit.
#[test]
fn an_ambiguous_prefix_lists_the_candidates_and_fails() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("s.db");
    seed(
        &db,
        &[
            ("aaaa1111", "the first one", None),
            ("aaaa2222", "the second one", None),
            ("bbbb3333", "unrelated", None),
        ],
    );

    let o = run(&db, &["resume", "aaaa"]);
    let text = err(&o);

    assert!(
        !o.status.success(),
        "an ambiguous prefix was resolved anyway"
    );
    assert!(text.contains("matches 2 conversations"), "{text}");
    assert!(text.contains("aaaa1111"), "{text}");
    assert!(text.contains("aaaa2222"), "{text}");
    assert!(!text.contains("bbbb3333"), "{text}");
}

/// A unique prefix gets far enough to say which conversation it picked.
/// It then needs a model, which this test does not give it, so the run
/// stops after that line rather than answering anything.
#[test]
fn a_unique_prefix_resolves_and_says_which_one_it_took() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("s.db");
    seed(
        &db,
        &[
            ("aaaa1111", "the only one", None),
            ("bbbb2222", "other", None),
        ],
    );

    let text = err(&run(&db, &["resume", "aaaa"]));

    assert!(text.contains("resuming aaaa1111"), "{text}");
    assert!(text.contains("the only one"), "{text}");
}

/// The single most common thing anybody wants, and it needs no id at all.
#[test]
fn resume_with_no_id_takes_the_most_recent() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("s.db");
    seed(
        &db,
        &[
            ("aaaa1111", "the older one", None),
            ("bbbb2222", "the newer one", None),
        ],
    );

    let text = err(&run(&db, &["resume"]));

    assert!(text.contains("resuming bbbb2222"), "{text}");
    assert!(text.contains("the newer one"), "{text}");
}

#[test]
fn resume_with_no_id_and_no_conversations_says_how_to_start_one() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("s.db");

    let o = run(&db, &["resume"]);

    assert!(!o.status.success());
    assert!(err(&o).contains("no conversations yet"), "{}", err(&o));
    assert!(err(&o).contains("zorp-agent chat"), "{}", err(&o));
}

/// A first message can be pasted from anywhere. A bidirectional override in
/// a listing reorders every line drawn after it, which is how one row
/// impersonates another in a list somebody is picking from.
#[test]
fn a_hostile_first_message_cannot_reorder_the_listing() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("s.db");
    seed(
        &db,
        &[("aaaa1111", "\u{202E}drop\u{0007} the database", None)],
    );

    let text = out(&run(&db, &["sessions"]));

    assert!(!text.contains('\u{202E}'), "{text:?}");
    assert!(!text.contains('\u{0007}'), "{text:?}");
    assert!(text.contains("drop the database"), "{text}");
}

/* ------------------------------------------------------------------ */
/* rm and branch                                                       */
/* ------------------------------------------------------------------ */

/// The one command here that destroys something, so it is the one that
/// asks. Answering no changes nothing.
#[test]
fn deleting_asks_first_and_a_no_changes_nothing() {
    use std::io::Write;
    use std::process::Stdio;

    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("s.db");
    seed(&db, &[("aaaa1111", "the one to keep", None)]);
    Store::open_at(&db)
        .unwrap()
        .set_status("aaaa1111", "done")
        .unwrap();

    let mut child = Command::new(bin())
        .args(["rm", "aaaa1111"])
        .env("ZORP_STATE_DB", &db)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.as_mut().unwrap().write_all(b"n\n").unwrap();
    let o = child.wait_with_output().unwrap();

    assert!(err(&o).contains("nothing deleted"), "{}", err(&o));
    // Still there.
    let text = out(&run(&db, &["sessions"]));
    assert!(text.contains("aaaa1111"), "{text}");
}

#[test]
fn deleting_with_yes_removes_the_conversation_and_its_messages() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("s.db");
    seed(
        &db,
        &[
            ("aaaa1111", "the one to go", None),
            ("bbbb2222", "the one to keep", None),
        ],
    );
    let store = Store::open_at(&db).unwrap();
    store.set_status("aaaa1111", "done").unwrap();

    let o = run(&db, &["rm", "aaaa1111", "--yes"]);
    assert!(o.status.success(), "{}", err(&o));
    assert!(out(&o).contains("deleted aaaa1111"), "{}", out(&o));

    let text = out(&run(&db, &["sessions"]));
    assert!(!text.contains("aaaa1111"), "{text}");
    assert!(text.contains("bbbb2222"), "{text}");
    // The messages went with it, which is what makes this the destructive one.
    let store = Store::open_at(&db).unwrap();
    assert_eq!(store.message_count("aaaa1111").unwrap(), 0);
}

/// A CLI cannot see the browser's threads and a second `zorp-agent` cannot
/// see the first one's, so this column is the only shared signal there is.
/// It refuses and names what it read.
#[test]
fn a_running_status_refuses_and_says_what_it_read() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("s.db");
    seed(&db, &[("aaaa1111", "busy", None)]);
    Store::open_at(&db)
        .unwrap()
        .set_status("aaaa1111", "running")
        .unwrap();

    for args in [vec!["rm", "aaaa1111", "--yes"], vec!["branch", "aaaa1111"]] {
        let o = run(&db, &args);
        assert!(!o.status.success(), "{args:?} was not refused");
        assert!(err(&o).contains("recorded as 'running'"), "{}", err(&o));
        assert!(err(&o).contains("--force"), "{}", err(&o));
    }

    // And it is still there.
    assert!(out(&run(&db, &["sessions"])).contains("aaaa1111"));
}

/// A process killed mid-turn leaves `running` behind with nothing running,
/// so the refusal has to be something a person can get past.
#[test]
fn force_gets_past_a_stale_running_status() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("s.db");
    seed(&db, &[("aaaa1111", "stale", None)]);
    Store::open_at(&db)
        .unwrap()
        .set_status("aaaa1111", "running")
        .unwrap();

    let o = run(&db, &["rm", "aaaa1111", "--yes", "--force"]);

    assert!(o.status.success(), "{}", err(&o));
    assert!(!out(&run(&db, &["sessions"])).contains("aaaa1111"));
}

#[test]
fn branching_prints_the_new_id_and_copies_the_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("s.db");
    let mut store = Store::open_at(&db).unwrap();
    store
        .create_session("aaaa1111", "the original", "/repo", "m")
        .unwrap();
    for (seq, m) in [
        (0, Message::user("first question")),
        (1, Message::assistant("first answer")),
        (2, Message::user("second question")),
        (3, Message::assistant("second answer")),
    ] {
        store.record_message("aaaa1111", seq, &m).unwrap();
    }
    store.set_status("aaaa1111", "done").unwrap();

    let o = run(&db, &["branch", "aaaa1111", "--answer", "1"]);
    assert!(o.status.success(), "{}", err(&o));

    // The new id goes to stdout on its own, so it can be piped.
    let new_id = out(&o).trim().to_string();
    assert!(!new_id.is_empty(), "{}", out(&o));
    assert!(
        err(&o).contains("branched aaaa1111 at answer 1 of 2"),
        "{}",
        err(&o)
    );

    let store = Store::open_at(&db).unwrap();
    let copied: Vec<String> = store
        .load_messages(&new_id)
        .unwrap()
        .iter()
        .map(|m| m.text().into_owned())
        .collect();
    assert_eq!(copied, vec!["first question", "first answer"], "{copied:?}");
    // The original is untouched.
    assert_eq!(store.message_count("aaaa1111").unwrap(), 4);
}

/// A terminal cannot see the answers to click one, so the number defaults
/// to the most recent.
#[test]
fn branching_defaults_to_the_latest_answer() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("s.db");
    let mut store = Store::open_at(&db).unwrap();
    store
        .create_session("aaaa1111", "the original", "/repo", "m")
        .unwrap();
    for (seq, m) in [
        (0, Message::user("first question")),
        (1, Message::assistant("first answer")),
        (2, Message::user("second question")),
        (3, Message::assistant("second answer")),
    ] {
        store.record_message("aaaa1111", seq, &m).unwrap();
    }
    store.set_status("aaaa1111", "done").unwrap();

    let o = run(&db, &["branch", "aaaa1111"]);
    let new_id = out(&o).trim().to_string();

    assert!(err(&o).contains("at answer 2 of 2"), "{}", err(&o));
    assert_eq!(
        Store::open_at(&db).unwrap().message_count(&new_id).unwrap(),
        4
    );
}

#[test]
fn an_out_of_range_answer_says_how_many_there_are() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("s.db");
    seed(&db, &[("aaaa1111", "one exchange", None)]);
    Store::open_at(&db)
        .unwrap()
        .set_status("aaaa1111", "done")
        .unwrap();

    let o = run(&db, &["branch", "aaaa1111", "--answer", "9"]);

    assert!(!o.status.success());
    assert!(err(&o).contains("has 1 answer,"), "{}", err(&o));
}

/// Both take a prefix, the same resolver `resume` uses.
#[test]
fn rm_and_branch_take_an_id_prefix_and_refuse_an_ambiguous_one() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("s.db");
    seed(&db, &[("aaaa1111", "one", None), ("aaaa2222", "two", None)]);

    let o = run(&db, &["rm", "aaaa", "--yes"]);
    assert!(!o.status.success());
    assert!(err(&o).contains("matches 2 conversations"), "{}", err(&o));

    let o = run(&db, &["branch", "aaaa", "--answer", "1"]);
    assert!(!o.status.success());
    assert!(err(&o).contains("matches 2 conversations"), "{}", err(&o));
}
