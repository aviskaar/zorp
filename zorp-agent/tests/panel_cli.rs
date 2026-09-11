//! `zorp-agent panel`.
//!
//! The decision that created the panel says in its own words that it is "a
//! button in the browser and a function in `zorp-agent`". The function was
//! here the whole time and only the browser could call it.
//!
//! The two rules in that entry are not about which surface launches a
//! panel. They are about who and what a reviewer is, and both are tested
//! here: a person launches it, and a reviewer gets strictly less than the
//! panel that launched it.

mod common;

use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_zorp-agent")
}

/// **A reviewer gets strictly less than the panel that launched it.** No
/// `write_file`, no `apply_patch`, no `run_command`: an opinion that can
/// edit the thing it is reviewing is not a review.
#[test]
fn a_reviewer_has_exactly_the_read_only_allow_list() {
    let tools = zorp_agent::reviewer_tools();

    assert_eq!(
        tools,
        vec![
            "read_file".to_string(),
            "list_files".to_string(),
            "search_text".to_string(),
            "git_diff".to_string(),
            "git_status".to_string(),
        ],
        "the reviewer allow list changed"
    );
    for forbidden in ["write_file", "apply_patch", "run_command", "spawn_subagent"] {
        assert!(
            !tools.iter().any(|t| t == forbidden),
            "{forbidden} reached a reviewer"
        );
    }
}

/// The CLI must not widen that list, and must not hand a reviewer the
/// caller's own approval mode. `--yes` on the outer command is a fixed
/// `AutoApprove` inside, over a tool set with nothing approval gated in it,
/// which is not a loosening: it is the honest name for a gate with nothing
/// behind it.
#[test]
fn the_cli_passes_a_fixed_approval_and_never_the_callers() {
    let source = include_str!("../src/main.rs");
    let panel_fn = source
        .split("fn panel_command(")
        .nth(1)
        .expect("panel_command is there");
    let body = &panel_fn[..panel_fn.find("\nfn ").unwrap_or(panel_fn.len())];

    assert!(
        body.contains("ApprovalMode::AutoApprove"),
        "the panel command stopped passing a fixed approval mode"
    );
    assert!(
        !body.contains("ApprovalMode::terminal"),
        "the caller's approval mode reached a reviewer"
    );
    assert!(
        !body.contains("auto_approve"),
        "the caller's --yes reached a reviewer"
    );
}

#[test]
fn the_lenses_can_be_listed_without_running_anything() {
    let out = Command::new(bin())
        .args(["panel", "--list-lenses"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);

    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    for lens in zorp_agent::default_lenses() {
        assert!(text.contains(&lens.name), "{lens:?} missing from:\n{text}");
    }
}

/// Five reviewers asked to review nothing produce five confident answers
/// about nothing, which costs five requests and reads exactly like a real
/// panel.
#[test]
fn an_empty_target_is_refused_before_any_request() {
    let dir = tempfile::tempdir().unwrap();
    let empty = dir.path().join("empty.md");
    std::fs::write(&empty, "   \n\n").unwrap();

    let out = Command::new(bin())
        .arg("panel")
        .arg(&empty)
        .env("ZORP_BASE_URL", "http://127.0.0.1:59996/v1")
        .env("ZORP_MODEL", "m")
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("nothing to review"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn a_missing_file_is_a_readable_refusal() {
    let out = Command::new(bin())
        .args(["panel", "/no/such/file.md"])
        .env("ZORP_MODEL", "m")
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("cannot read"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A panel runs against a real endpoint, prints every reviewer's verdict
/// and the agreement counted in code, and a complete one exits zero.
#[test]
fn a_panel_prints_the_verdicts_and_the_agreement() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("draft.md");
    std::fs::write(&target, "The system is 40% faster than before.\n").unwrap();

    // One reply per reviewer, all naming the same locus, so the agreement
    // count has something to count.
    let verdict = r#"{"choices":[{"message":{"content":"Looks unsupported.\n\n```json\n{\"findings\":[{\"severity\":\"concern\",\"claim\":\"the 40% has no source\",\"locus\":\"the speed claim\"}]}\n```"},"finish_reason":"stop"}]}"#;
    let lenses = zorp_agent::default_lenses().len();
    let base = common::mock_script(vec![verdict; lenses]);

    let out = Command::new(bin())
        .arg("panel")
        .arg(&target)
        .arg("--label")
        .arg("the draft")
        .env("ZORP_BASE_URL", &base)
        .env("ZORP_MODEL", "m")
        .env_remove("ZORP_API_KEY")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);

    assert!(text.contains("panel on the draft"), "{text}");
    assert!(
        text.contains(&format!("complete: {lenses} of {lenses}")),
        "{text}"
    );
    assert!(text.contains("the 40% has no source"), "{text}");
    assert!(text.contains("agreement, counted in code"), "{text}");
    assert!(
        text.contains(&format!("{lenses} of {lenses}  the speed claim")),
        "{text}"
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A panel that could not finish is not a panel whose numbers mean what
/// they look like, so it says so and exits non-zero.
#[test]
fn a_partial_panel_says_so_and_exits_non_zero() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("draft.md");
    std::fs::write(&target, "The system is 40% faster than before.\n").unwrap();

    // One usable reply and nothing else, so the rest fall over.
    let base = common::mock_script(vec![
        r#"{"choices":[{"message":{"content":"```json\n{\"findings\":[]}\n```"},"finish_reason":"stop"}]}"#,
    ]);

    let out = Command::new(bin())
        .arg("panel")
        .arg(&target)
        .env("ZORP_BASE_URL", &base)
        .env("ZORP_MODEL", "m")
        .env_remove("ZORP_API_KEY")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);

    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains("not complete"), "{text}");
}
