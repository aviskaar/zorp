//! `zorp-agent doctor`.
//!
//! Nearly everything interesting in zorp is a non-default Cargo feature,
//! and that is the right default. It also means the most common question
//! anybody has is "why is this not working", and the answer is nearly
//! always the feature is not compiled in, the local model is not running,
//! or the endpoint is not reachable. The browser answers that in three
//! places; a terminal got an error at the moment it tried to use something.
//!
//! The rule that matters most here is the one about secrets. A doctor
//! report is the thing people paste into bug reports, which is exactly why
//! it has to be safe to paste. `zorp-agent/src/doctor.rs` has the unit test
//! on the key check itself; this drives the whole binary and greps the
//! entire output, because a leak could come from any line.

use std::io::Write;
use std::net::TcpListener;
use std::process::Command;
use std::thread;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_zorp-agent")
}

/// An endpoint that answers a models listing, which is what the probe asks
/// for.
fn answering_endpoint() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let body = br#"{"data":[{"id":"demo"}]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(body);
            let _ = stream.flush();
            let _ = stream.shutdown(std::net::Shutdown::Write);
        }
    });
    format!("http://{addr}/v1")
}

/// A loopback address with nothing listening on it.
fn dead_endpoint() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    format!("http://{addr}/v1")
}

struct Run {
    stdout: String,
    code: Option<i32>,
}

fn doctor(base: &str, key: Option<&str>) -> Run {
    let dir = tempfile::tempdir().unwrap();
    let mut command = Command::new(bin());
    command
        .arg("doctor")
        .env("ZORP_BASE_URL", base)
        .env("ZORP_MODEL", "demo")
        .env("ZORP_STATE_DB", dir.path().join("s.db"))
        .env("ZORP_TRUST_FILE", dir.path().join("trust"))
        .env_remove("ZORP_API_KEY");
    if let Some(key) = key {
        command.env("ZORP_API_KEY", key);
    }
    let out = command.output().unwrap();
    Run {
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        code: out.status.code(),
    }
}

/// The whole point of the exit code: usable in a script.
#[test]
fn a_reachable_endpoint_exits_zero_and_an_unreachable_one_does_not() {
    let good = doctor(&answering_endpoint(), None);
    assert_eq!(good.code, Some(0), "{}", good.stdout);
    assert!(good.stdout.contains("ok  reachable"), "{}", good.stdout);

    let bad = doctor(&dead_endpoint(), None);
    assert_eq!(bad.code, Some(1), "{}", bad.stdout);
    assert!(bad.stdout.contains("bad reachable"), "{}", bad.stdout);
    assert!(bad.stdout.contains("did not answer"), "{}", bad.stdout);
}

/// **Nothing here prints a secret.** Not the key, not a prefix of it, not
/// its length. This greps the whole report rather than one line, because a
/// leak could come from any of them.
#[test]
fn the_report_never_carries_the_api_key() {
    let key = "sk-proj-do-not-print-me-abcdef0123456789";
    let run = doctor(&answering_endpoint(), Some(key));

    assert!(!run.stdout.contains(key), "{}", run.stdout);
    assert!(!run.stdout.contains("sk-proj"), "{}", run.stdout);
    assert!(!run.stdout.contains("abcdef0123456789"), "{}", run.stdout);
    assert!(
        !run.stdout.contains(&key.len().to_string()),
        "the key's length is in the report: {}",
        run.stdout
    );
    // It still says whether one is set, which is the useful half.
    assert!(run.stdout.contains("ZORP_API_KEY is set"), "{}", run.stdout);
}

/// A feature nobody compiled in is a choice, not a fault, or every default
/// build would exit non-zero and the code would stop meaning anything.
#[test]
fn a_default_build_is_not_a_failing_build() {
    let run = doctor(&answering_endpoint(), None);

    assert_eq!(run.code, Some(0), "{}", run.stdout);
    assert!(run.stdout.contains("features"), "{}", run.stdout);
    // `web_search` is off in a default build and says so without going red.
    if !cfg!(feature = "search") {
        assert!(run.stdout.contains("off web_search"), "{}", run.stdout);
    }
}

/// `ZORP_STATE_DB` and `XDG_STATE_HOME` mean somebody can easily be looking
/// at a different database than they think, and nothing else tells them.
#[test]
fn the_report_says_where_the_state_files_are() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("elsewhere.db");
    let out = Command::new(bin())
        .arg("doctor")
        .env("ZORP_BASE_URL", answering_endpoint())
        .env("ZORP_MODEL", "demo")
        .env("ZORP_STATE_DB", &db)
        .env("ZORP_TRUST_FILE", dir.path().join("trust"))
        .env_remove("ZORP_API_KEY")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);

    assert!(text.contains(&db.display().to_string()), "{text}");
    assert!(text.contains("trust file"), "{text}");
}

/// The tools are observed rather than re-derived, so the report cannot
/// disagree with what would actually be registered.
#[test]
fn the_report_lists_the_registered_tools() {
    let run = doctor(&answering_endpoint(), None);

    assert!(run.stdout.contains("tools"), "{}", run.stdout);
    assert!(run.stdout.contains("read_file"), "{}", run.stdout);
    assert!(run.stdout.contains("run_command"), "{}", run.stdout);
}

/// A model nobody set is the other common reason nothing works, and it is a
/// fault rather than a note.
#[test]
fn no_model_configured_is_a_fault() {
    let dir = tempfile::tempdir().unwrap();
    let out = Command::new(bin())
        .arg("doctor")
        .env("ZORP_BASE_URL", answering_endpoint())
        .env("ZORP_MODEL", "")
        .env("ZORP_STATE_DB", dir.path().join("s.db"))
        .env("ZORP_TRUST_FILE", dir.path().join("trust"))
        .env_remove("ZORP_API_KEY")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);

    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(text.contains("bad model"), "{text}");
}
