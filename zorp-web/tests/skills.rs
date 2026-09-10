//! What the browser can learn about installed skills.
//!
//! Read-only, in every build, and that is the whole surface. There is no
//! route that loads a skill and there must never be one: loading is the
//! agent's `skill` tool, a skill body arrives as a tool result like any
//! other, and it grants no tool, loosens no approval, and bypasses no
//! denylist entry. See `docs/DECISIONS.md` (2026-08-18).
//!
//! The list has to be the list the agent would register, which is why both
//! sides go through `zorp_skill::scope_dirs_from_env` rather than each
//! deriving the directories for itself.

use std::net::SocketAddr;
use std::path::Path;
use tokio::sync::Mutex;

/// `ZORP_SKILLS_DIR` and `ZORP_WORKSPACE` are process wide, so these take
/// turns.
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

fn write_skill(root: &Path, name: &str, description: &str, body: &str) {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("SKILL.md"),
        format!("---\ndescription: {description}\nallowed-tools: Read, Write\n---\n\n{body}\n"),
    )
    .unwrap();
}

#[tokio::test]
async fn skills_are_listed_with_where_they_came_from() {
    let _env = ENV.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let env_skills = dir.path().join("env-skills");
    write_skill(
        &env_skills,
        "tidy-notes",
        "Turn rough notes into a short ordered summary.",
        "Group by subject and put the open questions last.",
    );
    std::env::set_var("ZORP_SKILLS_DIR", &env_skills);
    std::env::set_var("ZORP_WORKSPACE", dir.path());

    let addr = spawn().await;
    let body = get_json(format!("http://{addr}/api/skills")).await;
    let skills = body["skills"].as_array().unwrap();

    let found = skills
        .iter()
        .find(|s| s["name"] == "tidy-notes")
        .unwrap_or_else(|| panic!("the skill is not listed: {body}"));
    assert_eq!(
        found["description"],
        "Turn rough notes into a short ordered summary."
    );
    assert_eq!(found["scope"], "env", "{body}");
    assert!(
        found["path"].as_str().unwrap().ends_with("SKILL.md"),
        "{body}"
    );
    // Reported and never acted on, so the gap between what a skill asks for
    // and what it gets is visible rather than silent.
    assert_eq!(
        found["declared_tools"],
        serde_json::json!(["Read", "Write"]),
        "{body}"
    );

    std::env::remove_var("ZORP_SKILLS_DIR");
}

/// A workspace's own `.claude/skills` is found, and says so.
#[tokio::test]
async fn a_workspace_skill_is_found_and_labelled() {
    let _env = ENV.lock().await;
    let dir = tempfile::tempdir().unwrap();
    write_skill(
        &dir.path().join(".claude").join("skills"),
        "house-style",
        "How this repository writes things down.",
        "Short sentences. Say why, not only what.",
    );
    std::env::remove_var("ZORP_SKILLS_DIR");
    std::env::set_var("ZORP_WORKSPACE", dir.path());

    let addr = spawn().await;
    let body = get_json(format!("http://{addr}/api/skills")).await;
    let found = body["skills"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "house-style")
        .unwrap_or_else(|| panic!("the workspace skill is not listed: {body}"));

    assert_eq!(found["scope"], "workspace", "{body}");
}

/// A skill that cannot be parsed is named rather than swallowed. Somebody
/// whose skill is missing needs to know why it is missing.
#[tokio::test]
async fn an_unreadable_skill_is_reported_as_a_warning() {
    let _env = ENV.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let env_skills = dir.path().join("env-skills");
    let broken = env_skills.join("broken");
    std::fs::create_dir_all(&broken).unwrap();
    // No frontmatter at all, so there is no description to key on.
    std::fs::write(broken.join("SKILL.md"), "just a body, no frontmatter\n").unwrap();
    std::env::set_var("ZORP_SKILLS_DIR", &env_skills);
    std::env::set_var("ZORP_WORKSPACE", dir.path());

    let addr = spawn().await;
    let body = get_json(format!("http://{addr}/api/skills")).await;

    assert!(
        !body["skills"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["name"] == "broken"),
        "a skill that does not parse was listed anyway: {body}"
    );
    let warnings = body["warnings"].as_array().unwrap();
    assert!(
        warnings
            .iter()
            .any(|w| w.as_str().unwrap().contains("broken")),
        "the broken skill was swallowed: {body}"
    );

    std::env::remove_var("ZORP_SKILLS_DIR");
}

/// The page draws a pill from the count, so the count has to be there and
/// has to agree with the list.
#[tokio::test]
async fn capabilities_reports_how_many_skills_there_are() {
    let _env = ENV.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let env_skills = dir.path().join("env-skills");
    write_skill(&env_skills, "one", "The first.", "Body.");
    write_skill(&env_skills, "two", "The second.", "Body.");
    std::env::set_var("ZORP_SKILLS_DIR", &env_skills);
    std::env::set_var("ZORP_WORKSPACE", dir.path());

    let addr = spawn().await;
    let caps = get_json(format!("http://{addr}/api/capabilities")).await;
    let listed = get_json(format!("http://{addr}/api/skills")).await;

    assert_eq!(caps["skills"]["available"], true, "{caps}");
    assert_eq!(
        caps["skills"]["count"].as_u64().unwrap() as usize,
        listed["skills"].as_array().unwrap().len(),
        "the count and the list disagree: {caps} / {listed}"
    );

    std::env::remove_var("ZORP_SKILLS_DIR");
}

/// No skills is not an error and not a missing field. It is a page that
/// draws no pill.
#[tokio::test]
async fn no_skills_is_a_count_of_zero_and_an_empty_list() {
    let _env = ENV.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let empty = dir.path().join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    std::env::set_var("ZORP_SKILLS_DIR", &empty);
    // A workspace with no `.claude/skills` of its own.
    std::env::set_var("ZORP_WORKSPACE", dir.path());
    // And a HOME with none either, so this does not depend on the machine
    // it runs on.
    std::env::set_var("HOME", dir.path());

    let addr = spawn().await;
    let caps = get_json(format!("http://{addr}/api/capabilities")).await;
    let listed = get_json(format!("http://{addr}/api/skills")).await;

    assert_eq!(caps["skills"]["available"], false, "{caps}");
    assert_eq!(caps["skills"]["count"], 0, "{caps}");
    assert!(listed["skills"].as_array().unwrap().is_empty(), "{listed}");

    std::env::remove_var("ZORP_SKILLS_DIR");
}

/// There is no route that loads a skill, and this is the test that says so.
/// Loading is the agent's `skill` tool, gated as it always was.
#[tokio::test]
async fn there_is_no_route_that_loads_a_skill() {
    let _env = ENV.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let env_skills = dir.path().join("env-skills");
    write_skill(&env_skills, "tidy-notes", "A description.", "A body.");
    std::env::set_var("ZORP_SKILLS_DIR", &env_skills);
    std::env::set_var("ZORP_WORKSPACE", dir.path());

    let addr = spawn().await;
    for path in ["/api/skills/tidy-notes", "/api/skills/tidy-notes/load"] {
        let url = format!("http://{addr}{path}");
        let status = tokio::task::spawn_blocking(move || match ureq::get(&url).call() {
            Ok(r) => r.status(),
            Err(ureq::Error::Status(code, _)) => code,
            Err(e) => panic!("{e}"),
        })
        .await
        .unwrap();
        assert_eq!(status, 404, "{path} answered something");
    }

    // And the body never reaches the browser: the listing is names,
    // descriptions and paths, and nothing else.
    let listed = get_json(format!("http://{addr}/api/skills")).await;
    assert!(
        !listed.to_string().contains("A body."),
        "the listing carried a skill body: {listed}"
    );

    std::env::remove_var("ZORP_SKILLS_DIR");
}
