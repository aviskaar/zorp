//! System One decision models over Ollama's `POST /v1/systemone`.
//!
//! A decision model is not a chat model: it takes a `state` plus a schema of
//! typed questions and returns a probability per option. It is one
//! non-streaming request, so it goes through `zorp::http_agent` and
//! `zorp::send_json` with no new transport, under the same retry rule as
//! everything else (only 429, 502 and 503 retry; 400, 404, 413 and 500 do
//! not). See issue #279 and `docs/decisions` for the placement decision.
//!
//! The one rule that shapes everything here: a probability a model returns is
//! model-authored, so nothing that decides may read it as evidence (the repo
//! rule that detection is code and a model only interprets). That is why this
//! is exposed as a tool the model calls and reads back, never as a gate whose
//! numbers code consumes. The `decide` tool hands the answers to the model as
//! a tool result and no detector, recall, memory or title path ever sees them.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

use crate::tools::{cap_output, Context, Tool, ToolError, ToolOutput, ToolResult};

/// The default Ollama endpoint, the same loopback the chat model defaults to.
const DEFAULT_BASE: &str = "http://localhost:11434/v1";

/// Ceiling on the rendered answer text handed back to the model.
const MAX_CONTENT_BYTES: usize = 16_000;

/// A `POST /v1/systemone` request. `state` and `questions` are passed through
/// as given: the API is open-ended (three question types, nested criteria) and
/// a typed mirror here would only drift from it. Validation that does not need
/// the server (one to 64 questions) happens before the send.
#[derive(Debug, Serialize)]
pub struct DecisionRequest {
    pub model: String,
    pub state: Value,
    pub questions: Map<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_alive: Option<Value>,
}

/// Token counts the server reports. Code-derived, so these are safe to read.
#[derive(Debug, Deserialize, Default)]
pub struct Usage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
}

/// One answer. The common fields are `type`, `probabilities` and `confidence`;
/// the rest are present for the question type that has them. All model-authored
/// and for the model to interpret, never for code to decide on.
#[derive(Debug, Deserialize)]
pub struct Answer {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub probabilities: Map<String, Value>,
    #[serde(default)]
    pub confidence: Option<f64>,
    #[serde(default)]
    pub choice: Option<String>,
    #[serde(default)]
    pub noul: Option<f64>,
    #[serde(default)]
    pub score: Option<i64>,
    #[serde(default)]
    pub legend: Option<Value>,
}

/// A `POST /v1/systemone` 200 response.
#[derive(Debug, Deserialize)]
pub struct DecisionResponse {
    pub model: String,
    #[serde(default)]
    pub usage: Usage,
    pub answers: BTreeMap<String, Answer>,
}

/// What can go wrong, kept apart so a broken request and an unreachable or
/// unparseable one are never reported as an empty or wrong set of answers.
#[derive(Debug)]
pub enum SystemOneError {
    /// 1 to 64 questions is the API's own bound; a request outside it is
    /// refused here rather than sent to be 400'd.
    BadRequest(String),
    /// The endpoint could not be reached, or returned a non-2xx status. The
    /// transport has already applied the retry rule.
    Transport(String),
    /// A 2xx body that did not parse as a decision response. Not a wrong
    /// answer: no answer at all.
    Malformed(String),
}

impl std::fmt::Display for SystemOneError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SystemOneError::BadRequest(m) => write!(f, "{m}"),
            SystemOneError::Transport(m) => {
                write!(f, "the decision endpoint could not be reached: {m}")
            }
            SystemOneError::Malformed(m) => write!(
                f,
                "the decision endpoint returned a reply this could not read: {m}"
            ),
        }
    }
}

impl std::error::Error for SystemOneError {}

/// The Ollama endpoint System One calls, resolved like the other model
/// settings: `ZORP_SYSTEMONE_URL` for a System One server that is not the chat
/// one, else `ZORP_BASE_URL`, else the saved config, else the loopback default.
///
/// Read from the environment and the saved file only, never from a flavor, for
/// the reason the search provider is: a workspace file the model can write must
/// not move where a person's `state` (which can hold their files) is sent.
pub fn resolve_base() -> String {
    for var in ["ZORP_SYSTEMONE_URL", "ZORP_BASE_URL"] {
        if let Ok(v) = std::env::var(var) {
            if !v.trim().is_empty() {
                return v;
            }
        }
    }
    crate::config::load()
        .and_then(|s| s.base_url)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_BASE.to_string())
}

/// The decision model named by `ZORP_SYSTEMONE_MODEL`, if any. There is no
/// default: a decision model is a specific local GGUF a person pulled, and
/// guessing one that is not loaded would turn a missing setting into a 404.
pub fn default_model() -> Option<String> {
    std::env::var("ZORP_SYSTEMONE_MODEL")
        .ok()
        .filter(|s| !s.trim().is_empty())
}

/// Send one decision request to `base_url` (which ends in `/v1`), returning the
/// parsed answers. The retry rule is the transport's; this adds only the
/// 1-to-64-questions bound and turns an unparseable 2xx into an error.
pub fn decide(base_url: &str, req: &DecisionRequest) -> Result<DecisionResponse, SystemOneError> {
    let n = req.questions.len();
    if !(1..=64).contains(&n) {
        return Err(SystemOneError::BadRequest(format!(
            "a decision call takes 1 to 64 questions, got {n}"
        )));
    }
    let url = format!("{}/systemone", base_url.trim_end_matches('/'));
    let body = serde_json::to_value(req)
        .map_err(|e| SystemOneError::BadRequest(format!("could not encode the request: {e}")))?;
    let resp = zorp::send_json(zorp::http_agent().post(&url), body)
        .map_err(|e| SystemOneError::Transport(e.to_string()))?;
    let text = resp
        .into_string()
        .map_err(|e| SystemOneError::Transport(e.to_string()))?;
    serde_json::from_str(&text).map_err(|e| SystemOneError::Malformed(e.to_string()))
}

/// The Ollama root for a base URL, so `/api/version` can be reached: the chat
/// base ends in `/v1` and the version endpoint is not under it.
pub fn ollama_root(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    trimmed.strip_suffix("/v1").unwrap_or(trimmed).to_string()
}

/// Whether `version` is at least `major.minor`. A version that does not parse
/// is treated as new enough: a doctor should not fail a build over a version
/// string it cannot read (a git or nightly build), only over one it can read
/// and that is clearly too old.
pub fn version_at_least(version: &str, major: u64, minor: u64) -> bool {
    let mut parts = version.trim().trim_start_matches('v').split('.');
    let parsed = (
        parts.next().and_then(|p| p.parse::<u64>().ok()),
        parts.next().and_then(|p| p.parse::<u64>().ok()),
    );
    match parsed {
        (Some(maj), Some(min)) => (maj, min) >= (major, minor),
        _ => true,
    }
}

/// Probe Ollama's `GET /api/version`, for `doctor`. Its own short-timeout
/// agent, the way the other diagnostic probes build their own: a diagnostic
/// that hangs is worse than one that says it could not tell. Reads no key and
/// returns none.
pub fn ollama_version(base_url: &str, timeout: std::time::Duration) -> Result<String, String> {
    let url = format!("{}/api/version", ollama_root(base_url));
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(timeout)
        .timeout_read(timeout)
        .build();
    let resp = agent.get(&url).call().map_err(|e| e.to_string())?;
    let body: Value = resp.into_json().map_err(|e| e.to_string())?;
    body.get("version")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "the version endpoint returned no version".to_string())
}

/// Render the answers as the text the model reads back. Code-derived fields
/// only; the model does the interpreting.
fn render(resp: &DecisionResponse) -> String {
    let mut out = format!("model: {}\n", resp.model);
    for (name, answer) in &resp.answers {
        out.push_str(&format!("\n{name} ({}):", answer.kind));
        if let Some(choice) = &answer.choice {
            out.push_str(&format!(" choice={choice}"));
        }
        if let Some(noul) = answer.noul {
            out.push_str(&format!(" true={noul:.3}"));
        }
        if let Some(score) = answer.score {
            out.push_str(&format!(" score={score}"));
        }
        if let Some(confidence) = answer.confidence {
            out.push_str(&format!(" confidence={confidence:.3}"));
        }
        if !answer.probabilities.is_empty() {
            let mut parts: Vec<String> = answer
                .probabilities
                .iter()
                .map(|(k, v)| format!("{k}={}", v))
                .collect();
            parts.sort();
            out.push_str(&format!("\n  probabilities: {}", parts.join(", ")));
        }
    }
    out
}

/// The `decide` tool: the agent hands a state and typed questions to a local
/// decision model and reads the probabilities back to interpret. A networked
/// built-in, so it is gated behind the non-default `systemone` feature the way
/// `web_search` is behind `search`.
pub struct Decide {
    base_url: String,
    default_model: Option<String>,
}

impl Decide {
    pub fn new(base_url: String, default_model: Option<String>) -> Self {
        Decide {
            base_url,
            default_model,
        }
    }

    /// From the resolved environment and saved config.
    pub fn from_env() -> Self {
        Decide::new(resolve_base(), default_model())
    }
}

impl Tool for Decide {
    fn name(&self) -> &str {
        "decide"
    }

    fn description(&self) -> &str {
        "Ask a local System One decision model to classify or score a text. \
         Give the text as `state` and one or more named `questions`, each a \
         `choice` (pick from options), `noul` (true/false), or `score` (rubric \
         levels). Returns a probability per option for you to interpret; it \
         does not explain or quote."
    }

    fn schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "state": {
                    "description": "the text, object or array every question is about"
                },
                "questions": {
                    "type": "object",
                    "description": "1 to 64 named questions, each {type: choice|noul|score, instructions, criteria}"
                },
                "model": {
                    "type": "string",
                    "description": "the decision model name; defaults to ZORP_SYSTEMONE_MODEL"
                }
            },
            "required": ["state", "questions"]
        })
    }

    fn run(&self, args: &Value, _cx: &mut Context) -> ToolResult {
        let state = args
            .get("state")
            .cloned()
            .filter(|s| !s.is_null())
            .ok_or_else(|| ToolError::new("decide: 'state' is required"))?;
        let questions = args
            .get("questions")
            .and_then(Value::as_object)
            .filter(|q| !q.is_empty())
            .ok_or_else(|| ToolError::new("decide: 'questions' is required and must not be empty"))?
            .clone();
        let model = args
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| self.default_model.clone())
            .ok_or_else(|| {
                ToolError::new(
                    "decide: no model given and ZORP_SYSTEMONE_MODEL is not set, so there is no \
                     decision model to call",
                )
            })?;

        let request = DecisionRequest {
            model,
            state,
            questions,
            images: None,
            keep_alive: None,
        };
        let response =
            decide(&self.base_url, &request).map_err(|e| ToolError::new(e.to_string()))?;
        let summary = format!("{} question(s)", response.answers.len());
        Ok(ToolOutput::new(
            cap_output(&render(&response), MAX_CONTENT_BYTES),
            summary,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// A one-shot HTTP/1.1 server that reads a request and writes `response`,
    /// returning its `http://127.0.0.1:PORT/v1` base. Enough to drive the real
    /// client over a real socket without a live Ollama.
    fn server(response: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        format!("http://{addr}/v1")
    }

    fn one_choice() -> Map<String, Value> {
        let mut q = Map::new();
        q.insert(
            "label".to_string(),
            serde_json::json!({
                "type": "choice",
                "instructions": "which label fits?",
                "criteria": {"bug": "software errors", "billing": "payments"}
            }),
        );
        q
    }

    fn request() -> DecisionRequest {
        DecisionRequest {
            model: "nimble".to_string(),
            state: serde_json::json!("checkout returns 500s"),
            questions: one_choice(),
            images: None,
            keep_alive: None,
        }
    }

    #[test]
    fn a_well_formed_reply_parses() {
        let body = r#"{"model":"nimble","usage":{"input_tokens":12,"output_tokens":3},"answers":{"label":{"type":"choice","probabilities":{"bug":0.91,"billing":0.09},"confidence":0.91,"choice":"bug"}}}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let base = server(Box::leak(response.into_boxed_str()));
        let out = decide(&base, &request()).expect("should parse");
        assert_eq!(out.model, "nimble");
        assert_eq!(
            out.answers.get("label").unwrap().choice.as_deref(),
            Some("bug")
        );
        assert_eq!(out.usage.input_tokens, 12);
    }

    #[test]
    fn a_malformed_reply_is_an_error_not_an_empty_answer() {
        let body = "not json at all";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let base = server(Box::leak(response.into_boxed_str()));
        let err = decide(&base, &request()).expect_err("malformed must error");
        assert!(matches!(err, SystemOneError::Malformed(_)), "{err}");
    }

    #[test]
    fn a_400_is_a_transport_error_and_is_not_retried() {
        let body = r#"{"error":"bad request"}"#;
        let response = format!(
            "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let base = server(Box::leak(response.into_boxed_str()));
        let err = decide(&base, &request()).expect_err("400 must error");
        assert!(matches!(err, SystemOneError::Transport(_)), "{err}");
    }

    #[test]
    fn zero_or_too_many_questions_is_refused_before_sending() {
        let mut req = request();
        req.questions = Map::new();
        assert!(matches!(
            decide("http://127.0.0.1:1/v1", &req),
            Err(SystemOneError::BadRequest(_))
        ));
    }

    #[test]
    fn ollama_root_strips_the_v1_suffix() {
        assert_eq!(
            ollama_root("http://localhost:11434/v1"),
            "http://localhost:11434"
        );
        assert_eq!(
            ollama_root("http://localhost:11434/v1/"),
            "http://localhost:11434"
        );
        assert_eq!(ollama_root("http://host/custom"), "http://host/custom");
    }

    #[test]
    fn version_at_least_reads_major_minor_and_is_lenient_on_the_unparseable() {
        assert!(version_at_least("0.35.1", 0, 35));
        assert!(version_at_least("0.36.0", 0, 35));
        assert!(version_at_least("1.0.0", 0, 35));
        assert!(!version_at_least("0.34.9", 0, 35));
        assert!(!version_at_least("0.12.0", 0, 35));
        // A version string with no readable major.minor is treated as new
        // enough rather than failing a build over something it cannot read.
        assert!(version_at_least("weird", 0, 35));
        assert!(version_at_least("v1", 0, 35));
    }

    #[test]
    fn the_tool_requires_state_and_questions_and_a_model() {
        let tool = Decide::new("http://127.0.0.1:1/v1".to_string(), None);
        let mut cx = Context::new(
            std::path::PathBuf::from("."),
            crate::sandbox::cancel_token(),
        );
        // No model available: the call is refused with a readable message
        // rather than sent to 404.
        let result = tool.run(
            &serde_json::json!({"state": "x", "questions": {"q": {"type": "noul"}}}),
            &mut cx,
        );
        match result {
            Err(e) => assert!(e.message.contains("ZORP_SYSTEMONE_MODEL"), "{}", e.message),
            Ok(_) => panic!("a call with no model should be refused"),
        }
    }
}
