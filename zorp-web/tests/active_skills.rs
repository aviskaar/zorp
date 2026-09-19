//! Which skills are in one conversation's context, over HTTP.
//!
//! `/api/skills` says what is installed. This route says what is loaded in
//! a given session, and the two answers diverge the moment a conversation
//! is long enough to compact. A skill body is a tool result body,
//! `plan_seed` takes the oldest of those first, and the activity line is
//! drawn from the store rather than from the window, so it keeps showing a
//! load whose instructions left the request several turns ago.
//!
//! The cases here are the ones a wrong implementation gets wrong: a body
//! that went, a name the model invented, and a skill that has since been
//! uninstalled. The happy path is one test.
//!
//! Read-only is pinned too. There is no route that loads a skill, this is
//! not one, and asking it a question must not change what the next turn
//! sends.

use std::net::SocketAddr;
use std::path::Path;
use tokio::sync::Mutex;
use zorp_agent::{Message, Store, ToolCall};

/// `ZORP_STATE_DB`, `ZORP_SKILLS_DIR` and `ZORP_WORKSPACE` are process
/// wide, so these take turns.
static ENV: Mutex<()> = Mutex::const_new(());

async fn spawn() -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, zorp_web::api::router())
            .await
            .unwrap();
    });
    addr
}

async fn get_json(url: String) -> serde_json::Value {
    let body =
        tokio::task::spawn_blocking(move || ureq::get(&url).call().unwrap().into_string().unwrap())
            .await
            .unwrap();
    serde_json::from_str(&body).unwrap()
}

async fn get_status(url: String) -> u16 {
    tokio::task::spawn_blocking(move || match ureq::get(&url).call() {
        Ok(response) => response.status(),
        Err(ureq::Error::Status(code, _)) => code,
        Err(e) => panic!("{e}"),
    })
    .await
    .unwrap()
}

fn write_skill(root: &Path, name: &str, description: &str, body: &str) {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("SKILL.md"),
        format!("---\ndescription: {description}\n---\n\n{body}\n"),
    )
    .unwrap();
}

/// The body a successful `skill` call returns, in `zorp-skill`'s own
/// format. Built through the real type so this file cannot drift from it.
fn skill_body(name: &str, text: &str) -> String {
    zorp_skill::Skill::parse(
        &format!("---\nname: {name}\ndescription: d\n---\n{text}"),
        name,
        std::path::PathBuf::from(format!("/skills/{name}/SKILL.md")),
    )
    .expect("parses")
    .instructions()
}

fn skill_call(id: &str, name: &str) -> Message {
    Message::assistant_with_calls(
        "loading a skill",
        vec![ToolCall {
            id: id.to_string(),
            name: "skill".to_string(),
            arguments: serde_json::json!({ "name": name }),
        }],
    )
}

/// A session holding one `skill` call per entry, plus a closing assistant
/// message so the last exchange is complete.
fn seed_session(db: &Path, id: &str, loads: &[(&str, &str, String)]) {
    let mut store = Store::open_at(db).unwrap();
    store.create_session(id, "task", "/tmp", "model").unwrap();
    let mut seq = 0i64;
    let mut push = |store: &mut Store, m: Message| {
        store.record_message(id, seq, &m).unwrap();
        seq += 1;
    };
    push(&mut store, Message::user("please do the thing"));
    for (call_id, name, body) in loads {
        push(&mut store, skill_call(call_id, name));
        push(&mut store, Message::tool_result(*call_id, body.as_str()));
    }
    push(&mut store, Message::assistant("done"));
}

struct Fixture {
    _dir: tempfile::TempDir,
    addr: SocketAddr,
}

impl Fixture {
    fn url(&self, session: &str) -> String {
        format!("http://{}/api/sessions/{session}/skills/active", self.addr)
    }
}

async fn fixture(loads: &[(&str, &str, String)], installed: &[&str]) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("sessions.db");
    let skills = dir.path().join("skills");
    std::fs::create_dir_all(&skills).unwrap();
    for name in installed {
        write_skill(&skills, name, "does a thing", "Step one.");
    }
    std::env::set_var("ZORP_STATE_DB", &db);
    std::env::set_var("ZORP_SKILLS_DIR", &skills);
    std::env::remove_var("ZORP_WORKSPACE");
    std::env::set_var("ZORP_CONTEXT_TOKENS", "");

    seed_session(&db, "s1", loads);
    Fixture {
        _dir: dir,
        addr: spawn().await,
    }
}

#[tokio::test]
async fn a_loaded_skill_whose_body_survived_is_reported_present() {
    let _env = ENV.lock().await;
    let fx = fixture(
        &[("c1", "tidy-notes", skill_body("tidy-notes", "Step one."))],
        &["tidy-notes"],
    )
    .await;

    let body = get_json(fx.url("s1")).await;
    assert_eq!(body["loaded"], 1, "{body}");
    assert_eq!(body["active"], 1, "{body}");
    let row = &body["skills"][0];
    assert_eq!(row["name"], "tidy-notes");
    assert_eq!(row["presence"], "present");
    assert_eq!(row["active"], true);
    assert_eq!(row["scope"], "env");
    assert!(row["bytes_in_window"].as_u64().unwrap() > 0, "{row}");
}

/// The whole reason this route exists. A big enough transcript pushes the
/// oldest tool result out of the window, and the report has to say so even
/// though the call that loaded it is still sitting in the conversation.
#[tokio::test]
async fn a_body_compaction_took_is_reported_as_not_active() {
    let _env = ENV.lock().await;
    let filler = "x".repeat(400 * 1024);
    let fx = fixture(
        &[
            ("c1", "old", skill_body("old", &filler)),
            ("c2", "new", skill_body("new", &filler)),
        ],
        &["old", "new"],
    )
    .await;

    let body = get_json(fx.url("s1")).await;
    assert_eq!(body["loaded"], 2, "{body}");
    assert_eq!(body["active"], 1, "{body}");

    let rows = body["skills"].as_array().unwrap();
    let find = |name: &str| {
        rows.iter()
            .find(|r| r["name"] == name)
            .unwrap_or_else(|| panic!("{name} missing from {body}"))
    };
    assert_eq!(find("new")["presence"], "present");
    assert_eq!(find("old")["active"], false, "{body}");
    assert_ne!(find("old")["presence"], "present", "{body}");
    // Live first, so the answer to "what is in my context" is at the top.
    assert_eq!(rows[0]["name"], "new", "{body}");
}

/// The name is read out of the header zorp writes onto a loaded body, not
/// out of the call's arguments. A call naming a skill that does not exist
/// returns an error rather than instructions, and an error is not a load.
/// Taking the name from the arguments would put whatever the model typed
/// into a list a person reads to find out what is influencing the model.
#[tokio::test]
async fn a_call_that_errored_is_not_reported_as_a_loaded_skill() {
    let _env = ENV.lock().await;
    let fx = fixture(
        &[(
            "c1",
            "../../etc/passwd",
            "skill: no skill named '../../etc/passwd'. Available: tidy-notes".to_string(),
        )],
        &["tidy-notes"],
    )
    .await;

    let body = get_json(fx.url("s1")).await;
    assert_eq!(body["loaded"], 0, "{body}");
    assert!(
        !body.to_string().contains("passwd"),
        "a name the model invented reached the listing: {body}"
    );
}

/// Instructions from a file that is no longer on disk are precisely what
/// somebody needs to be able to see, so the row survives its skill being
/// uninstalled and says the scope is gone rather than disappearing.
#[tokio::test]
async fn a_skill_uninstalled_since_it_was_loaded_keeps_its_row() {
    let _env = ENV.lock().await;
    let fx = fixture(
        &[("c1", "tidy-notes", skill_body("tidy-notes", "Step one."))],
        &[],
    )
    .await;

    let body = get_json(fx.url("s1")).await;
    assert_eq!(body["loaded"], 1, "{body}");
    assert_eq!(body["skills"][0]["name"], "tidy-notes");
    assert_eq!(body["skills"][0]["presence"], "present");
    assert!(body["skills"][0]["scope"].is_null(), "{body}");
}

#[tokio::test]
async fn a_conversation_that_loaded_nothing_is_an_empty_list_and_not_an_error() {
    let _env = ENV.lock().await;
    let fx = fixture(&[], &["tidy-notes"]).await;

    let body = get_json(fx.url("s1")).await;
    assert_eq!(body["loaded"], 0, "{body}");
    assert_eq!(body["active"], 0, "{body}");
    assert!(body["skills"].as_array().unwrap().is_empty(), "{body}");
}

#[tokio::test]
async fn a_session_that_does_not_exist_is_a_404() {
    let _env = ENV.lock().await;
    let fx = fixture(&[], &["tidy-notes"]).await;

    assert_eq!(get_status(fx.url("nope")).await, 404);
}

/// The body is what this route is describing and it must never be what the
/// route sends. A `SKILL.md` is a file zorp did not write, its text is
/// handed to a model as untrusted input, and it has no business crossing
/// into a browser.
#[tokio::test]
async fn the_report_never_carries_a_skill_body() {
    let _env = ENV.lock().await;
    let secret = "THE INSTRUCTIONS THEMSELVES";
    let fx = fixture(
        &[("c1", "tidy-notes", skill_body("tidy-notes", secret))],
        &["tidy-notes"],
    )
    .await;

    let body = get_json(fx.url("s1")).await.to_string();
    assert!(
        !body.contains(secret),
        "the body reached the browser: {body}"
    );
    assert!(!body.contains("not a grant of permission"), "{body}");
}

/// Read-only means the stored transcript is exactly as it was afterwards.
/// A report that re-injected a body in order to describe it, or that wrote
/// anything at all, would fail here.
#[tokio::test]
async fn asking_the_question_changes_nothing_in_the_store() {
    let _env = ENV.lock().await;
    let fx = fixture(
        &[("c1", "tidy-notes", skill_body("tidy-notes", "Step one."))],
        &["tidy-notes"],
    )
    .await;

    let db = std::env::var("ZORP_STATE_DB").unwrap();
    let before = Store::open_at(Path::new(&db))
        .unwrap()
        .load_message_records("s1")
        .unwrap();

    let first = get_json(fx.url("s1")).await;
    let second = get_json(fx.url("s1")).await;
    assert_eq!(first, second, "the answer moved under a second read");

    let after = Store::open_at(Path::new(&db))
        .unwrap()
        .load_message_records("s1")
        .unwrap();
    assert_eq!(before.len(), after.len());
    for (a, b) in before.iter().zip(after.iter()) {
        assert_eq!(a.message.text(), b.message.text());
    }
}

/// "Why does this conversation not have the component skill" is a question a
/// person asks about a conversation, so the answer is on this route too: the
/// rule's sentence, the model it was applied to, and what it withheld.
#[tokio::test]
async fn the_report_says_which_skills_this_conversations_model_is_offered() {
    let _env = ENV.lock().await;
    std::env::set_var("ZORP_MODEL", "gemma2:2b");
    std::env::remove_var("ZORP_SKILL_TIER");
    let fx = fixture(
        &[(
            "c1",
            "landing-page",
            skill_body("landing-page", "Step one."),
        )],
        &["landing-page", "react-components"],
    )
    .await;

    let body = get_json(fx.url("s1")).await;
    std::env::remove_var("ZORP_MODEL");
    let offer = &body["offer"];
    assert_eq!(offer["model"], "gemma2:2b", "{body}");
    assert_eq!(offer["tier"], "plain", "{body}");
    assert_eq!(
        offer["withheld"],
        serde_json::json!(["react-components"]),
        "{body}"
    );
    assert!(
        offer["reason"].as_str().unwrap().contains("gemma2:2b"),
        "{body}"
    );
    // The loaded skill is still reported as loaded; routing is not presence.
    assert_eq!(body["skills"][0]["name"], "landing-page", "{body}");
}
