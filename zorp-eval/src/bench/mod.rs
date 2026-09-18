//! The benchmark part of `zorp-eval`.
//!
//! `harness` proves the code with no model in the loop and gates every pull
//! request. `compat` asks a behavioural question of live models and gates
//! nothing. `bench` is the third kind: live models, real network, scored on
//! public benchmarks, one table across the runtimes in a manifest. It reuses
//! the manifest `compat` reads (reference plus candidates, `api_key_env`
//! naming a variable and never holding a key, `repetitions`) and writes into
//! the runner's telemetry database, so the table is a query over rows the
//! runner already knows how to key, not a second store.
//!
//! One rule decides everything else here: a measurement that did not happen
//! is not a zero. Every item ends in one of three states, correct, incorrect
//! or unevaluable, and the table puts the attempted count beside the scored
//! count so a row that scored 40 of 400 says so on the page. See `client.rs`
//! for what counts as unevaluable and `report.rs` for the table.
//!
//! It never gates a merge, for the reason `compat` does not: an upstream
//! having a bad day would fail it for reasons that have nothing to do with
//! the code. See `docs/DECISIONS.md` (2026-09-18).
//!
//! What it does not measure, on purpose: perplexity, memory use and
//! active-parameter counts need local weights, and no API endpoint reports
//! them. Code-execution benchmarks (HumanEval, LiveCodeBench) need a sandbox
//! and a timeout per item and are not here yet either.

pub mod case;
pub mod client;
pub mod dataset;
pub mod grade;
pub mod report;

use std::path::PathBuf;

use client::{Endpoint, Wire};
use report::{Outcome, Record};

/// What one bench run reads and writes.
#[derive(Debug, Clone)]
pub struct Options {
    pub manifest: PathBuf,
    pub cases: PathBuf,
    pub db: PathBuf,
    /// Where fetched datasets are cached. Never inside the tree.
    pub cache: PathBuf,
    /// The Hugging Face datasets server. A field so a test can serve pages
    /// from loopback.
    pub datasets_server: String,
}

/// Everything a run needs that is read from outside the process, resolved
/// before any request is sent and before the environment is cleared: the
/// manifest, each runtime's key, and the cases.
pub struct Plan {
    options: Options,
    experiment_id: String,
    repetitions: u32,
    runtimes: Vec<(String, Endpoint)>,
    cases: Vec<case::Loaded>,
}

/// What a run produced.
pub struct Outcomes {
    pub session: String,
    pub rows: Vec<report::Row>,
    pub table: String,
}

impl Plan {
    pub fn prepare(options: Options) -> anyhow::Result<Self> {
        let manifest = crate::manifest::load_manifest(&options.manifest)
            .map_err(|e| anyhow::anyhow!("{}: {e}", options.manifest.display()))?;
        if manifest.experiment.repetitions == 0 {
            anyhow::bail!("experiment.repetitions is 0, which is a run that measures nothing");
        }
        let mut runtimes = Vec::new();
        for runtime in std::iter::once(&manifest.reference).chain(&manifest.candidates) {
            if runtimes.iter().any(|(id, _)| id == &runtime.id) {
                anyhow::bail!("two runtimes are both called {:?}", runtime.id);
            }
            runtimes.push((runtime.id.clone(), endpoint(runtime)?));
        }
        let cases = case::load_dir(&options.cases)?;
        Ok(Self {
            experiment_id: manifest.experiment.id,
            repetitions: manifest.experiment.repetitions,
            runtimes,
            cases,
            options,
        })
    }

    /// Load every case's items, then ask every runtime every item as many
    /// times as the manifest says, and build the table.
    pub fn run(self) -> anyhow::Result<Outcomes> {
        // Every dataset is read before anything is sent, so a missing file
        // or a schema change fails the run before it costs anything.
        let mut suites = Vec::new();
        for case in &self.cases {
            let items = dataset::load(case, &self.options.cache, &self.options.datasets_server)?;
            suites.push((case, items));
        }

        let conn = crate::runner::init_db(&self.options.db)?;
        report::init_schema(&conn)?;
        let session = session_id();

        for (case, items) in &suites {
            let bounds = &case.case.bounds;
            // GPQA's text stays out of the database: its authors ask that
            // items not be kept anywhere they could be scraped, and a reply
            // often quotes the question back.
            let keep_text = case.case.format != case::Format::Gpqa;
            for (runtime_id, endpoint) in &self.runtimes {
                eprintln!(
                    "bench: {} on {runtime_id}: {} items x {} repetitions",
                    case.name,
                    items.len(),
                    self.repetitions
                );
                for item in items {
                    let prompt = grade::prompt(item);
                    let expected = item.key.expected();
                    for repetition in 0..self.repetitions {
                        let asked = client::ask(endpoint, bounds, &prompt);
                        let graded = asked.as_ref().ok().map(|a| grade::grade(item, &a.text));
                        let record = match (&asked, &graded) {
                            (Ok(answer), Some(graded)) => Record {
                                session: &session,
                                experiment_id: &self.experiment_id,
                                runtime_id,
                                benchmark: &case.name,
                                item_id: &item.id,
                                repetition,
                                outcome: if graded.correct {
                                    Outcome::Correct
                                } else {
                                    Outcome::Incorrect
                                },
                                unparsed: graded.extracted.is_none(),
                                reason: None,
                                http_status: None,
                                detail: None,
                                expected: &expected,
                                extracted: graded.extracted.as_deref(),
                                response: keep_text.then_some(answer.text.as_str()),
                                prompt_tokens: answer.prompt_tokens,
                                completion_tokens: answer.completion_tokens,
                                ttft_ms: answer.ttft_ms,
                                latency_ms: answer.latency_ms,
                                sends: answer.sends,
                            },
                            (Err(missing), _) => Record {
                                session: &session,
                                experiment_id: &self.experiment_id,
                                runtime_id,
                                benchmark: &case.name,
                                item_id: &item.id,
                                repetition,
                                outcome: Outcome::Unevaluable,
                                unparsed: false,
                                reason: Some(missing.reason.code()),
                                http_status: missing.http_status,
                                detail: keep_text.then_some(missing.detail.as_str()),
                                expected: &expected,
                                extracted: None,
                                response: None,
                                prompt_tokens: None,
                                completion_tokens: None,
                                ttft_ms: None,
                                latency_ms: missing.latency_ms,
                                sends: missing.sends,
                            },
                            (Ok(_), None) => unreachable!("an answer is always graded"),
                        };
                        report::insert(&conn, &record)?;
                    }
                }
            }
        }

        let benchmarks: Vec<String> = self.cases.iter().map(|c| c.name.clone()).collect();
        let runtimes: Vec<String> = self.runtimes.iter().map(|(id, _)| id.clone()).collect();
        let rows = report::rows(&conn, &session, &benchmarks, &runtimes)?;
        let table = report::render(&session, &rows);
        Ok(Outcomes {
            session,
            rows,
            table,
        })
    }
}

/// One runtime from the manifest, resolved. The key is read here, once,
/// before the environment is cleared, and a variable the manifest names but
/// nobody set is an error now rather than a table of 401s later.
fn endpoint(runtime: &crate::manifest::RuntimeConfig) -> anyhow::Result<Endpoint> {
    let id = &runtime.id;
    let wire = match runtime.provider.as_deref().unwrap_or("openai") {
        "openai" => Wire::OpenAi,
        "anthropic" | "claude" => Wire::Anthropic,
        other => anyhow::bail!("runtime {id}: unknown provider {other:?}"),
    };
    let model = runtime.model.clone().ok_or_else(|| {
        anyhow::anyhow!(
            "runtime {id}: bench needs a model; the agent's default is not a benchmark subject"
        )
    })?;
    let base = runtime.base_url.clone().unwrap_or_else(|| {
        match wire {
            Wire::OpenAi => "https://api.openai.com/v1",
            Wire::Anthropic => "https://api.anthropic.com/v1",
        }
        .to_string()
    });
    let url = zorp::join_url(
        &base,
        match wire {
            Wire::OpenAi => "chat/completions",
            Wire::Anthropic => "messages",
        },
    );
    let api_key = match &runtime.api_key_env {
        None => None,
        Some(var) => Some(std::env::var(var).map_err(|_| {
            anyhow::anyhow!("runtime {id}: api_key_env names {var}, which is not set")
        })?),
    };
    let reasoning_mode = if runtime.reasoning_mode.trim().is_empty() {
        None
    } else {
        Some(
            client::ReasoningMode::parse(&runtime.reasoning_mode)
                .map_err(|e| anyhow::anyhow!("runtime {id}: {e}"))?,
        )
    };
    Ok(Endpoint {
        wire,
        url,
        model,
        api_key,
        reasoning_mode,
    })
}

/// A name for one invocation, so a database that holds several bench runs
/// reports each on its own.
fn session_id() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("bench-{}-{}", now.as_secs(), std::process::id())
}

/// The `ZORP_` variables in `vars`, sorted: the ones bench clears.
pub fn inherited_zorp_vars(vars: impl IntoIterator<Item = (String, String)>) -> Vec<String> {
    let mut names: Vec<String> = vars
        .into_iter()
        .map(|(k, _)| k)
        .filter(|k| k.starts_with("ZORP_"))
        .collect();
    names.sort();
    names
}

/// Clear every inherited `ZORP_` variable from this process and return their
/// names. Called by `main` after [`Plan::prepare`] has read what it needs
/// (a key variable may itself be a `ZORP_` one) and before anything is sent,
/// while the process is still single threaded.
///
/// `harness` does this to a child's environment. Bench sends from this
/// process, through `zorp::http_agent`, whose read timeout is read from the
/// environment the first time it is built, so it is this process's own
/// environment that has to be clean. A case states its bounds as fields
/// instead.
pub fn clear_inherited_zorp_env() -> Vec<String> {
    let names = inherited_zorp_vars(std::env::vars());
    for name in &names {
        std::env::remove_var(name);
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use zorp_stub::{scripted_server, Ending, Framing, Reply};

    fn answer(text: &str) -> Reply {
        Reply::Scripted {
            events: vec![
                serde_json::json!({"choices": [{"delta": {"content": text}}]}).to_string(),
                serde_json::json!({"choices": [{"delta": {}, "finish_reason": "stop"}]})
                    .to_string(),
                serde_json::json!({"choices": [], "usage": {"prompt_tokens": 40, "completion_tokens": 8}})
                    .to_string(),
            ]
            .into(),
            ending: Ending::Done,
        }
    }

    /// A port nothing is listening on: bound, read, and let go.
    fn dead_port() -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    }

    fn setup(root: &Path, runtimes: &str, case: &str) -> Options {
        let cases = root.join("cases");
        std::fs::create_dir_all(&cases).unwrap();
        std::fs::write(cases.join("mmlu.toml"), case).unwrap();
        let manifest = root.join("manifest.yaml");
        std::fs::write(
            &manifest,
            format!(
                "schema_version: zorp.compat/v1\nexperiment:\n  id: bench-test\n  repetitions: 1\n{runtimes}"
            ),
        )
        .unwrap();
        Options {
            manifest,
            cases,
            db: root.join("telemetry.db"),
            cache: root.join("cache"),
            datasets_server: "http://127.0.0.1:9".into(),
        }
    }

    fn mmlu_case() -> String {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bench/mmlu.jsonl");
        format!(
            "format = \"mmlu\"\n[source]\npath = {:?}\n[bounds]\ntimeout_secs = 10\nretry_attempts = 1\n",
            fixture.display().to_string()
        )
    }

    /// The test the issue asks for by name. A runtime nobody is listening
    /// for produces unevaluable rows, and its accuracy is n/a, not 0.
    #[test]
    fn an_unreachable_runtime_is_unevaluable_and_never_scored_zero() {
        let root = tempfile::tempdir().unwrap();
        let (address, _) = scripted_server(
            Framing::Chunked,
            vec![answer("Answer: B"), answer("Answer: A"), answer("no idea")],
        );
        let runtimes = format!(
            "reference:\n  id: live\n  reasoning_mode: \"\"\n  model: stub\n  base_url: http://{address}/v1\n\
             candidates:\n  - id: down\n    reasoning_mode: \"\"\n    model: stub\n    base_url: http://127.0.0.1:{}/v1\n",
            dead_port()
        );
        let options = setup(root.path(), &runtimes, &mmlu_case());
        let out = Plan::prepare(options).unwrap().run().unwrap();

        let live = out.rows.iter().find(|r| r.runtime == "live").unwrap();
        assert_eq!((live.attempted, live.scored, live.unevaluable), (3, 3, 0));
        assert_eq!(live.correct, 1, "{}", out.table);
        assert_eq!(live.unparsed, 1);

        let down = out.rows.iter().find(|r| r.runtime == "down").unwrap();
        assert_eq!((down.attempted, down.scored, down.unevaluable), (3, 0, 3));
        assert_eq!(down.correct, 0);
        assert_eq!(down.accuracy(), None);
        assert_eq!(
            down.reasons.get("unreachable"),
            Some(&3),
            "{:?}",
            down.reasons
        );

        let line = out.table.lines().find(|l| l.contains("| down")).unwrap();
        assert!(line.contains("n/a") && !line.contains('%'), "{line}");
    }

    #[test]
    fn repetitions_multiply_attempts_and_land_in_the_runs_table() {
        let root = tempfile::tempdir().unwrap();
        let (address, connections) =
            scripted_server(Framing::CloseDelimited, vec![answer("Answer: B")]);
        let runtimes = format!(
            "reference:\n  id: live\n  reasoning_mode: \"\"\n  model: stub\n  base_url: http://{address}/v1\ncandidates: []\n"
        );
        let mut options = setup(root.path(), &runtimes, &mmlu_case());
        let text = std::fs::read_to_string(&options.manifest)
            .unwrap()
            .replace("repetitions: 1", "repetitions: 2");
        std::fs::write(&options.manifest, text).unwrap();
        options.db = root.path().join("nested/telemetry.db");
        let db = options.db.clone();
        let out = Plan::prepare(options).unwrap().run().unwrap();
        assert_eq!(out.rows[0].attempted, 6);
        assert_eq!(connections.load(std::sync::atomic::Ordering::SeqCst), 6);
        let conn = rusqlite::Connection::open(db).unwrap();
        let runs: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM runs WHERE suite = 'bench:mmlu' AND experiment_id = 'bench-test' AND repetition = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(runs, 3);
    }

    #[test]
    fn a_key_variable_nobody_set_fails_before_anything_is_sent() {
        let root = tempfile::tempdir().unwrap();
        let runtimes = "reference:\n  id: live\n  reasoning_mode: high\n  model: m\n  api_key_env: ZORP_BENCH_TEST_KEY_THAT_IS_NEVER_SET\ncandidates: []\n";
        let options = setup(root.path(), runtimes, &mmlu_case());
        let error = Plan::prepare(options).err().unwrap().to_string();
        assert!(error.contains("not set"), "{error}");
    }

    #[test]
    fn a_runtime_without_a_model_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let runtimes = "reference:\n  id: live\n  reasoning_mode: high\ncandidates: []\n";
        let options = setup(root.path(), runtimes, &mmlu_case());
        let error = Plan::prepare(options).err().unwrap().to_string();
        assert!(error.contains("needs a model"), "{error}");
    }

    #[test]
    fn a_bench_manifest_needs_no_contracts_section() {
        // `setup` writes none. compat still requires one; see runner.rs.
        let root = tempfile::tempdir().unwrap();
        let runtimes = "reference:\n  id: live\n  reasoning_mode: \"\"\n  model: m\n  base_url: http://127.0.0.1:9/v1\ncandidates: []\n";
        assert!(Plan::prepare(setup(root.path(), runtimes, &mmlu_case())).is_ok());
    }

    #[test]
    fn only_zorp_variables_are_cleared() {
        let names = inherited_zorp_vars([
            ("ZORP_RETRY_ATTEMPTS".to_string(), "9".to_string()),
            ("HOME".to_string(), "/h".to_string()),
            ("ZORP_HTTP_TIMEOUT_SECS".to_string(), "1".to_string()),
            ("OPENAI_API_KEY".to_string(), "k".to_string()),
        ]);
        assert_eq!(names, vec!["ZORP_HTTP_TIMEOUT_SECS", "ZORP_RETRY_ATTEMPTS"]);
    }
}
