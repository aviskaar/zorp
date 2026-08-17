mod common;
use common::mock;
use std::io::Write;
use std::process::{Command, Stdio};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_zorp")
}

#[test]
fn oneshot_buffered_joins_args() {
    let base = mock(
        200,
        "application/json",
        r#"{"choices":[{"message":{"content":"hi there"}}]}"#,
    );
    let out = Command::new(bin())
        .arg("say")
        .arg("hi")
        .env("ZORP_BASE_URL", &base)
        .env("ZORP_MODEL", "m")
        .env("ZORP_STREAM", "0")
        .env_remove("ZORP_API_KEY")
        .env_remove("ZORP_SYSTEM")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hi there\n");
}

#[test]
fn oneshot_streaming_prints_deltas() {
    let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"str\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"eam\"}}]}\n\ndata: [DONE]\n\n";
    let base = mock(200, "text/event-stream", sse);
    let out = Command::new(bin())
        .arg("go")
        .env("ZORP_BASE_URL", &base)
        .env("ZORP_MODEL", "m")
        .env("ZORP_STREAM", "1")
        .env_remove("ZORP_API_KEY")
        .env_remove("ZORP_SYSTEM")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "stream\n");
}

#[test]
fn repl_answers_one_line_then_eof() {
    let base = mock(
        200,
        "application/json",
        r#"{"choices":[{"message":{"content":"reply"}}]}"#,
    );
    let mut child = Command::new(bin())
        .env("ZORP_BASE_URL", &base)
        .env("ZORP_MODEL", "m")
        .env("ZORP_STREAM", "0")
        .env_remove("ZORP_API_KEY")
        .env_remove("ZORP_SYSTEM")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"hello\n").unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("reply"));
}

// The base URL in these three points at a closed port on purpose. If the
// flag were forwarded to the model the way any other argument is, the
// request would fail and the process would exit 1, so "succeeded" is the
// same assertion as "no request was made".
#[test]
fn version_flag_is_answered_locally() {
    let out = Command::new(bin())
        .arg("--version")
        .env("ZORP_BASE_URL", "http://127.0.0.1:1")
        .env("ZORP_MODEL", "m")
        .env("ZORP_API_KEY", "k")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(
        s.contains(env!("CARGO_PKG_VERSION")),
        "expected the version in stdout, got: {s}"
    );
}

#[test]
fn help_flag_is_answered_locally() {
    let out = Command::new(bin())
        .arg("--help")
        .env("ZORP_BASE_URL", "http://127.0.0.1:1")
        .env("ZORP_MODEL", "m")
        .env("ZORP_API_KEY", "k")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("--init"), "help should list --init, got: {s}");
    assert!(
        s.contains("ZORP_BASE_URL"),
        "help should name the config vars, got: {s}"
    );
}

#[test]
fn short_flags_are_answered_locally() {
    for flag in ["-V", "-h"] {
        let out = Command::new(bin())
            .arg(flag)
            .env("ZORP_BASE_URL", "http://127.0.0.1:1")
            .env("ZORP_MODEL", "m")
            .env("ZORP_API_KEY", "k")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{flag} stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

// A prompt is still a prompt. Only a leading flag is intercepted, so a
// question that happens to mention one is answered by the model.
#[test]
fn a_flag_later_in_the_prompt_still_goes_to_the_model() {
    let base = mock(
        200,
        "application/json",
        r#"{"choices":[{"message":{"content":"answered"}}]}"#,
    );
    let out = Command::new(bin())
        .arg("what does")
        .arg("--version")
        .arg("print")
        .env("ZORP_BASE_URL", &base)
        .env("ZORP_MODEL", "m")
        .env("ZORP_STREAM", "0")
        .env_remove("ZORP_API_KEY")
        .env_remove("ZORP_SYSTEM")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "answered\n");
}

#[test]
fn init_prints_exports() {
    let mut child = Command::new(bin())
        .arg("--init")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"http://localhost:11434/v1\n\nqwen\n\n")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("export ZORP_BASE_URL='http://localhost:11434/v1'"));
    assert!(s.contains("export ZORP_MODEL='qwen'"));
    assert!(!s.contains("ZORP_API_KEY"));
}

#[test]
fn init_escapes_special_chars() {
    let mut child = Command::new(bin())
        .arg("--init")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    // base, (blank key), model, system-with-quote-and-dollar
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"http://x/v1\n\nm\nit's $HOME\n")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let s = String::from_utf8_lossy(&out.stdout);
    // Single-quoted, with the embedded ' escaped as '\'' — inert under eval.
    assert!(
        s.contains(r#"export ZORP_SYSTEM='it'\''s $HOME'"#),
        "got: {s}"
    );
}
