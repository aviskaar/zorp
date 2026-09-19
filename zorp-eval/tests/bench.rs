//! `zorp-eval bench` end to end: the real binary, a scripted provider on
//! loopback, and a runtime with nothing listening. No network and no key.

use std::path::Path;
use std::process::Command;

use serde_json::json;
use zorp_stub::{scripted_server, Ending, Framing, Reply};

fn answer(text: &str) -> Reply {
    Reply::Scripted {
        events: vec![
            json!({"choices": [{"delta": {"content": text}}]}).to_string(),
            json!({"choices": [{"delta": {}, "finish_reason": "stop"}]}).to_string(),
            json!({"choices": [], "usage": {"prompt_tokens": 40, "completion_tokens": 6}})
                .to_string(),
        ]
        .into(),
        ending: Ending::Done,
    }
}

#[test]
fn bench_prints_one_table_and_an_unreachable_runtime_is_never_a_zero() {
    let root = tempfile::tempdir().unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bench");
    let cases = root.path().join("cases");
    std::fs::create_dir_all(&cases).unwrap();
    for (name, format, file) in [
        ("mmlu", "mmlu", "mmlu.jsonl"),
        ("gsm8k", "gsm8k", "gsm8k.jsonl"),
    ] {
        std::fs::write(
            cases.join(format!("{name}.toml")),
            format!(
                "format = \"{format}\"\n[source]\npath = {:?}\n[bounds]\ntimeout_secs = 10\nretry_attempts = 1\n",
                fixtures.join(file).display().to_string()
            ),
        )
        .unwrap();
    }

    // Cases run in file name order (gsm8k, then mmlu), each across the
    // runtimes in manifest order, so the stub only ever sees the live one:
    // two gsm8k items, then three mmlu items.
    let (address, _) = scripted_server(
        Framing::Chunked,
        vec![
            answer("3 * 4 = 12\nAnswer: 12"),
            answer("Answer: 1,000"),
            answer("Answer: B"),
            answer("Answer: A"),
            answer("Answer: D"),
        ],
    );
    let dead = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let manifest = root.path().join("manifest.yaml");
    std::fs::write(
        &manifest,
        format!(
            "schema_version: zorp.compat/v1\nexperiment:\n  id: e2e\n  repetitions: 1\n\
             reference:\n  id: live\n  reasoning_mode: \"\"\n  model: stub\n  base_url: http://{address}/v1\n\
             candidates:\n  - id: down\n    reasoning_mode: \"\"\n    model: stub\n    base_url: http://127.0.0.1:{dead}/v1\n"
        ),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_zorp-eval"))
        .args(["bench", "--manifest"])
        .arg(&manifest)
        .arg("--cases")
        .arg(&cases)
        .arg("--db")
        .arg(root.path().join("telemetry.db"))
        .arg("--cache")
        .arg(root.path().join("cache"))
        // Inherited, and not the bench's to keep.
        .env("ZORP_RETRY_ATTEMPTS", "9")
        .env("ZORP_BENCH_E2E_SENTINEL", "1")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stdout}\n{stderr}");
    println!("{stdout}");

    assert!(
        stderr.contains("cleared inherited")
            && stderr.contains("ZORP_BENCH_E2E_SENTINEL")
            && stderr.contains("ZORP_RETRY_ATTEMPTS"),
        "{stderr}"
    );

    let row = |benchmark: &str, runtime: &str| -> Vec<String> {
        stdout
            .lines()
            .find(|l| {
                let cells: Vec<&str> = l.split('|').map(str::trim).collect();
                cells.get(1) == Some(&benchmark) && cells.get(2) == Some(&runtime)
            })
            .unwrap_or_else(|| panic!("no {benchmark}/{runtime} row in\n{stdout}"))
            .split('|')
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty())
            .collect()
    };
    // benchmark, runtime, attempted, scored, unevaluable, correct, accuracy, unparsed, ...
    let live = row("gsm8k", "live");
    assert_eq!(&live[2..8], ["2", "2", "0", "1", "50.0%", "0"]);
    let live = row("mmlu", "live");
    assert_eq!(&live[2..8], ["3", "3", "0", "1", "33.3%", "0"]);
    for benchmark in ["gsm8k", "mmlu"] {
        let down = row(benchmark, "down");
        let attempted = if benchmark == "mmlu" { "3" } else { "2" };
        assert_eq!(
            &down[2..8],
            [attempted, "0", attempted, "0", "n/a", "0"],
            "{benchmark}"
        );
    }
    assert!(stdout.contains("mmlu on down: 3 unreachable"), "{stdout}");
    assert!(stdout.contains("contamination"), "{stdout}");
}

#[test]
fn bench_refuses_an_empty_case_directory() {
    let root = tempfile::tempdir().unwrap();
    let manifest = root.path().join("manifest.yaml");
    std::fs::write(
        &manifest,
        "schema_version: zorp.compat/v1\nexperiment:\n  id: e\n  repetitions: 1\nreference:\n  id: r\n  reasoning_mode: \"\"\n  model: m\ncandidates: []\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_zorp-eval"))
        .args(["bench", "--manifest"])
        .arg(&manifest)
        .arg("--cases")
        .arg(root.path())
        .arg("--cache")
        .arg(root.path().join("cache"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no .toml bench cases"), "{stderr}");
}
