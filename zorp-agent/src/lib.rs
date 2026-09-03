//! zorp-agent — a research agent built on the tiny zorp core.
//! Milestone 1 (walking skeleton): normalized model turns + a bare agent loop.

mod agent;
mod approval;
mod blocks;
mod capsule;
mod chat;
#[cfg(feature = "research")]
pub mod co_write;
mod context;
pub mod context_window;
#[cfg(feature = "research")]
pub mod critique;
#[cfg(feature = "research")]
pub mod deliver;
mod embed;
mod flavor;
mod identity;
mod instructions;
#[cfg(feature = "research")]
pub mod investigate;
mod model;
pub mod panel;
mod policy;
mod provider;
mod reasoning;
mod recorder;
mod render;
mod sandbox;
#[cfg(feature = "search")]
mod search_tool;
mod session;
mod skill_tool;
pub mod streaming;
mod tools;
mod trust;
#[cfg(feature = "research")]
pub mod validate;
mod verify;

pub use agent::{web_search_availability, Agent, Outcome, RunRecorder, ToolAvailability};
pub use approval::{ApprovalMode, Approver, TerminalApprover};
pub use capsule::{
    default_user_capsules_dir, extract_fenced_block, is_reserved, project_capsules_dir, Capsule,
    CapsuleRegistry, CapsuleState, RESERVED_NAMES,
};
pub use chat::{parse_command, ChatCommand, ReasoningCommand};
pub use context::seed as seed_context;
pub use context_window::{
    compact_tool_results, estimate_tokens, parse_token_usage, plan_seed, repair_tool_calls,
    CompactionReport, ContextBudget, ContextUsage, SeedPlan, TokenUsage, UsageSource,
};
pub use embed::{embed_request_body, embed_texts, parse_embedding_response};
pub use flavor::{
    content_hash, is_valid_flavor_name, layer_paths, named_flavor_exists, project_raw, resolve,
    resolve_configured, resolve_scoped, resolve_scoped_configured, ApprovalSection,
    ConfiguredFlavor, Flavor, Scope, ToolsSection, VerifySection,
};
pub use identity::DEFAULT_SYSTEM_PROMPT;
pub use instructions::load as load_instructions;
pub use model::{
    messages_to_body, parse_assistant, parse_assistant_completion, AssistantMessage,
    ConfiguredHttpModel, ContentPart, HttpModel, Message, MessageMetadata, MessageRecord, Model,
    ModelCompletion, ToolCall,
};
pub use panel::{
    default_lenses, reviewer_tools, Agreement, Lens, PanelConfig, PanelFinding, PanelObserver,
    PanelReport, ReviewerFailure, ReviewerVerdict, Severity, SilentObserver, Target,
};
pub use policy::{Decision, Policy, Preset};
pub use provider::Provider;
pub use reasoning::{
    parse_env_reasoning_mode, reasoning_payload, CompletionOptions, CompletionTelemetry,
    ReasoningMode,
};
pub use recorder::SqliteRecorder;
pub use render::{
    chat_spinner_renderer, parse_spinner_verbs, render_assistant_text, stderr_renderer,
    LineRenderer, Renderer,
};
pub use sandbox::{cancel_token, CancelToken, CommandOutput, Sandbox};
pub use session::{new_session_id, render_change_summary, SessionRow, Store};
pub use tools::fs::{ListFiles, ReadFile, WriteFile};
pub use tools::git::{GitDiff, GitStatus};
pub use tools::patch::ApplyPatch;
pub use tools::search::SearchText;
pub use tools::shell::RunCommand;
pub use tools::{
    builtin_tools, builtin_tools_filtered, cap_output, Context, FileChange, Registry, Tool,
    ToolError, ToolOutput, ToolResult,
};
pub use trust::TrustStore;
#[cfg(feature = "research")]
pub use validate::{parse_validation_result, ParseError, ValidateError, ValidationResult};
pub use verify::{Verifier, VerifyReport, VerifyResult};
pub use zorp::join_url;

#[cfg(feature = "mcp")]
pub mod mcp_adapter;
#[cfg(feature = "mcp")]
pub use mcp_adapter::McpToolAdapter;

/// Shared boxed error, mirroring the core so `?` composes across both crates.
pub type BoxErr = Box<dyn std::error::Error + Send + Sync>;
