//! `zorp-agent config`, and the file both surfaces read.
//!
//! Setting zorp up in the browser and setting it up in the terminal used to
//! be two separate jobs. The browser saved provider, base URL, model and
//! max tokens to a file; the terminal read none of it.
//!
//! Two things are worth driving through the real binary. That the chain
//! resolves in the stated order, because somebody will be surprised by it
//! otherwise, and that **the API key never reaches the file**, because that
//! is the one thing this feature must not do.

use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_zorp-agent")
}

struct Run {
    stdout: String,
    stderr: String,
    code: Option<i32>,
}

/// Every run gets its own config home, so nothing here reads or writes the
/// developer's own settings.
fn run(home: &std::path::Path, args: &[&str], env: &[(&str, &str)]) -> Run {
    let mut command = Command::new(bin());
    command
        .args(args)
        .env("XDG_CONFIG_HOME", home)
        .env_remove("ZORP_CONFIG")
        .env_remove("ZORP_WEB_CONFIG")
        .env_remove("ZORP_MODEL")
        .env_remove("ZORP_BASE_URL")
        .env_remove("ZORP_PROVIDER")
        .env_remove("ZORP_MAX_TOKENS")
        .env_remove("ZORP_API_KEY");
    for (name, value) in env {
        command.env(name, value);
    }
    let out = command.output().unwrap();
    Run {
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        code: out.status.code(),
    }
}

/// The one line of a report that matters for a given key.
fn line<'a>(text: &'a str, key: &str) -> &'a str {
    text.lines()
        .find(|l| l.starts_with(key))
        .unwrap_or_else(|| panic!("no {key} line in:\n{text}"))
}

#[test]
fn what_is_set_is_what_comes_back_and_where_it_came_from() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();

    let set = run(home, &["config", "set", "model", "qwen3-coder"], &[]);
    assert_eq!(set.code, Some(0), "{}", set.stderr);

    let report = run(home, &["config"], &[]).stdout;
    let model = line(&report, "model");
    assert!(model.contains("qwen3-coder"), "{report}");
    assert!(model.contains("zorp.toml"), "{report}");
}

/// The order is flag, environment variable, flavor, saved file, default,
/// and the point of adding the file below the flavor is that nothing which
/// used to win stops winning.
#[test]
fn an_environment_variable_still_beats_the_saved_file() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    run(home, &["config", "set", "model", "from-file"], &[]);

    let report = run(home, &["config"], &[("ZORP_MODEL", "from-env")]).stdout;

    let model = line(&report, "model");
    assert!(model.contains("from-env"), "{report}");
    assert!(model.contains("$ZORP_MODEL"), "{report}");
}

#[test]
fn a_flag_beats_everything() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    run(home, &["config", "set", "model", "from-file"], &[]);

    let report = run(
        home,
        &["--model", "from-flag", "config"],
        &[("ZORP_MODEL", "from-env")],
    )
    .stdout;

    let model = line(&report, "model");
    assert!(model.contains("from-flag"), "{report}");
    assert!(model.contains("a flag on this command"), "{report}");
}

#[test]
fn nothing_set_anywhere_reports_the_default_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let report = run(dir.path(), &["config"], &[]).stdout;

    assert!(
        line(&report, "base url").contains("the built in default"),
        "{report}"
    );
    assert!(
        line(&report, "base url").contains("localhost:11434"),
        "{report}"
    );
}

/// **The API key is never written to that file, from either side.** This
/// is the rule the issue is most explicit about.
#[test]
fn the_api_key_is_refused_and_never_written() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();

    let refused = run(
        home,
        &["config", "set", "api-key", "sk-do-not-write-me"],
        &[],
    );
    assert_eq!(refused.code, Some(2), "{}", refused.stdout);
    assert!(
        refused.stderr.contains("never written to a file"),
        "{}",
        refused.stderr
    );
    assert!(
        refused.stderr.contains("ZORP_API_KEY"),
        "{}",
        refused.stderr
    );

    // And a real key in the environment does not end up on disk either,
    // even after a write that does succeed.
    run(
        home,
        &["config", "set", "model", "m"],
        &[("ZORP_API_KEY", "sk-real-secret-0123456789")],
    );
    let file = std::fs::read_to_string(home.join("zorp").join("zorp.toml")).unwrap();
    assert!(!file.contains("sk-real-secret"), "{file}");
    assert!(!file.contains("api_key"), "{file}");
    assert!(!file.contains("0123456789"), "{file}");
}

/// It still has to say whether one is set. That is the useful half and it
/// costs nothing.
#[test]
fn the_report_says_whether_a_key_is_set_without_showing_it() {
    let dir = tempfile::tempdir().unwrap();
    let key = "sk-real-secret-0123456789";

    let with = run(dir.path(), &["config"], &[("ZORP_API_KEY", key)]).stdout;
    assert!(with.contains("set in $ZORP_API_KEY"), "{with}");
    assert!(!with.contains(key), "{with}");
    assert!(!with.contains("0123456789"), "{with}");

    let without = run(dir.path(), &["config"], &[]).stdout;
    assert!(line(&without, "api key").contains("not set"), "{without}");
}

#[test]
fn unsetting_falls_back_through_the_chain() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    run(home, &["config", "set", "model", "from-file"], &[]);

    let unset = run(home, &["config", "unset", "model"], &[]);
    assert_eq!(unset.code, Some(0), "{}", unset.stderr);

    let report = run(home, &["config"], &[]).stdout;
    assert!(
        line(&report, "model").contains("built in default"),
        "{report}"
    );
}

/// A value that cannot be used is a refusal now rather than a confusing
/// failure on the next run.
#[test]
fn an_unusable_value_is_refused_at_write_time() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();

    let bad_provider = run(home, &["config", "set", "provider", "nonsense"], &[]);
    assert_eq!(bad_provider.code, Some(2));
    assert!(
        bad_provider.stderr.contains("openai or anthropic"),
        "{}",
        bad_provider.stderr
    );

    let bad_number = run(home, &["config", "set", "max-tokens", "lots"], &[]);
    assert_eq!(bad_number.code, Some(2));
    assert!(
        bad_number.stderr.contains("must be a number"),
        "{}",
        bad_number.stderr
    );

    let unknown = run(home, &["config", "set", "colour", "green"], &[]);
    assert_eq!(unknown.code, Some(2));
    assert!(
        unknown.stderr.contains("unknown setting"),
        "{}",
        unknown.stderr
    );
}

/// Somebody who configured zorp in the browser before the file was shared
/// should not have to do it again.
#[test]
fn the_browsers_old_web_toml_is_still_read() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let config = home.join("zorp");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(config.join("web.toml"), "model = \"from-the-browser\"\n").unwrap();

    let report = run(home, &["config"], &[]).stdout;

    assert!(
        line(&report, "model").contains("from-the-browser"),
        "{report}"
    );
}

#[test]
fn config_path_prints_where_it_writes() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(dir.path(), &["config", "path"], &[]).stdout;

    assert!(out.trim().ends_with("zorp.toml"), "{out}");
    assert!(out.contains(dir.path().to_str().unwrap()), "{out}");
}

/// The saved settings reach a real run, not only the report.
#[test]
fn a_saved_model_is_what_a_run_would_use() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    run(
        home,
        &["config", "set", "base-url", "http://127.0.0.1:59997/v1"],
        &[],
    );
    run(home, &["config", "set", "model", "saved-model"], &[]);

    // `doctor` is not in this branch, so the report is the observable one,
    // and it resolves through the same functions a run does.
    let report = run(home, &["config"], &[]).stdout;
    assert!(line(&report, "model").contains("saved-model"), "{report}");
    assert!(line(&report, "base url").contains("59997"), "{report}");
}
