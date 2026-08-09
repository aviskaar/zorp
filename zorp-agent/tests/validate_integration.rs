//! End-to-end integration test for `validate::run`, using a stub MCP
//! search server (a separate binary fixture, see
//! `tests/fixtures/stub_search_mcp_server.rs`) and a stub `Model`, so
//! the whole validate pipeline can be proven without a real LLM or a
//! real network search.
//!
//! This whole file compiles to nothing when the `research` feature is
//! off: it imports `zorp_mcp`, `zorp_track`, `zorp_agent::mcp_adapter`,
//! and `zorp_agent::validate`, all of which are behind the optional
//! `mcp`/`research` features on `zorp-agent`, and no other workspace
//! member depends on `zorp-agent`, so nothing else rescues
//! `cargo test --workspace` (default features) from these otherwise
//! failing to resolve.
//!
//! Note on imports: `zorp_agent::model` is a private module (`mod
//! model;` in `src/lib.rs`); its types are re-exported directly at the
//! crate root (`pub use model::{..., Model, ...}`), so this test
//! imports `zorp_agent::{AssistantMessage, Message, Model}` rather than
//! `zorp_agent::model::{...}`.
#![cfg(feature = "research")]

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tempfile::tempdir;
use zorp_agent::{cancel_token, Agent, ApprovalMode, AssistantMessage, BoxErr, Message, Model, ToolCall};
use zorp_mcp::{McpConfig, McpRegistry};
use zorp_track::checkpoint::CheckpointMode;
use zorp_track::Project;

/// Stateful stub model: on its *first* `complete()` call it requests the
/// `search` MCP tool (so the agent actually invokes
/// `McpToolAdapter::run` -> `registry.call_tool` -> the stub server's
/// `tools/call` handler, not just the discovery handshake); on every
/// call after that it returns the final scored JSON-block answer, as if
/// it had just read the tool's result.
struct StubModel {
    response: String,
    search_tool_name: String,
    calls: Arc<AtomicUsize>,
}

impl Model for StubModel {
    fn complete(&self, _messages: &[Message], _tools: &[serde_json::Value]) -> Result<AssistantMessage, BoxErr> {
        let call_index = self.calls.fetch_add(1, Ordering::SeqCst);
        if call_index == 0 {
            Ok(AssistantMessage {
                content: String::new(),
                tool_calls: vec![ToolCall {
                    id: "1".to_string(),
                    name: self.search_tool_name.clone(),
                    arguments: serde_json::json!({ "query": "does caching help" }),
                }],
                finish_reason: "tool_calls".to_string(),
                reasoning_content: None,
            })
        } else {
            Ok(AssistantMessage {
                content: self.response.clone(),
                tool_calls: vec![],
                finish_reason: "stop".to_string(),
                reasoning_content: None,
            })
        }
    }

    fn clone_box(&self) -> Box<dyn Model> {
        Box::new(StubModel {
            response: self.response.clone(),
            search_tool_name: self.search_tool_name.clone(),
            calls: self.calls.clone(),
        })
    }
}

fn well_formed_response() -> String {
    "Based on the search: \n```json\n{\"redundancy_score\": 15.0, \"redundancy_citations\": [{\"text\": \"no prior work found on this exact question\", \"source\": \"stub search\"}], \"feasibility_score\": 88.0, \"feasibility_citations\": [{\"text\": \"a relevant benchmarking tool already exists\", \"source\": \"stub search\"}], \"verdict\": \"worth investigating\"}\n```".to_string()
}

fn stub_server_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_stub_search_mcp_server"))
}

#[test]
fn validate_end_to_end_with_a_stub_search_server_and_stub_model() {
    let dir = tempdir().unwrap();
    std::process::Command::new("git").arg("-C").arg(dir.path()).args(["init", "-q"]).output().unwrap();
    std::process::Command::new("git").arg("-C").arg(dir.path()).args(["config", "user.email", "t@example.com"]).output().unwrap();
    std::process::Command::new("git").arg("-C").arg(dir.path()).args(["config", "user.name", "T"]).output().unwrap();

    // Everything from here through the tool-call assertions below needs
    // no embeddings provider: spawning the stub server, the
    // initialize/tools-list handshake, and the MCP protocol framing
    // work all run and assert unconditionally, regardless of whether
    // ZORP_EMBEDDING_MODEL is configured in this environment.
    let config = McpConfig::from_toml_str(&format!(
        "[[server]]\nname = \"stub\"\ntransport = \"stdio\"\ncommand = \"{}\"\ntrust = \"sandbox\"\n",
        stub_server_binary().display()
    ))
    .unwrap();
    let mut registry = McpRegistry::new(config);
    let tools = registry.discover();
    assert!(!tools.is_empty(), "stub server should advertise at least one tool");
    let search_tool_name = tools
        .iter()
        .find(|t| t.name == "search")
        .expect("stub server should advertise a `search` tool")
        .prefixed_name
        .clone();
    assert_eq!(search_tool_name, "mcp__stub__search");

    let calls = Arc::new(AtomicUsize::new(0));
    let model = StubModel { response: well_formed_response(), search_tool_name: search_tool_name.clone(), calls: calls.clone() };
    let mut agent = Agent::new(Box::new(model), "system", 5, dir.path().to_path_buf(), cancel_token(), ApprovalMode::AutoApprove)
        .register_builtins();
    // Attach the stub MCP tool the same way attach_mcp_tools does, adapted
    // inline here since attach_mcp_tools itself lives in the binary crate,
    // not the library, and isn't reachable from an integration test.
    use std::sync::Mutex;
    let registry = Arc::new(Mutex::new(registry));
    for tool in tools {
        agent = agent.register(Box::new(zorp_agent::mcp_adapter::McpToolAdapter { tool, registry: registry.clone() }));
    }

    let project = Project::open(dir.path()).unwrap();
    let track_id = "2026-08-09-validate-integration-test";
    project.store.create_track(track_id, "does caching help").unwrap();
    let mode = CheckpointMode::terminal(true).unwrap();

    // embed_texts (Task 3) requires a real ZORP_EMBEDDING_MODEL and a
    // reachable embeddings endpoint; validate::run calls it for every
    // citation, after the search/scoring turn. Everything above this
    // point (stub server spawn, MCP handshake, tool discovery) has
    // already run and been asserted unconditionally; only the
    // embedding-dependent tail is skipped when no embeddings provider
    // is configured, matching zorp-mcp's own convention of guarding on
    // external prerequisites (e.g. `has_npx()` in
    // zorp-mcp/tests/integration.rs).
    if std::env::var("ZORP_EMBEDDING_MODEL").is_err() {
        eprintln!("skipping past this point: ZORP_EMBEDDING_MODEL is not set (no embeddings provider configured)");
        return;
    }

    let approved = zorp_agent::validate::run(&mut agent, &project, track_id, "does caching help", &mode).unwrap();
    assert!(approved);

    // The model must have been called at least twice: once to request
    // the search tool, once to read the tool's result and produce the
    // final scored answer. This confirms McpToolAdapter::run and
    // registry.call_tool actually executed against the stub server's
    // `tools/call` handler, not just the discovery handshake.
    assert!(calls.load(Ordering::SeqCst) >= 2, "model should have been called again after the tool result was fed back");

    let validation = project.store.get_validation(track_id).unwrap();
    assert_eq!(validation.redundancy_score, 15.0);
    assert_eq!(validation.feasibility_score, 88.0);
    assert_eq!(validation.redundancy_citations.len(), 1);

    let track = project.store.get_track(track_id).unwrap();
    assert_eq!(track.status, zorp_track::track::TrackStatus::Active);
}
