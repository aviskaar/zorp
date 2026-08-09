//! End-to-end integration test for `validate::run`, using a stub MCP
//! search server (a separate binary fixture, see
//! `tests/fixtures/stub_search_mcp_server.rs`) and a stub `Model`, so
//! the whole validate pipeline can be proven without a real LLM or a
//! real network search.
//!
//! Note on imports: `zorp_agent::model` is a private module (`mod
//! model;` in `src/lib.rs`); its types are re-exported directly at the
//! crate root (`pub use model::{..., Model, ...}`), so this test
//! imports `zorp_agent::{AssistantMessage, Message, Model}` rather than
//! `zorp_agent::model::{...}`.
use std::path::PathBuf;
use tempfile::tempdir;
use zorp_agent::{cancel_token, Agent, ApprovalMode, AssistantMessage, BoxErr, Message, Model};
use zorp_mcp::{McpConfig, McpRegistry};
use zorp_track::checkpoint::CheckpointMode;
use zorp_track::Project;

struct StubModel {
    response: String,
}

impl Model for StubModel {
    fn complete(&self, _messages: &[Message], _tools: &[serde_json::Value]) -> Result<AssistantMessage, BoxErr> {
        Ok(AssistantMessage {
            content: self.response.clone(),
            tool_calls: vec![],
            finish_reason: "stop".to_string(),
            reasoning_content: None,
        })
    }

    fn clone_box(&self) -> Box<dyn Model> {
        Box::new(StubModel { response: self.response.clone() })
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
    // embed_texts (Task 3) requires a real ZORP_EMBEDDING_MODEL and a
    // reachable embeddings endpoint; validate::run calls it for every
    // citation. Skip cleanly rather than fail when no embeddings
    // provider is configured in this environment, matching zorp-mcp's
    // own convention of guarding on external prerequisites (e.g.
    // `has_npx()` in zorp-mcp/tests/integration.rs).
    if std::env::var("ZORP_EMBEDDING_MODEL").is_err() {
        eprintln!("skipping: ZORP_EMBEDDING_MODEL is not set (no embeddings provider configured)");
        return;
    }

    let dir = tempdir().unwrap();
    std::process::Command::new("git").arg("-C").arg(dir.path()).args(["init", "-q"]).output().unwrap();
    std::process::Command::new("git").arg("-C").arg(dir.path()).args(["config", "user.email", "t@example.com"]).output().unwrap();
    std::process::Command::new("git").arg("-C").arg(dir.path()).args(["config", "user.name", "T"]).output().unwrap();

    let config = McpConfig::from_toml_str(&format!(
        "[[server]]\nname = \"stub\"\ntransport = \"stdio\"\ncommand = \"{}\"\ntrust = \"sandbox\"\n",
        stub_server_binary().display()
    ))
    .unwrap();
    let mut registry = McpRegistry::new(config);
    let tools = registry.discover();
    assert!(!tools.is_empty(), "stub server should advertise at least one tool");

    let model = StubModel { response: well_formed_response() };
    let mut agent = Agent::new(Box::new(model), "system", 5, dir.path().to_path_buf(), cancel_token(), ApprovalMode::AutoApprove)
        .register_builtins();
    // Attach the stub MCP tool the same way attach_mcp_tools does, adapted
    // inline here since attach_mcp_tools itself lives in the binary crate,
    // not the library, and isn't reachable from an integration test.
    use std::sync::{Arc, Mutex};
    let registry = Arc::new(Mutex::new(registry));
    for tool in tools {
        agent = agent.register(Box::new(zorp_agent::mcp_adapter::McpToolAdapter { tool, registry: registry.clone() }));
    }

    let project = Project::open(dir.path()).unwrap();
    let track_id = "2026-08-09-validate-integration-test";
    project.store.create_track(track_id, "does caching help").unwrap();
    let mode = CheckpointMode::terminal(true).unwrap();

    let approved = zorp_agent::validate::run(&mut agent, &project, track_id, "does caching help", &mode).unwrap();
    assert!(approved);

    let validation = project.store.get_validation(track_id).unwrap();
    assert_eq!(validation.redundancy_score, 15.0);
    assert_eq!(validation.feasibility_score, 88.0);
    assert_eq!(validation.redundancy_citations.len(), 1);

    let track = project.store.get_track(track_id).unwrap();
    assert_eq!(track.status, zorp_track::track::TrackStatus::Active);
}
