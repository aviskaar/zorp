//! End-to-end integration test for `deliver::run`, using the same stub
//! MCP server binary `validate_integration.rs` already uses
//! (`tests/fixtures/stub_search_mcp_server.rs`), configured under the
//! server name `huiban` so its tools come out prefixed
//! `mcp__huiban__*` — the exact gate `deliver::run` checks for. No real
//! huiban-specific code is needed anywhere: `mcp__<name>__<tool>`
//! prefixing is generic in `zorp-mcp`, driven entirely by the config's
//! `name` field.
#![cfg(feature = "research")]

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use zorp_agent::deliver::{run, DeliverError};
use zorp_agent::{
    cancel_token, Agent, ApprovalMode, AssistantMessage, BoxErr, Message, Model, ToolCall,
};
use zorp_mcp::{McpConfig, McpRegistry};
use zorp_track::checkpoint::CheckpointMode;
use zorp_track::track::TrackStatus;
use zorp_track::Project;

struct StubModel {
    response: String,
    search_tool_name: String,
    calls: Arc<AtomicUsize>,
}

impl Model for StubModel {
    fn complete(
        &self,
        _messages: &[Message],
        _tools: &[serde_json::Value],
    ) -> Result<AssistantMessage, BoxErr> {
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

struct RejectAll;
impl zorp_track::checkpoint::Decider for RejectAll {
    fn decide(&self, _prompt: &str) -> bool {
        false
    }
}

fn stub_server_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_stub_search_mcp_server"))
}

fn build_agent_with_huiban_stub(response: &str) -> (Agent, tempfile::TempDir, Arc<AtomicUsize>) {
    let dir = tempdir().unwrap();
    let config = McpConfig::from_toml_str(&format!(
        "[[server]]\nname = \"huiban\"\ntransport = \"stdio\"\ncommand = \"{}\"\ntrust = \"sandbox\"\n",
        stub_server_binary().display()
    ))
    .unwrap();
    let mut registry = McpRegistry::new(config);
    let tools = registry.discover();
    let search_tool_name = tools
        .iter()
        .find(|t| t.name == "search")
        .expect("stub server should advertise a `search` tool")
        .prefixed_name
        .clone();
    assert_eq!(search_tool_name, "mcp__huiban__search");

    let calls = Arc::new(AtomicUsize::new(0));
    let model = StubModel {
        response: response.to_string(),
        search_tool_name: search_tool_name.clone(),
        calls: calls.clone(),
    };
    let mut agent = Agent::new(
        Box::new(model),
        "system",
        5,
        dir.path().to_path_buf(),
        cancel_token(),
        ApprovalMode::AutoApprove,
    )
    .register_builtins();
    let registry = Arc::new(Mutex::new(registry));
    for tool in tools {
        agent = agent.register(Box::new(zorp_agent::mcp_adapter::McpToolAdapter {
            tool,
            registry: registry.clone(),
        }));
    }
    (agent, dir, calls)
}

fn track_with_draft(project: &Project, track_id: &str) {
    project
        .store
        .create_track(track_id, "does caching help")
        .unwrap();
    let track_dir = project.track_dir(track_id);
    std::fs::create_dir_all(&track_dir).unwrap();
    std::fs::write(track_dir.join("draft.md"), "# Draft\n\nLatency improved.").unwrap();
}

#[test]
fn full_round_trip_finds_venues_and_approves() {
    let (mut agent, agent_dir, calls) = build_agent_with_huiban_stub(
        "## Candidate Venues\n\n1. Example Systems Conference (deadline 2026-12-01, CORE A)",
    );
    let dir = tempdir().unwrap();
    let project = Project::open(dir.path()).unwrap();
    track_with_draft(&project, "t1");
    let mode = CheckpointMode::terminal(true).unwrap();

    let approved = run(&mut agent, &project, "t1", "does caching help", &mode).unwrap();
    assert!(approved);

    let venues_path = project.track_dir("t1").join("venues.md");
    let content = std::fs::read_to_string(&venues_path).unwrap();
    assert!(content.contains("Example Systems Conference"));

    // The model must have been called at least twice: once to request
    // the search tool, once to read the tool's result and produce the
    // final venue list. This confirms McpToolAdapter::run and
    // registry.call_tool actually executed against the stub server's
    // `tools/call` handler, not just the discovery handshake.
    assert!(
        calls.load(Ordering::SeqCst) >= 2,
        "model should have been called again after the tool result was fed back"
    );
    drop(agent_dir);
}

#[test]
fn rejected_checkpoint_leaves_track_status_unchanged() {
    let (mut agent, agent_dir, calls) =
        build_agent_with_huiban_stub("## Candidate Venues\n\n1. Example Systems Conference");
    let dir = tempdir().unwrap();
    let project = Project::open(dir.path()).unwrap();
    track_with_draft(&project, "t1");
    let mode = CheckpointMode::Interactive(Arc::new(RejectAll));

    let approved = run(&mut agent, &project, "t1", "does caching help", &mode).unwrap();
    assert!(!approved);

    let track = project.store.get_track("t1").unwrap();
    assert_eq!(track.status, TrackStatus::Active);

    // Even though the checkpoint was rejected, the agent still ran the
    // full search round trip before reaching the checkpoint: once to
    // request the search tool, once to read the tool's result. This
    // confirms McpToolAdapter::run and registry.call_tool actually
    // executed against the stub server's `tools/call` handler, not just
    // the discovery handshake.
    assert!(
        calls.load(Ordering::SeqCst) >= 2,
        "model should have been called again after the tool result was fed back"
    );
    drop(agent_dir);
}

#[test]
fn no_draft_refuses_before_running_the_agent() {
    let (mut agent, agent_dir, calls) =
        build_agent_with_huiban_stub("## Candidate Venues\n\n1. Example Systems Conference");
    let dir = tempdir().unwrap();
    let project = Project::open(dir.path()).unwrap();
    project
        .store
        .create_track("t1", "does caching help")
        .unwrap();
    let mode = CheckpointMode::terminal(true).unwrap();

    let err = run(&mut agent, &project, "t1", "does caching help", &mode).unwrap_err();
    assert!(matches!(err, DeliverError::NoDraft));

    // The missing-draft check must short-circuit before the agent (and
    // therefore the model) ever runs.
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "model should never have been called when there's no draft"
    );
    drop(agent_dir);
}

#[test]
fn no_huiban_tool_refuses_even_with_a_draft_present() {
    let dir = tempdir().unwrap();
    let project = Project::open(dir.path()).unwrap();
    track_with_draft(&project, "t1");
    let mode = CheckpointMode::terminal(true).unwrap();

    // Build an agent with no MCP tools attached at all, unlike the other
    // tests in this file.
    let calls = Arc::new(AtomicUsize::new(0));
    let model = StubModel {
        response: "irrelevant".to_string(),
        search_tool_name: "irrelevant".to_string(),
        calls: calls.clone(),
    };
    let mut agent = Agent::new(
        Box::new(model),
        "system",
        5,
        std::env::temp_dir(),
        cancel_token(),
        ApprovalMode::AutoApprove,
    )
    .register_builtins();

    let err = run(&mut agent, &project, "t1", "does caching help", &mode).unwrap_err();
    assert!(matches!(err, DeliverError::NoVenueTool));

    // The huiban gate must short-circuit before the agent (and therefore
    // the model) ever runs.
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "model should never have been called when there's no huiban tool"
    );
}
