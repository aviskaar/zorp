use clap::{error::ErrorKind, CommandFactory, Parser, Subcommand};
use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use zorp_agent::{
    cancel_token, chat_spinner_renderer, content_hash, default_user_capsules_dir,
    extract_fenced_block, is_reserved, join_url, load_instructions, named_flavor_exists,
    new_session_id, parse_command, parse_spinner_verbs, project_capsules_dir, project_raw,
    render_assistant_text, render_change_summary, resolve_scoped_configured, seed_context, Agent,
    ApprovalMode, Capsule, CapsuleRegistry, CapsuleState, ChatCommand, ConfiguredFlavor, Flavor,
    HttpModel, LineRenderer, Message, Outcome, Policy, Preset, Provider, ReasoningCommand,
    ReasoningMode, Renderer, SqliteRecorder, Store, TrustStore, Verifier,
};

#[cfg(feature = "otel")]
mod otel_init {
    pub struct OtelGuard {
        _rt: tokio::runtime::Runtime,
    }

    impl Drop for OtelGuard {
        fn drop(&mut self) {
            opentelemetry::global::shutdown_tracer_provider();
        }
    }

    pub fn init_otel() -> Option<OtelGuard> {
        // gRPC/HTTP OTLP exporter batch processor runs asynchronously.
        // Create a dedicated single-threaded runtime to orchestrate exports.
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .ok()?;

        let _guard = rt.enter();

        let _ = opentelemetry::global::set_error_handler(|_| {});

        let tracer = opentelemetry_otlp::new_pipeline()
            .tracing()
            .with_exporter(opentelemetry_otlp::new_exporter().http())
            .install_batch(opentelemetry_sdk::runtime::Tokio)
            .ok()?;

        use tracing_subscriber::prelude::*;
        let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);
        let subscriber = tracing_subscriber::registry().with(telemetry);

        tracing::subscriber::set_global_default(subscriber).ok()?;

        Some(OtelGuard { _rt: rt })
    }
}

use zorp_agent::DEFAULT_SYSTEM_PROMPT as DEFAULT_SYSTEM;

#[derive(Parser)]
#[command(version)]
#[command(subcommand_precedence_over_arg = true)]
struct Cli {
    /// Approve trusted prompts without asking. This answers the asks an
    /// approval preset produces, so --approval read-only --yes still edits.
    /// To block an operation outright, set it to "deny" in a flavor's
    /// [approval] section; the hard denylist always wins regardless.
    #[arg(long, global = true)]
    yes: bool,
    /// Skip configured verification commands.
    #[arg(long, global = true)]
    no_verify: bool,
    /// Select a named flavor profile.
    #[arg(long, global = true)]
    flavor: Option<String>,
    /// Override the model name.
    #[arg(long, global = true)]
    model: Option<String>,
    /// Override the OpenAI-compatible base URL.
    #[arg(long, global = true)]
    base_url: Option<String>,
    /// Select the provider wire format: "openai" (default) or "anthropic".
    #[arg(long, global = true)]
    provider: Option<String>,
    /// Override the max_tokens sent to Anthropic requests (ignored for openai).
    #[arg(long, global = true)]
    max_tokens: Option<u32>,
    /// Limit the number of agent steps. A failing verification gate needs a
    /// few steps of headroom to report itself: it stops after 3 no-progress
    /// attempts, and a tighter limit ends the run as a step limit instead.
    #[arg(long, global = true)]
    max_steps: Option<usize>,
    /// Select the approval preset: read-only, editor, or full. Presets set
    /// what is asked about, not what is refused. See --yes.
    #[arg(long, global = true)]
    approval: Option<String>,
    /// Connect to an MCP server. Format: stdio:name:command[:arg1:arg2...]
    /// or streamable_http:name:url  or  sse:name:url (legacy).
    /// Can be specified multiple times. Requires --features mcp build.
    #[cfg(feature = "mcp")]
    #[arg(long = "mcp", global = true, value_name = "TRANSPORT:NAME:...")]
    mcp: Vec<String>,
    /// Attach image file(s) to the prompt. Can be specified multiple times.
    #[arg(long = "image", global = true, value_name = "PATH")]
    images: Vec<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    task: Vec<String>,
}

struct Overrides {
    flavor: Option<String>,
    model: Option<String>,
    base_url: Option<String>,
    provider: Option<String>,
    max_tokens: Option<u32>,
    max_steps: Option<usize>,
    approval: Option<String>,
    #[cfg(feature = "mcp")]
    mcp: Vec<String>,
}

#[derive(Subcommand)]
enum Command {
    /// Start an interactive chat session.
    Chat,
    /// Continue a previous session by id.
    Resume { id: String },
    /// Revert the most recent recorded file change.
    Undo,
    /// Print a summary of the latest session's file changes.
    Diff,
    /// Scaffold a new flavor manifest at ./.zorp/flavors/<name>.toml.
    New { name: String },
    /// Validate whether a question is worth investigating.
    #[cfg(feature = "research")]
    Validate { question: String },
    /// Run one staged, pre-registered investigate attempt against a track.
    #[cfg(feature = "research")]
    Investigate {
        question: String,
        #[arg(long = "metric-name")]
        metric_name: Option<String>,
        #[arg(long = "kill-threshold")]
        kill_threshold: Option<f64>,
        /// Which side of the kill threshold kills the track:
        /// lower-is-better kills when the metric goes above it,
        /// higher-is-better when it goes below. Required with
        /// --kill-threshold.
        #[arg(long = "threshold-direction")]
        threshold_direction: Option<String>,
    },
    /// Draft an artifact from a track's recorded evidence.
    #[cfg(feature = "research")]
    CoWrite { question: String },
    /// Match a co-written draft against real venues.
    #[cfg(feature = "research")]
    Deliver { question: String },
}

fn main() {
    let cli = Cli::parse();

    #[cfg(feature = "otel")]
    let _otel_guard = match &cli.command {
        Some(Command::Chat) | Some(Command::Resume { .. }) | None => otel_init::init_otel(),
        _ => None,
    };
    let overrides = Overrides {
        flavor: cli.flavor.clone(),
        model: cli.model.clone(),
        base_url: cli.base_url.clone(),
        provider: cli.provider.clone(),
        max_tokens: cli.max_tokens,
        max_steps: cli.max_steps,
        approval: cli.approval.clone(),
        #[cfg(feature = "mcp")]
        mcp: cli.mcp,
    };
    match cli.command {
        Some(Command::Chat) => chat(cli.yes, cli.no_verify, &overrides),
        Some(Command::Resume { id }) => resume(&id, cli.yes, cli.no_verify, &overrides),
        Some(Command::Undo) => undo(),
        Some(Command::Diff) => diff(),
        Some(Command::New { name }) => scaffold(&name),
        #[cfg(feature = "research")]
        Some(Command::Validate { question }) => validate(&question, cli.yes, &overrides),
        #[cfg(feature = "research")]
        Some(Command::Investigate {
            question,
            metric_name,
            kill_threshold,
            threshold_direction,
        }) => investigate(
            &question,
            metric_name,
            kill_threshold,
            threshold_direction,
            cli.yes,
            &overrides,
        ),
        #[cfg(feature = "research")]
        Some(Command::CoWrite { question }) => co_write(&question, cli.yes, &overrides),
        #[cfg(feature = "research")]
        Some(Command::Deliver { question }) => deliver(&question, cli.yes, &overrides),
        None => {
            if cli.task.is_empty() {
                eprintln!("usage: zorp-agent [--yes] [--no-verify] \"<task>\"");
                std::process::exit(2);
            }
            if let Some(flag) = cli.task.first().filter(|arg| arg.starts_with("--")) {
                Cli::command()
                    .error(
                        ErrorKind::UnknownArgument,
                        format!("unexpected argument '{flag}' found"),
                    )
                    .exit();
            }
            run(
                cli.task.join(" "),
                &cli.images,
                cli.yes,
                cli.no_verify,
                &overrides,
            );
        }
    }
}

const SCAFFOLD_TEMPLATE: &str = r#"name = "{name}"

# All keys are optional; omitted keys inherit from the layer below.
# api_key is NEVER read from a manifest — set ZORP_API_KEY in the environment.
# model         = "qwen3.6:35b"
# base_url      = "http://localhost:11434/v1"
# provider      = "openai"  # openai | anthropic
# max_tokens    = 4096      # required by Anthropic; ignored for openai
# reasoning_mode  = "low"
# max_steps     = 30
# auto_verify   = true
# system_prompt = "You are a terse senior reviewer."

[tools]
# Allow-list over all built-in tools. Omit to enable all.
# enabled = ["read_file", "search_text", "list_files", "git_diff"]

[approval]
# preset = "read-only"   # read-only | editor | full
# run_command = "ask"    # allow | ask | deny

[verify]
# Commands run as a completion gate (project flavors require trust-on-first-use).
# test = "cargo test"
# lint = "cargo clippy -- -D warnings"
"#;

fn mime_from_extension(path: &Path) -> String {
    match path.extension().and_then(|e| e.to_str()) {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        _ => "image/png",
    }
    .to_string()
}

fn is_image_extension(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("png" | "jpg" | "jpeg" | "gif" | "webp")
    )
}

/// Parse `@image <path>` or `@img <path>` references from text.
/// Returns (cleaned_text, vec of (image_data, mime_type)).
fn extract_image_refs(text: &str, cwd: &Path) -> (String, Vec<(Vec<u8>, String)>) {
    static IMAGE_REF: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = IMAGE_REF.get_or_init(|| regex::Regex::new(r"@(?:image|img)\s+(\S+)").unwrap());
    let mut images = Vec::new();
    let cleaned = re.replace_all(text, |caps: &regex::Captures| {
        let raw_path = &caps[1];
        let path = cwd.join(raw_path);
        match std::fs::read(&path) {
            Ok(data) => {
                let mime = mime_from_extension(&path);
                images.push((data, mime));
                format!("[Image {}]", images.len())
            }
            Err(e) => {
                eprintln!("zorp-agent: cannot read {}: {e}", path.display());
                caps[0].to_string()
            }
        }
    });
    (cleaned.to_string(), images)
}

enum Segment {
    Text(String),
    Paste(String),
    Image {
        data: Vec<u8>,
        mime_type: String,
        index: usize,
    },
}

fn segments_to_parts(segments: &[Segment], cwd: &Path) -> Vec<zorp_agent::ContentPart> {
    use zorp_agent::ContentPart;
    let mut parts: Vec<ContentPart> = Vec::new();
    let mut text_buf = String::new();
    for seg in segments {
        match seg {
            Segment::Text(t) => text_buf.push_str(t),
            Segment::Paste(s) => text_buf.push_str(s),
            Segment::Image {
                data, mime_type, ..
            } => {
                if !text_buf.is_empty() {
                    parts.push(ContentPart::Text(std::mem::take(&mut text_buf)));
                }
                parts.push(ContentPart::Image {
                    data: data.clone(),
                    mime_type: mime_type.clone(),
                });
            }
        }
    }
    // Process @image refs in remaining text
    if !text_buf.is_empty() {
        let (cleaned, img_refs) = extract_image_refs(&text_buf, cwd);
        if !cleaned.trim().is_empty() {
            parts.push(ContentPart::Text(cleaned));
        }
        for (data, mime) in img_refs {
            parts.push(ContentPart::Image {
                data,
                mime_type: mime,
            });
        }
    }
    parts
}

fn scaffold(name: &str) {
    if !zorp_agent::is_valid_flavor_name(name) {
        eprintln!(
            "zorp-agent: {name} is not a valid flavor name \
             (must be a single path component, no '/' or '..')"
        );
        std::process::exit(1);
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let dir = cwd.join(".zorp").join("flavors");
    let path = dir.join(format!("{name}.toml"));
    if path.exists() {
        eprintln!("zorp-agent: {} already exists", path.display());
        std::process::exit(1);
    }
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("zorp-agent: {e}");
        std::process::exit(1);
    }
    let body = SCAFFOLD_TEMPLATE.replace("{name}", name);
    if let Err(e) = std::fs::write(&path, body) {
        eprintln!("zorp-agent: {e}");
        std::process::exit(1);
    }
    println!("created {}", path.display());
}

fn open_store() -> Option<Store> {
    match Store::open_default() {
        Ok(s) => Some(s),
        Err(e) => {
            eprintln!("zorp-agent: session store unavailable: {e}");
            None
        }
    }
}

fn install_cancel() -> zorp_agent::CancelToken {
    let cancel = cancel_token();
    let signal = cancel.clone();
    if let Err(e) = ctrlc::set_handler(move || signal.store(true, Ordering::SeqCst)) {
        eprintln!("zorp-agent: failed to install Ctrl-C handler: {e}");
        std::process::exit(1);
    }
    cancel
}

fn compose_system(cwd: &Path) -> String {
    let mut system = std::env::var("ZORP_SYSTEM").unwrap_or_else(|_| DEFAULT_SYSTEM.to_string());
    if let Some(rules) = load_instructions(cwd, cwd) {
        system.push_str("\n\n# Repository rules\n");
        system.push_str(&rules);
    }
    system.push_str("\n\n");
    system.push_str(&seed_context(cwd));
    system
}

fn compose_system_with_persona(cwd: &Path, persona: Option<&str>) -> String {
    let mut system = String::new();
    if let Some(p) = persona {
        if !p.trim().is_empty() {
            system.push_str("# Persona\n");
            system.push_str(p.trim());
            system.push_str("\n\n");
        }
    }
    system.push_str(&compose_system(cwd));
    system
}

fn resolve_flavor(overrides: &Overrides) -> (ConfiguredFlavor, ConfiguredFlavor) {
    let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_default();
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    // A name that matches no file is a mistake, not a no-op. Flavors usually
    // restrict what the agent may do, so running without the one that was
    // asked for hands back more freedom than the user wanted, silently.
    if let Some(name) = overrides.flavor.as_deref() {
        if !named_flavor_exists(&home, &cwd, name) {
            eprintln!(
                "zorp-agent: no flavor named '{name}'; looked in \
                 {}/.config/zorp/flavors/{name}.toml and ./.zorp/flavors/{name}.toml",
                home.display()
            );
            std::process::exit(1);
        }
    }
    match resolve_scoped_configured(&home, &cwd, overrides.flavor.as_deref()) {
        Ok(pair) => pair,
        Err(e) => {
            eprintln!("zorp-agent: flavor error: {e}");
            std::process::exit(1);
        }
    }
}

/// Return the flavor whose command-bearing/loosening fields may be applied:
/// `user ⊕ project` when the project flavor is trusted (or needs no privilege),
/// otherwise `user` alone. Prompts on a TTY; non-interactive denies; `--yes`
/// trusts and records.
fn gated_flavor(
    user: &ConfiguredFlavor,
    project: &ConfiguredFlavor,
    flavor_name: Option<&str>,
    auto_approve: bool,
) -> ConfiguredFlavor {
    let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_default();
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let Some(raw) = project_raw(&home, &cwd, flavor_name) else {
        return user.clone();
    };
    if !project.wants_privilege() {
        // Only safe project fields exist. There is nothing to gate, so they
        // apply. Returning `user` alone here would silently discard a
        // project flavor that tightens approvals, which is a restriction the
        // user asked for and gets no warning about.
        return user.clone().merge(project.clone());
    }
    let hash = content_hash(&raw);
    let mut store = TrustStore::open();
    if store.is_trusted(&hash) {
        return user.clone().merge(project.clone());
    }
    let trusted = auto_approve || prompt_trust(project);
    if trusted {
        if let Err(e) = store.trust(&hash) {
            eprintln!(
                "zorp-agent: could not persist trust decision ({e}); \
                 you will be asked again next run"
            );
        }
    } else {
        eprintln!(
            "zorp-agent: project flavor not trusted; its verify/approval settings are ignored"
        );
    }
    if trusted {
        user.clone().merge(project.clone())
    } else {
        user.clone()
    }
}

/// Ask the human to approve a project flavor. Denies unless stdin is a TTY and
/// the answer is y/yes.
fn prompt_trust(project: &Flavor) -> bool {
    if !std::io::stdin().is_terminal() {
        return false;
    }
    eprintln!("⚠  ./.zorp/flavor.toml is new/changed and wants to:");
    for line in project.privilege_summary() {
        eprintln!("     • {line}");
    }
    eprint!("   Allow this project flavor? [y/N] ");
    let _ = std::io::stderr().flush();
    let mut input = String::new();
    if std::io::stdin().read_line(&mut input).is_err() {
        return false;
    }
    matches!(input.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

fn pick(flag: Option<&str>, env: &str, flavor: Option<&str>, default: &str) -> String {
    flag.map(str::to_string)
        .or_else(|| std::env::var(env).ok().filter(|s| !s.is_empty()))
        .or_else(|| flavor.map(str::to_string))
        .unwrap_or_else(|| default.to_string())
}

fn build_policy(flag: Option<&str>, user: &Flavor, repo_root: &Path) -> Policy {
    let preset_name = flag
        .map(str::to_string)
        .or_else(|| user.approval.preset.clone());
    let mut policy = match preset_name.as_deref().and_then(Preset::parse) {
        Some(p) => Policy::from_preset(p),
        None => Policy::default(),
    };
    // The destructive-rm and redirect checks compare targets against the
    // root. Without it every absolute target denies, which is safe but
    // needlessly blunt.
    policy = policy.with_repo_root(repo_root);
    for (op, decision) in &user.approval.overrides {
        policy = policy.with_override(op, decision);
    }
    if policy.write_barrier_is_porous() {
        eprintln!(
            "zorp-agent: note: edits are denied but run_command is not, so the \
             agent can still write through the shell. Deny run_command too if \
             you meant to stop writes."
        );
    }
    policy
}

fn persona(cwd: &Path, flavor: &Flavor) -> Option<String> {
    flavor.system_prompt.clone().or_else(|| {
        flavor
            .system_prompt_file
            .as_deref()
            .and_then(|path| std::fs::read_to_string(cwd.join(path)).ok())
    })
}

fn attach_verifier(mut agent: Agent, no_verify: bool, user_flavor: &Flavor) -> Agent {
    if !no_verify {
        let commands = user_flavor.verify_commands();
        if !commands.is_empty() {
            agent = agent.with_verifier(Verifier::new(commands));
        } else if let Some(verifier) = Verifier::from_env() {
            agent = agent.with_verifier(verifier);
        }
    }
    agent
}

fn finish(outcome: Outcome, store_status: Option<(&Store, &str)>) {
    let status = match &outcome {
        Outcome::Complete(answer) => {
            let rendered = render_assistant_text(answer, std::io::stdout().is_terminal());
            println!("{rendered}");
            "done"
        }
        Outcome::StepLimit => {
            eprintln!("zorp-agent: {}", outcome.describe());
            "step_limit"
        }
        Outcome::VerificationFailed { .. } => {
            eprintln!("zorp-agent: {}", outcome.describe());
            "verification_failed"
        }
        Outcome::Error(e) => {
            eprintln!("zorp-agent: {e}");
            "error"
        }
        Outcome::Cancelled => {
            eprintln!("zorp-agent: {}", outcome.describe());
            "cancelled"
        }
        Outcome::RepeatedAction => {
            eprintln!("zorp-agent: {}", outcome.describe());
            "repeated_action"
        }
        Outcome::Blocked => {
            eprintln!(
                "zorp-agent: {}. Re-run with --yes to auto-approve edits and \
                 commands, or set an approval preset in a flavor.",
                outcome.describe()
            );
            "blocked"
        }
    };
    if let Some((store, id)) = store_status {
        let _ = store.set_status(id, status);
    }
    if !matches!(outcome, Outcome::Complete(_)) {
        std::process::exit(1);
    }
}

#[derive(serde::Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaModel>,
}

#[derive(serde::Deserialize)]
struct OllamaModel {
    name: String,
}

fn resolve_host_and_model(overrides: &Overrides, merged: &Flavor) -> (String, String) {
    let base_url = pick(
        overrides.base_url.as_deref(),
        "ZORP_BASE_URL",
        merged.base_url.as_deref(),
        "http://localhost:11434/v1",
    );
    let mut model_name = pick(
        overrides.model.as_deref(),
        "ZORP_MODEL",
        merged.model.as_deref(),
        "",
    );

    if model_name.is_empty() && base_url.contains("localhost:11434") {
        let tags_url = base_url.replace("/v1", "/api/tags");
        // Plain GET; the zorp core only exposes POST-a-JSON-body helpers
        // (zorp_raw), so this one call keeps a direct ureq dependency.
        if let Ok(res) = ureq::get(&tags_url).call() {
            if let Ok(json) = res.into_json::<OllamaTagsResponse>() {
                if !json.models.is_empty() {
                    eprintln!("No model specified. Available Ollama models:");
                    for (i, model) in json.models.iter().enumerate() {
                        eprintln!("  {}) {}", i + 1, model.name);
                    }
                    eprint!("Select a model (1-{}): ", json.models.len());
                    let _ = std::io::stdout().flush();
                    let mut input = String::new();
                    if std::io::stdin().read_line(&mut input).is_ok() {
                        let input = input.trim();
                        if let Ok(idx) = input.parse::<usize>() {
                            if idx > 0 && idx <= json.models.len() {
                                model_name = json.models[idx - 1].name.clone();
                                eprintln!("Selected model: {}", model_name);
                            }
                        }
                    }
                }
            }
        }
    }

    (base_url, model_name)
}

fn resolve_provider(
    overrides: &Overrides,
    merged: &Flavor,
) -> Result<Provider, zorp_agent::BoxErr> {
    if let Some(flag) = &overrides.provider {
        return flag.parse();
    }
    if let Ok(env) = std::env::var("ZORP_PROVIDER") {
        if !env.is_empty() {
            return env.parse();
        }
    }
    Ok(merged.provider.unwrap_or_default())
}

fn resolve_max_tokens(overrides: &Overrides, merged: &Flavor) -> Option<u32> {
    overrides
        .max_tokens
        .or_else(|| {
            std::env::var("ZORP_MAX_TOKENS")
                .ok()
                .and_then(|v| v.parse().ok())
        })
        .or(merged.max_tokens)
}

fn run(
    task: String,
    images: &[PathBuf],
    auto_approve: bool,
    no_verify: bool,
    overrides: &Overrides,
) {
    let cancel = install_cancel();
    let approval = ApprovalMode::terminal(auto_approve);
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let (user_flavor, project_flavor) = resolve_flavor(overrides);
    let gated = gated_flavor(
        &user_flavor,
        &project_flavor,
        overrides.flavor.as_deref(),
        auto_approve,
    );
    let merged = user_flavor.clone().merge(project_flavor);
    let system = compose_system_with_persona(&cwd, persona(&cwd, &merged).as_deref());
    let (base_url, model_name) = resolve_host_and_model(overrides, &merged);
    let provider = resolve_provider(overrides, &merged).unwrap_or_else(|e| {
        eprintln!("zorp-agent: {e}");
        std::process::exit(2);
    });
    let api_key = std::env::var("ZORP_API_KEY").ok().filter(|s| !s.is_empty());
    let model = HttpModel {
        url: join_url(&base_url, provider.path_suffix()),
        api_key,
        model: model_name,
        provider,
        max_tokens: resolve_max_tokens(overrides, &merged),
    }
    .try_with_env_reasoning_mode(merged.reasoning_mode)
    .unwrap_or_else(|e| {
        eprintln!("zorp-agent: {e}");
        std::process::exit(2);
    });
    let steps = overrides
        .max_steps
        .or_else(|| {
            std::env::var("ZORP_MAX_STEPS")
                .ok()
                .and_then(|v| v.parse().ok())
        })
        .or(merged.max_steps)
        .unwrap_or(20);

    let session_id = new_session_id();
    let mut agent = Agent::new(
        Box::new(model),
        system,
        steps,
        cwd.clone(),
        cancel,
        approval,
    )
    .register_builtins_filtered(merged.tools.enabled.as_deref())
    .with_policy(build_policy(overrides.approval.as_deref(), &gated, &cwd));

    agent = attach_mcp_tools(agent, overrides, true);

    agent = attach_verifier(agent, no_verify, &gated);

    // Attach a recorder when the store is available; the run proceeds regardless.
    let recorder_store = open_store();
    if let Some(store) = &recorder_store {
        if let Err(e) = store.create_session(&session_id, &task, &cwd.display().to_string(), "") {
            eprintln!("zorp-agent: could not create session: {e}");
        } else if let Ok(rec_store) = Store::open_default() {
            agent = agent.with_recorder(Box::new(SqliteRecorder::new(
                rec_store,
                session_id.clone(),
                0,
                0,
            )));
        }
    }

    if images.is_empty() {
        let outcome = agent.run(&task);
        let status_target = recorder_store.as_ref().map(|s| (s, session_id.as_str()));
        finish(outcome, status_target);
    } else {
        use zorp_agent::ContentPart;
        let mut parts: Vec<ContentPart> = Vec::new();
        for path in images {
            let data = std::fs::read(path).unwrap_or_else(|e| {
                eprintln!("zorp-agent: cannot read image {}: {e}", path.display());
                std::process::exit(2);
            });
            let mime_type = mime_from_extension(path);
            parts.push(ContentPart::Image { data, mime_type });
        }
        parts.push(ContentPart::Text(task));
        let outcome = agent.run_multimodal(parts);
        let status_target = recorder_store.as_ref().map(|s| (s, session_id.as_str()));
        finish(outcome, status_target);
    }
}

/// Prepended to the composed system prompt for `validate`, which narrows the
/// default prompt's general research framing to this one job: scoring a
/// hypothesis, and not touching code while doing it. The task prompt
/// (TASK_PROMPT_PREFIX in validate/mod.rs) already spells out the exact
/// scoring/citation format; this just sets the frame before that.
#[cfg(feature = "research")]
const VALIDATE_SYSTEM_PREAMBLE: &str = "\
You are conducting research to validate a hypothesis, not writing or \
modifying code. Every claim you make must be backed by a citation to \
something you actually found; do not assert a score or conclusion without one.";

#[cfg(feature = "research")]
fn validate(question: &str, auto_approve: bool, overrides: &Overrides) {
    let cancel = install_cancel();
    let approval = ApprovalMode::terminal(auto_approve);
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let (user_flavor, project_flavor) = resolve_flavor(overrides);
    let gated = gated_flavor(
        &user_flavor,
        &project_flavor,
        overrides.flavor.as_deref(),
        auto_approve,
    );
    let merged = user_flavor.clone().merge(project_flavor);
    let mut system = VALIDATE_SYSTEM_PREAMBLE.to_string();
    system.push_str("\n\n");
    system.push_str(&compose_system_with_persona(
        &cwd,
        persona(&cwd, &merged).as_deref(),
    ));
    let (base_url, model_name) = resolve_host_and_model(overrides, &merged);
    let provider = resolve_provider(overrides, &merged).unwrap_or_else(|e| {
        eprintln!("zorp-agent: {e}");
        std::process::exit(2);
    });
    let api_key = std::env::var("ZORP_API_KEY").ok().filter(|s| !s.is_empty());
    let model = HttpModel {
        url: join_url(&base_url, provider.path_suffix()),
        api_key,
        model: model_name,
        provider,
        max_tokens: resolve_max_tokens(overrides, &merged),
    }
    .try_with_env_reasoning_mode(merged.reasoning_mode)
    .unwrap_or_else(|e| {
        eprintln!("zorp-agent: {e}");
        std::process::exit(2);
    });
    let steps = overrides
        .max_steps
        .or_else(|| {
            std::env::var("ZORP_MAX_STEPS")
                .ok()
                .and_then(|v| v.parse().ok())
        })
        .or(merged.max_steps)
        .unwrap_or(20);

    let mut agent = Agent::new(
        Box::new(model),
        system,
        steps,
        cwd.clone(),
        cancel,
        approval,
    )
    .register_builtins_filtered(merged.tools.enabled.as_deref())
    .with_policy(build_policy(overrides.approval.as_deref(), &gated, &cwd));

    agent = attach_mcp_tools(agent, overrides, true);

    let project = match zorp_track::Project::open(&cwd) {
        Ok(p) => p,
        Err(e) => {
            // Exit 1, not 2. Two is this binary's usage-error code (no
            // arguments, unknown flag). A store that will not open is a
            // runtime failure, most often another zorp run holding the
            // DuckDB lock, and a caller scripting zorp should be able to
            // tell a locked database from a mistyped command.
            eprintln!("zorp-agent: {e}");
            std::process::exit(1);
        }
    };
    let track_id = zorp_track::id::track_id(question);
    if let Err(e) = get_or_create_track(&project.store, &track_id, question) {
        eprintln!("zorp-agent: {e}");
        std::process::exit(2);
    }
    let checkpoint_mode = match zorp_track::checkpoint::CheckpointMode::terminal(auto_approve) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("zorp-agent: {e}");
            std::process::exit(2);
        }
    };
    match zorp_agent::validate::run(&mut agent, &project, &track_id, question, &checkpoint_mode) {
        Ok(true) => println!("validate: approved, track {track_id} ready for investigate"),
        Ok(false) => println!("validate: rejected, track {track_id} killed"),
        Err(e) => {
            eprintln!("zorp-agent: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(feature = "research")]
const INVESTIGATE_SYSTEM_PREAMBLE: &str = "\
You are running one staged attempt on a hypothesis that has already been \
pre-registered: a metric name and a kill threshold were committed before \
this attempt started and cannot be changed by you. Work the problem, then \
report the metric's actual value honestly, even if it misses the \
threshold.";

#[cfg(feature = "research")]
fn investigate(
    question: &str,
    metric_name: Option<String>,
    kill_threshold: Option<f64>,
    threshold_direction: Option<String>,
    auto_approve: bool,
    overrides: &Overrides,
) {
    // A NaN or infinite threshold would be written into the prereg and
    // then never compare equal to itself again (NaN != NaN), locking the
    // track out of any later run that passes the flags explicitly. Refuse
    // it here, before anything is recorded.
    if kill_threshold.is_some_and(|t| !t.is_finite()) {
        eprintln!("zorp-agent: --kill-threshold must be a finite number");
        std::process::exit(2);
    }
    let cancel = install_cancel();
    let approval = ApprovalMode::terminal(auto_approve);
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let (user_flavor, project_flavor) = resolve_flavor(overrides);
    let gated = gated_flavor(
        &user_flavor,
        &project_flavor,
        overrides.flavor.as_deref(),
        auto_approve,
    );
    let merged = user_flavor.clone().merge(project_flavor);
    let mut system = INVESTIGATE_SYSTEM_PREAMBLE.to_string();
    system.push_str("\n\n");
    system.push_str(&compose_system_with_persona(
        &cwd,
        persona(&cwd, &merged).as_deref(),
    ));
    let (base_url, model_name) = resolve_host_and_model(overrides, &merged);
    let provider = resolve_provider(overrides, &merged).unwrap_or_else(|e| {
        eprintln!("zorp-agent: {e}");
        std::process::exit(2);
    });
    let api_key = std::env::var("ZORP_API_KEY").ok().filter(|s| !s.is_empty());
    let model = HttpModel {
        url: join_url(&base_url, provider.path_suffix()),
        api_key,
        model: model_name,
        provider,
        max_tokens: resolve_max_tokens(overrides, &merged),
    }
    .try_with_env_reasoning_mode(merged.reasoning_mode)
    .unwrap_or_else(|e| {
        eprintln!("zorp-agent: {e}");
        std::process::exit(2);
    });
    let steps = overrides
        .max_steps
        .or_else(|| {
            std::env::var("ZORP_MAX_STEPS")
                .ok()
                .and_then(|v| v.parse().ok())
        })
        .or(merged.max_steps)
        .unwrap_or(20);

    let mut agent = Agent::new(
        Box::new(model),
        system,
        steps,
        cwd.clone(),
        cancel,
        approval,
    )
    .register_builtins_filtered(merged.tools.enabled.as_deref())
    .with_policy(build_policy(overrides.approval.as_deref(), &gated, &cwd));

    agent = attach_mcp_tools(agent, overrides, true);

    let project = match zorp_track::Project::open(&cwd) {
        Ok(p) => p,
        Err(e) => {
            // Exit 1, not 2. Two is this binary's usage-error code (no
            // arguments, unknown flag). A store that will not open is a
            // runtime failure, most often another zorp run holding the
            // DuckDB lock, and a caller scripting zorp should be able to
            // tell a locked database from a mistyped command.
            eprintln!("zorp-agent: {e}");
            std::process::exit(1);
        }
    };
    let track_id = zorp_track::id::track_id(question);
    if let Err(e) = get_or_create_track(&project.store, &track_id, question) {
        eprintln!("zorp-agent: {e}");
        std::process::exit(2);
    }
    let checkpoint_mode = match zorp_track::checkpoint::CheckpointMode::terminal(auto_approve) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("zorp-agent: {e}");
            std::process::exit(2);
        }
    };

    let prereg_params = match (
        metric_name.as_deref(),
        kill_threshold,
        threshold_direction.as_deref(),
    ) {
        (Some(name), Some(threshold), Some(direction)) => {
            let Some(direction) = zorp_track::prereg::ThresholdDirection::parse(direction) else {
                eprintln!(
                    "zorp-agent: --threshold-direction must be lower-is-better or higher-is-better"
                );
                std::process::exit(2);
            };
            Some(zorp_agent::investigate::PreregParams {
                metric_name: name,
                kill_threshold: threshold,
                threshold_direction: direction,
            })
        }
        (None, None, None) => None,
        _ => {
            eprintln!("zorp-agent: --metric-name, --kill-threshold, and --threshold-direction must be given together");
            std::process::exit(2);
        }
    };

    match zorp_agent::investigate::run(
        &mut agent,
        &project,
        &track_id,
        question,
        prereg_params,
        &checkpoint_mode,
    ) {
        Ok(true) => println!("investigate: approved, track {track_id} stays active"),
        Ok(false) => println!("investigate: rejected, track {track_id} killed"),
        Err(e) => {
            eprintln!("zorp-agent: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(feature = "research")]
const CO_WRITE_SYSTEM_PREAMBLE: &str = "\
You are drafting an evidence-based artifact from a research run record. \
Cite only the metric values and verdict given to you; never invent a \
number. State confidence no higher than the evidence given supports.";

#[cfg(feature = "research")]
fn co_write(question: &str, auto_approve: bool, overrides: &Overrides) {
    let cancel = install_cancel();
    let approval = ApprovalMode::terminal(auto_approve);
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let (user_flavor, project_flavor) = resolve_flavor(overrides);
    let gated = gated_flavor(
        &user_flavor,
        &project_flavor,
        overrides.flavor.as_deref(),
        auto_approve,
    );
    let merged = user_flavor.clone().merge(project_flavor);
    let mut system = CO_WRITE_SYSTEM_PREAMBLE.to_string();
    system.push_str("\n\n");
    system.push_str(&compose_system_with_persona(
        &cwd,
        persona(&cwd, &merged).as_deref(),
    ));
    let (base_url, model_name) = resolve_host_and_model(overrides, &merged);
    let provider = resolve_provider(overrides, &merged).unwrap_or_else(|e| {
        eprintln!("zorp-agent: {e}");
        std::process::exit(2);
    });
    let api_key = std::env::var("ZORP_API_KEY").ok().filter(|s| !s.is_empty());
    let model = HttpModel {
        url: join_url(&base_url, provider.path_suffix()),
        api_key,
        model: model_name,
        provider,
        max_tokens: resolve_max_tokens(overrides, &merged),
    }
    .try_with_env_reasoning_mode(merged.reasoning_mode)
    .unwrap_or_else(|e| {
        eprintln!("zorp-agent: {e}");
        std::process::exit(2);
    });
    let steps = overrides
        .max_steps
        .or_else(|| {
            std::env::var("ZORP_MAX_STEPS")
                .ok()
                .and_then(|v| v.parse().ok())
        })
        .or(merged.max_steps)
        .unwrap_or(20);

    let mut agent = Agent::new(
        Box::new(model),
        system,
        steps,
        cwd.clone(),
        cancel,
        approval,
    )
    .register_builtins_filtered(merged.tools.enabled.as_deref())
    .with_policy(build_policy(overrides.approval.as_deref(), &gated, &cwd));

    agent = attach_mcp_tools(agent, overrides, true);

    let project = match zorp_track::Project::open(&cwd) {
        Ok(p) => p,
        Err(e) => {
            // Exit 1, not 2. Two is this binary's usage-error code (no
            // arguments, unknown flag). A store that will not open is a
            // runtime failure, most often another zorp run holding the
            // DuckDB lock, and a caller scripting zorp should be able to
            // tell a locked database from a mistyped command.
            eprintln!("zorp-agent: {e}");
            std::process::exit(1);
        }
    };
    let track_id = zorp_track::id::track_id(question);
    if let Err(e) = get_or_create_track(&project.store, &track_id, question) {
        eprintln!("zorp-agent: {e}");
        std::process::exit(2);
    }
    let checkpoint_mode = match zorp_track::checkpoint::CheckpointMode::terminal(auto_approve) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("zorp-agent: {e}");
            std::process::exit(2);
        }
    };

    match zorp_agent::co_write::run(&mut agent, &project, &track_id, question, &checkpoint_mode) {
        Ok(true) => println!(
            "co-write: approved, draft ready for review at .zorp/tracks/{track_id}/draft.md"
        ),
        Ok(false) => {
            println!("co-write: not yet approved, draft left at .zorp/tracks/{track_id}/draft.md")
        }
        Err(e) => {
            eprintln!("zorp-agent: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(feature = "research")]
const DELIVER_SYSTEM_PREAMBLE: &str = "\
You are matching a finished draft against real academic venues using \
the tools available to you. Only report venues you actually found \
through those tools; never invent a conference or journal name.";

#[cfg(feature = "research")]
fn deliver(question: &str, auto_approve: bool, overrides: &Overrides) {
    let cancel = install_cancel();
    let approval = ApprovalMode::terminal(auto_approve);
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let (user_flavor, project_flavor) = resolve_flavor(overrides);
    let gated = gated_flavor(
        &user_flavor,
        &project_flavor,
        overrides.flavor.as_deref(),
        auto_approve,
    );
    let merged = user_flavor.clone().merge(project_flavor);
    let mut system = DELIVER_SYSTEM_PREAMBLE.to_string();
    system.push_str("\n\n");
    system.push_str(&compose_system_with_persona(
        &cwd,
        persona(&cwd, &merged).as_deref(),
    ));
    let (base_url, model_name) = resolve_host_and_model(overrides, &merged);
    let provider = resolve_provider(overrides, &merged).unwrap_or_else(|e| {
        eprintln!("zorp-agent: {e}");
        std::process::exit(2);
    });
    let api_key = std::env::var("ZORP_API_KEY").ok().filter(|s| !s.is_empty());
    let model = HttpModel {
        url: join_url(&base_url, provider.path_suffix()),
        api_key,
        model: model_name,
        provider,
        max_tokens: resolve_max_tokens(overrides, &merged),
    }
    .try_with_env_reasoning_mode(merged.reasoning_mode)
    .unwrap_or_else(|e| {
        eprintln!("zorp-agent: {e}");
        std::process::exit(2);
    });
    let steps = overrides
        .max_steps
        .or_else(|| {
            std::env::var("ZORP_MAX_STEPS")
                .ok()
                .and_then(|v| v.parse().ok())
        })
        .or(merged.max_steps)
        .unwrap_or(20);

    let mut agent = Agent::new(
        Box::new(model),
        system,
        steps,
        cwd.clone(),
        cancel,
        approval,
    )
    .register_builtins_filtered(merged.tools.enabled.as_deref())
    .with_policy(build_policy(overrides.approval.as_deref(), &gated, &cwd));

    agent = attach_mcp_tools(agent, overrides, true);

    let project = match zorp_track::Project::open(&cwd) {
        Ok(p) => p,
        Err(e) => {
            // Exit 1, not 2. Two is this binary's usage-error code (no
            // arguments, unknown flag). A store that will not open is a
            // runtime failure, most often another zorp run holding the
            // DuckDB lock, and a caller scripting zorp should be able to
            // tell a locked database from a mistyped command.
            eprintln!("zorp-agent: {e}");
            std::process::exit(1);
        }
    };
    let track_id = zorp_track::id::track_id(question);
    if let Err(e) = get_or_create_track(&project.store, &track_id, question) {
        eprintln!("zorp-agent: {e}");
        std::process::exit(2);
    }
    let checkpoint_mode = match zorp_track::checkpoint::CheckpointMode::terminal(auto_approve) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("zorp-agent: {e}");
            std::process::exit(2);
        }
    };

    match zorp_agent::deliver::run(&mut agent, &project, &track_id, question, &checkpoint_mode) {
        Ok(true) => println!(
            "deliver: approved, shortlist ready for review at .zorp/tracks/{track_id}/venues.md"
        ),
        Ok(false) => println!(
            "deliver: not yet approved, shortlist left at .zorp/tracks/{track_id}/venues.md"
        ),
        Err(e) => {
            eprintln!("zorp-agent: {e}");
            std::process::exit(1);
        }
    }
}

/// Ensure a track exists for `track_id`/`question`, creating it if absent.
///
/// `track_id` is a lowercased, punctuation-stripped, 60-char-truncated
/// slug of the question (see `zorp_track::id::track_id`), so two distinct
/// questions can collide onto the same id within the same day. A retry of
/// the *same* question (e.g. after a prior run failed before completing)
/// is expected to reuse the existing row; a collision with a genuinely
/// different question must not silently proceed using the wrong track's
/// data, so it is reported as an error instead.
#[cfg(feature = "research")]
fn get_or_create_track(
    store: &zorp_track::Store,
    track_id: &str,
    question: &str,
) -> Result<(), String> {
    match store.get_track(track_id) {
        Ok(existing) if existing.hypothesis == question => Ok(()),
        Ok(existing) => Err(format!(
            "track id '{track_id}' is already registered today for a different question ({:?}); refusing to reuse it for ({:?}). Rephrase the question so it produces a distinct id.",
            existing.hypothesis, question
        )),
        Err(zorp_track::TrackError::NotFound { .. }) => store
            .create_track(track_id, question)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        Err(e) => Err(e.to_string()),
    }
}

const HELP: &str = "\
/commands            list available tools (same as /tools)
/exit, /quit, /q     leave chat
/help, /h, /?        show this help
/model               show the active model
/context             show transcript size
/diff                summarize this session's file changes
/status              show session id and status
/undo                revert the last recorded file change
/approve             auto-approve edits and commands this session
/deny                deny edits and commands this session
/clear               forget the conversation (keep system prompt)
/reasoning           show the active session reasoning mode
/reasoning <mode>    set reasoning mode for future turns in this session
/capsules            list available and loaded capsules
/load <name>         load a capsule
/unload <name>       unload a capsule
/<capsule_name> [text]  load a capsule (if needed) and optionally send a prompt through it
/capsule-create <name> <what it should do>  draft and load a new capsule via the agent

Images:
  @image <path>      attach an image file to your prompt
  @img <path>        alias for @image
  Ctrl+V             paste image from clipboard (requires --features clipboard)
  Drag & drop        drag an image file into the terminal
  --image <path>     attach image in one-shot mode (repeatable)";

/// Disables terminal raw mode on drop, so a panic anywhere in the chat loop
/// cannot leave the user's shell with echo off.
struct RawModeGuard;

impl RawModeGuard {
    fn enable() -> Option<RawModeGuard> {
        crossterm::terminal::enable_raw_mode()
            .ok()
            .map(|()| RawModeGuard)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

/// The part of a chat REPL's context that is fixed for as long as the REPL
/// runs: which session it is, where the store recording it lives, what
/// directory it is rooted in, and which model it talks to. These four were
/// being threaded as separate parameters, in the same order, through
/// `chat_line_loop` and `handle_chat_command` and repeated at every call
/// site. Naming them once removes that duplication and drops both functions
/// back under clippy's argument limit. Copy, because it is four shared
/// references and callers pass it on every loop iteration.
#[derive(Clone, Copy)]
struct ChatContext<'a> {
    store: &'a Option<Store>,
    session_id: &'a str,
    cwd: &'a Path,
    model_name: &'a str,
}

/// Line-based chat input loop, used for piped stdin and as the fallback when
/// raw mode cannot be enabled on a TTY.
fn chat_line_loop(
    agent: &mut Agent,
    ctx: ChatContext<'_>,
    capsules: &mut CapsuleState,
    out: &mut dyn Renderer,
) {
    let ChatContext {
        store, session_id, ..
    } = ctx;
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    loop {
        print!("› ");
        let _ = std::io::stdout().flush();
        let Some(line) = lines.next() else { break };
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let exit = handle_chat_command(&line, agent, ctx, capsules, out);
        if exit {
            break;
        }
    }
    if let Some(s) = store {
        let _ = s.set_status(session_id, "done");
    }
    out.notice("bye");
}

fn chat(auto_approve: bool, no_verify: bool, overrides: &Overrides) {
    let cancel = install_cancel();
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let (user_flavor, project_flavor) = resolve_flavor(overrides);
    let gated = gated_flavor(
        &user_flavor,
        &project_flavor,
        overrides.flavor.as_deref(),
        auto_approve,
    );
    let merged = user_flavor.clone().merge(project_flavor);
    let system = compose_system_with_persona(&cwd, persona(&cwd, &merged).as_deref());
    let user_capsules_dir = default_user_capsules_dir().unwrap_or_default();
    let project_capsules_dir_path = project_capsules_dir(&cwd);
    let capsule_registry =
        CapsuleRegistry::discover(&user_capsules_dir, &project_capsules_dir_path);
    let mut capsules = CapsuleState::new(capsule_registry, system.clone());
    let (base_url, model_name) = resolve_host_and_model(overrides, &merged);
    let provider = resolve_provider(overrides, &merged).unwrap_or_else(|e| {
        eprintln!("zorp-agent: {e}");
        std::process::exit(2);
    });
    let model = HttpModel {
        url: join_url(&base_url, provider.path_suffix()),
        api_key: std::env::var("ZORP_API_KEY").ok().filter(|s| !s.is_empty()),
        model: model_name.clone(),
        provider,
        max_tokens: resolve_max_tokens(overrides, &merged),
    }
    .try_with_env_reasoning_mode(merged.reasoning_mode)
    .unwrap_or_else(|e| {
        eprintln!("zorp-agent: {e}");
        std::process::exit(2);
    });
    let steps = overrides
        .max_steps
        .or_else(|| {
            std::env::var("ZORP_MAX_STEPS")
                .ok()
                .and_then(|v| v.parse().ok())
        })
        .or(merged.max_steps)
        .unwrap_or(20);

    let color = std::io::stdout().is_terminal();
    let spinner_verbs = parse_spinner_verbs(std::env::var("ZORP_SPINNER_VERBS").ok().as_deref());
    let approval = if auto_approve {
        ApprovalMode::AutoApprove
    } else {
        ApprovalMode::NonInteractive
    };
    let session_id = new_session_id();
    let mut agent = Agent::new(
        Box::new(model),
        system,
        steps,
        cwd.clone(),
        cancel,
        approval,
    )
    .register_builtins_filtered(merged.tools.enabled.as_deref())
    .with_policy(build_policy(overrides.approval.as_deref(), &gated, &cwd))
    .with_renderer(if color {
        chat_spinner_renderer(spinner_verbs)
    } else {
        Box::new(LineRenderer::new(std::io::stdout(), color))
    });

    agent = attach_mcp_tools(agent, overrides, true);

    agent = attach_verifier(agent, no_verify, &gated);

    let store = open_store();
    if let Some(s) = &store {
        if let Err(e) = s.create_session_with_reasoning_mode(
            &session_id,
            "chat",
            &cwd.display().to_string(),
            "",
            merged.reasoning_mode,
        ) {
            eprintln!("zorp-agent: could not create session: {e}");
        } else if let Ok(rec_store) = Store::open_default() {
            agent = agent.with_recorder(Box::new(SqliteRecorder::new(
                rec_store,
                session_id.clone(),
                0,
                0,
            )));
        }
    }

    let mut out = LineRenderer::new(std::io::stdout(), color);
    out.notice("zorp-agent chat — /help for commands, /exit to quit");

    let ctx = ChatContext {
        store: &store,
        session_id: &session_id,
        cwd: &cwd,
        model_name: &model_name,
    };

    if !std::io::stdin().is_terminal() {
        chat_line_loop(&mut agent, ctx, &mut capsules, &mut out);
        return;
    }

    let mut segments = vec![Segment::Text(String::new())];
    let mut image_counter: usize = 0;
    let mut redraw = true;

    // The guard restores the terminal even if the loop below panics. When raw
    // mode is unavailable, fall back to plain line input instead of dying.
    let Some(_raw_guard) = RawModeGuard::enable() else {
        eprintln!("zorp-agent: could not enable raw terminal mode; using line input");
        chat_line_loop(&mut agent, ctx, &mut capsules, &mut out);
        return;
    };
    #[cfg(feature = "clipboard")]
    let mut clipboard = arboard::Clipboard::new().ok();
    let _ = crossterm::execute!(std::io::stdout(), crossterm::event::EnableBracketedPaste);

    loop {
        if redraw {
            let mut prompt = String::from("\r› ");
            for seg in &segments {
                match seg {
                    Segment::Text(t) => prompt.push_str(t),
                    Segment::Paste(s) => {
                        prompt.push_str(&format!("[pasted +{} characters]", s.len()))
                    }
                    Segment::Image { index, .. } => prompt.push_str(&format!("[Image {}]", index)),
                }
            }
            let _ = crossterm::execute!(
                std::io::stdout(),
                crossterm::terminal::Clear(crossterm::terminal::ClearType::CurrentLine)
            );
            print!("{}", prompt);
            let _ = std::io::stdout().flush();
            redraw = false;
        }

        if let Ok(event) = crossterm::event::read() {
            match event {
                crossterm::event::Event::Key(key)
                    if key.kind == crossterm::event::KeyEventKind::Press =>
                {
                    match key.code {
                        crossterm::event::KeyCode::Char('c')
                            if key
                                .modifiers
                                .contains(crossterm::event::KeyModifiers::CONTROL) =>
                        {
                            println!("\r");
                            break;
                        }
                        crossterm::event::KeyCode::Char('d')
                            if key
                                .modifiers
                                .contains(crossterm::event::KeyModifiers::CONTROL) =>
                        {
                            println!("\r");
                            break;
                        }
                        crossterm::event::KeyCode::Enter => {
                            println!("\r");
                            // Convert segments to content parts
                            use zorp_agent::ContentPart;
                            let parts = segments_to_parts(&segments, &cwd);

                            segments.clear();
                            segments.push(Segment::Text(String::new()));
                            image_counter = 0;

                            let _ = crossterm::execute!(
                                std::io::stdout(),
                                crossterm::event::DisableBracketedPaste
                            );
                            let _ = crossterm::terminal::disable_raw_mode();

                            // Determine if we have content to send
                            let has_content = parts.iter().any(|p| match p {
                                ContentPart::Text(t) => !t.trim().is_empty(),
                                ContentPart::Image { .. } => true,
                            });

                            if has_content {
                                // Check if it's a command (first text part)
                                let first_text = parts
                                    .iter()
                                    .find_map(|p| match p {
                                        ContentPart::Text(t) => Some(t.as_str()),
                                        _ => None,
                                    })
                                    .unwrap_or("");

                                if first_text.trim_start().starts_with('/')
                                    && !parts.iter().any(|p| matches!(p, ContentPart::Image { .. }))
                                {
                                    // Pure text command — use existing command handler
                                    let exit = handle_chat_command(
                                        first_text,
                                        &mut agent,
                                        ctx,
                                        &mut capsules,
                                        &mut out,
                                    );
                                    if exit {
                                        break;
                                    }
                                } else {
                                    // Multimodal or text message
                                    match agent.run_multimodal(parts) {
                                        Outcome::Complete(answer) => out.assistant(&answer),
                                        Outcome::StepLimit => out.notice("(step limit reached)"),
                                        Outcome::VerificationFailed { attempts } => out.notice(
                                            &format!("(verification still failing after {attempts} attempts)"),
                                        ),
                                        Outcome::Cancelled => out.notice("(cancelled)"),
                                        Outcome::RepeatedAction => out.notice("(stopped: repeated action)"),
                                        Outcome::Blocked => out.notice(
                                            "(stopped: actions denied, use /approve to allow this session)",
                                        ),
                                        Outcome::Error(e) => out.notice(&format!("(error: {e})")),
                                    }
                                }
                            }

                            if crossterm::terminal::enable_raw_mode().is_err() {
                                out.notice("(could not re-enable raw terminal mode; exiting chat)");
                                break;
                            }
                            let _ = crossterm::execute!(
                                std::io::stdout(),
                                crossterm::event::EnableBracketedPaste
                            );
                            redraw = true;
                        }
                        crossterm::event::KeyCode::Backspace => {
                            let mut pop_segment = false;
                            if let Some(last) = segments.last_mut() {
                                match last {
                                    Segment::Text(t) => {
                                        if !t.is_empty() {
                                            t.pop();
                                        } else {
                                            pop_segment = true;
                                        }
                                    }
                                    Segment::Paste(_) => {
                                        pop_segment = true;
                                    }
                                    Segment::Image { .. } => {
                                        pop_segment = true;
                                    }
                                }
                            }
                            if pop_segment {
                                segments.pop();
                            }
                            if segments.is_empty() {
                                segments.push(Segment::Text(String::new()));
                            }
                            redraw = true;
                        }
                        #[cfg(feature = "clipboard")]
                        crossterm::event::KeyCode::Char('v')
                            if key
                                .modifiers
                                .contains(crossterm::event::KeyModifiers::CONTROL)
                                || key
                                    .modifiers
                                    .contains(crossterm::event::KeyModifiers::SUPER) =>
                        {
                            let mut used_clipboard = false;
                            if let Some(ref mut cb) = clipboard {
                                if let Ok(img) = cb.get_image() {
                                    // Encode RGBA to PNG
                                    let mut png_buf = Vec::new();
                                    if let Ok(()) = {
                                        let encoder = image::codecs::png::PngEncoder::new(
                                            std::io::Cursor::new(&mut png_buf),
                                        );
                                        image::ImageEncoder::write_image(
                                            encoder,
                                            &img.bytes,
                                            img.width as u32,
                                            img.height as u32,
                                            image::ExtendedColorType::Rgba8,
                                        )
                                    } {
                                        image_counter += 1;
                                        segments.push(Segment::Image {
                                            data: png_buf,
                                            mime_type: "image/png".into(),
                                            index: image_counter,
                                        });
                                        segments.push(Segment::Text(String::new()));
                                        used_clipboard = true;
                                    }
                                }
                            }
                            if !used_clipboard {
                                // Fall through to normal 'v' character
                                if let Some(Segment::Text(t)) = segments.last_mut() {
                                    t.push('v');
                                } else {
                                    segments.push(Segment::Text("v".to_string()));
                                }
                            }
                            redraw = true;
                        }
                        crossterm::event::KeyCode::Char(c) => {
                            if let Some(Segment::Text(t)) = segments.last_mut() {
                                t.push(c);
                            } else {
                                segments.push(Segment::Text(c.to_string()));
                            }
                            redraw = true;
                        }
                        _ => {}
                    }
                }
                crossterm::event::Event::Paste(s) => {
                    let trimmed = s.trim().trim_matches('\'').trim_matches('"');
                    let path = std::path::Path::new(trimmed);
                    if path.is_file() && is_image_extension(path) {
                        if let Ok(data) = std::fs::read(path) {
                            image_counter += 1;
                            let mime_type = mime_from_extension(path);
                            segments.push(Segment::Image {
                                data,
                                mime_type,
                                index: image_counter,
                            });
                            segments.push(Segment::Text(String::new()));
                        } else {
                            segments.push(Segment::Paste(s));
                            segments.push(Segment::Text(String::new()));
                        }
                    } else {
                        segments.push(Segment::Paste(s));
                        segments.push(Segment::Text(String::new()));
                    }
                    redraw = true;
                }
                _ => {}
            }
        }
    }

    let _ = crossterm::execute!(std::io::stdout(), crossterm::event::DisableBracketedPaste);
    let _ = crossterm::terminal::disable_raw_mode();

    if let Some(s) = &store {
        let _ = s.set_status(&session_id, "done");
    }
    out.notice("bye");
}
/// Dispatch one parsed chat-REPL command line against the running agent and
/// session. Shared by the non-TTY (piped stdin) and TTY (raw-mode) input
/// loops in `chat`, which otherwise duplicated this match verbatim. Returns
/// `true` if the REPL should exit.
fn handle_chat_command(
    line: &str,
    agent: &mut Agent,
    ctx: ChatContext<'_>,
    capsules: &mut CapsuleState,
    out: &mut dyn Renderer,
) -> bool {
    let ChatContext {
        store,
        session_id,
        cwd,
        model_name,
    } = ctx;
    let mut exit = false;
    let capsule_names = capsules.registry().names();
    match parse_command(line, &capsule_names) {
        ChatCommand::Exit => exit = true,
        ChatCommand::Help => out.notice(HELP),
        ChatCommand::Model => {
            if model_name.is_empty() {
                out.notice("model: (not set)");
            } else {
                out.notice(&format!("model: {model_name}"));
            }
        }
        ChatCommand::Context => {
            let msg_n = agent.messages.len().saturating_sub(1);
            let char_count: usize = agent
                .messages
                .iter()
                .map(|m| {
                    m.text().len()
                        + m.tool_calls
                            .iter()
                            .map(|tc| tc.name.len() + tc.arguments.to_string().len())
                            .sum::<usize>()
                })
                .sum();
            out.notice(&format!(
                "session: {} ({} messages, ~{} chars)",
                session_id, msg_n, char_count
            ));
        }
        ChatCommand::Status => {
            let status = store
                .as_ref()
                .and_then(|s| s.session_status(session_id).ok().flatten())
                .unwrap_or_else(|| "unknown".to_string());
            let bg_count = agent.background_process_count();
            if bg_count > 0 {
                let plural = if bg_count == 1 {
                    "process"
                } else {
                    "processes"
                };
                out.notice(&format!(
                    "session {session_id} [{status}] ({} background {} running)",
                    bg_count, plural
                ));
            } else {
                out.notice(&format!("session {session_id} [{status}]"));
            }
        }
        ChatCommand::Diff => {
            if let Some(s) = &store {
                let changes = s.load_changes(session_id).unwrap_or_default();
                out.notice(render_change_summary(&changes).trim_end());
            } else {
                out.notice("no session store");
            }
        }
        ChatCommand::Undo => chat_undo(store, session_id, cwd, out),
        ChatCommand::Approve => {
            agent.set_approval(ApprovalMode::AutoApprove);
            out.notice("edits and commands will be auto-approved this session");
        }
        ChatCommand::Deny => {
            agent.set_approval(ApprovalMode::NonInteractive);
            out.notice("edits and commands will be denied this session");
        }
        ChatCommand::Clear => {
            agent.clear_history();
            out.notice(&format!("session {} conversation cleared", session_id));
        }
        ChatCommand::Tools => {
            out.notice(&agent.tool_names().join("\n"));
        }
        ChatCommand::Reasoning(ReasoningCommand::Show) => {
            out.notice(&format!(
                "reasoning: {}",
                agent
                    .session_reasoning_mode()
                    .map(|mode| mode.effort_str())
                    .unwrap_or("off")
            ));
        }
        ChatCommand::Reasoning(ReasoningCommand::Set(raw)) => {
            let mode = if raw.eq_ignore_ascii_case("off") {
                None
            } else {
                match raw.parse::<ReasoningMode>() {
                    Ok(mode) => Some(mode),
                    Err(_) => {
                        out.notice(&format!("unknown reasoning mode: {raw}"));
                        return exit;
                    }
                }
            };
            if let Err(e) = agent.set_session_reasoning_mode(mode) {
                out.notice(&format!("error: {e}"));
                return exit;
            }
            if let Some(s) = store {
                if let Err(e) = s.set_session_reasoning_mode(session_id, mode) {
                    out.notice(&format!("error: {e}"));
                    return exit;
                }
            }
            match mode {
                Some(mode) => out.notice(&format!("reasoning set to {}", mode.effort_str())),
                None => out.notice("reasoning turned off"),
            }
        }
        ChatCommand::Capsules => out.notice(&capsules.list_display()),
        ChatCommand::LoadCapsule(name) => {
            if name.is_empty() {
                out.notice("usage: /load <capsule_name>");
            } else {
                match capsules.load(&name) {
                    Ok(true) => {
                        agent.messages[0] = Message::system(capsules.render_system_prompt());
                        out.notice(&format!("loaded {name}"));
                    }
                    Ok(false) => out.notice(&format!("{name} already loaded")),
                    Err(msg) => out.notice(&msg),
                }
            }
        }
        ChatCommand::UnloadCapsule(name) => {
            if name.is_empty() {
                out.notice("usage: /unload <capsule_name>");
            } else if capsules.unload(&name) {
                agent.messages[0] = Message::system(capsules.render_system_prompt());
                out.notice(&format!("unloaded {name}"));
            } else {
                out.notice(&format!("{name} is not loaded"));
            }
        }
        ChatCommand::InvokeCapsule { name, prompt } => {
            match capsules.load(&name) {
                Ok(true) => {
                    agent.messages[0] = Message::system(capsules.render_system_prompt());
                    out.notice(&format!("loaded {name}"));
                }
                Ok(false) => {}
                Err(msg) => {
                    out.notice(&msg);
                    return exit;
                }
            }
            if let Some(text) = prompt {
                run_and_render(agent, &text, out);
            }
        }
        ChatCommand::CreateCapsule { name, description } => {
            if name.is_empty() || description.is_empty() {
                out.notice("usage: /capsule-create <name> <what it should do>");
            } else if is_reserved(&name) {
                out.notice(&format!(
                    "{name} is a reserved command name, choose another"
                ));
            } else if std::path::Path::new(&name).components().count() != 1
                || !matches!(
                    std::path::Path::new(&name).components().next(),
                    Some(std::path::Component::Normal(_))
                )
            {
                out.notice(&format!(
                    "{name} is not a valid capsule name (must be a single path component, no '/' or '..')"
                ));
            } else if capsules.registry().get(&name).is_some() {
                out.notice(&format!("capsule {name} already exists (see /capsules)"));
            } else {
                let meta_prompt = format!(
                    "Draft a CAPSULE.md file for a new capsule named `{name}` that does the \
                     following: {description}. Output ONLY the file content: YAML frontmatter \
                     with `name: {name}` and a one-line `description:`, followed by a `---` \
                     closing delimiter and a markdown instructions body. Wrap the entire file \
                     content in a single fenced code block and output nothing else."
                );
                match agent.run(&meta_prompt) {
                    Outcome::Complete(answer) => {
                        out.assistant(&answer);
                        let dir = project_capsules_dir(cwd).join(&name);
                        let drafted = extract_fenced_block(&answer)
                            .and_then(|block| Capsule::parse(&block, dir.clone()));
                        match drafted {
                            Ok(mut capsule) => {
                                if !capsule.name.eq_ignore_ascii_case(&name) {
                                    capsule.name = name.clone();
                                }
                                if let Err(e) = std::fs::create_dir_all(&dir) {
                                    out.notice(&format!(
                                        "capsule draft failed: could not create {}: {e}",
                                        dir.display()
                                    ));
                                } else {
                                    let file_text = format!(
                                        "---\nname: {}\ndescription: {}\n---\n{}\n",
                                        capsule.name, capsule.description, capsule.instructions
                                    );
                                    match std::fs::write(dir.join("CAPSULE.md"), file_text) {
                                        Ok(()) => {
                                            let path = dir.join("CAPSULE.md");
                                            capsules.create_and_load(capsule);
                                            agent.messages[0] =
                                                Message::system(capsules.render_system_prompt());
                                            out.notice(&format!(
                                                "created and loaded capsule {name} at {}",
                                                path.display()
                                            ));
                                        }
                                        Err(e) => out.notice(&format!(
                                            "capsule draft failed: could not write CAPSULE.md: {e}"
                                        )),
                                    }
                                }
                            }
                            Err(reason) => {
                                out.notice(&format!("capsule draft failed: {reason}"));
                            }
                        }
                    }
                    Outcome::StepLimit => out.notice("(step limit reached)"),
                    Outcome::VerificationFailed { attempts } => out.notice(&format!(
                        "(verification still failing after {attempts} attempts)"
                    )),
                    Outcome::Cancelled => out.notice("(cancelled)"),
                    Outcome::RepeatedAction => out.notice("(stopped: repeated action)"),
                    Outcome::Blocked => {
                        out.notice("(stopped: actions denied, use /approve to allow this session)")
                    }
                    Outcome::Error(e) => out.notice(&format!("(error: {e})")),
                }
            }
        }
        ChatCommand::Unknown(name) => {
            out.notice(&format!("unknown command '/{name}' — try /help"));
        }
        ChatCommand::Say(text) => {
            if !text.is_empty() {
                run_and_render(agent, &text, out);
            }
        }
    }
    exit
}

fn run_and_render(agent: &mut Agent, text: &str, out: &mut dyn Renderer) {
    match agent.run(text) {
        Outcome::Complete(answer) => out.assistant(&answer),
        Outcome::StepLimit => out.notice("(step limit reached)"),
        Outcome::VerificationFailed { attempts } => out.notice(&format!(
            "(verification still failing after {attempts} attempts)"
        )),
        Outcome::Cancelled => out.notice("(cancelled)"),
        Outcome::RepeatedAction => out.notice("(stopped: repeated action)"),
        Outcome::Blocked => {
            out.notice("(stopped: actions denied, use /approve to allow this session)")
        }
        Outcome::Error(e) => out.notice(&format!("(error: {e})")),
    }
}

fn chat_undo(store: &Option<Store>, session_id: &str, cwd: &Path, out: &mut dyn Renderer) {
    let Some(store) = store else {
        out.notice("no session store");
        return;
    };
    match store.take_last_change(session_id) {
        Ok(Some(change)) => {
            let path = cwd.join(&change.path);
            let result = match &change.before {
                Some(before) => std::fs::write(&path, before),
                None => std::fs::remove_file(&path),
            };
            match result {
                Ok(()) => out.notice(&format!("reverted {}", change.path)),
                Err(e) => out.notice(&format!("could not revert {}: {e}", change.path)),
            }
        }
        Ok(None) => out.notice("no changes to undo"),
        Err(e) => out.notice(&format!("error: {e}")),
    }
}

fn resume(id: &str, auto_approve: bool, no_verify: bool, overrides: &Overrides) {
    let store = match open_store() {
        Some(s) => s,
        None => std::process::exit(1),
    };
    let messages = match store.load_message_records(id) {
        Ok(m) if !m.is_empty() => m,
        Ok(_) => {
            eprintln!("zorp-agent: no session '{id}'");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("zorp-agent: {e}");
            std::process::exit(1);
        }
    };
    let cancel = install_cancel();
    let approval = ApprovalMode::terminal(auto_approve);
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let (user_flavor, project_flavor) = resolve_flavor(overrides);
    let gated = gated_flavor(
        &user_flavor,
        &project_flavor,
        overrides.flavor.as_deref(),
        auto_approve,
    );
    let merged = user_flavor.clone().merge(project_flavor);
    let system = compose_system_with_persona(&cwd, persona(&cwd, &merged).as_deref());
    let (base_url, model_name) = resolve_host_and_model(overrides, &merged);
    let provider = resolve_provider(overrides, &merged).unwrap_or_else(|e| {
        eprintln!("zorp-agent: {e}");
        std::process::exit(2);
    });
    let model = HttpModel {
        url: join_url(&base_url, provider.path_suffix()),
        api_key: std::env::var("ZORP_API_KEY").ok().filter(|s| !s.is_empty()),
        model: model_name,
        provider,
        max_tokens: resolve_max_tokens(overrides, &merged),
    }
    .try_with_env_reasoning_mode(
        store
            .session_reasoning_mode(id)
            .ok()
            .flatten()
            .or(merged.reasoning_mode),
    )
    .unwrap_or_else(|e| {
        eprintln!("zorp-agent: {e}");
        std::process::exit(2);
    });
    let steps = overrides
        .max_steps
        .or_else(|| {
            std::env::var("ZORP_MAX_STEPS")
                .ok()
                .and_then(|v| v.parse().ok())
        })
        .or(merged.max_steps)
        .unwrap_or(20);

    let msg_seq = store.message_count(id).unwrap_or(0);
    let change_seq = store.change_count(id).unwrap_or(0);
    let mut agent = Agent::new(
        Box::new(model),
        system,
        steps,
        cwd.clone(),
        cancel,
        approval,
    )
    .register_builtins_filtered(merged.tools.enabled.as_deref())
    .with_policy(build_policy(overrides.approval.as_deref(), &gated, &cwd))
    .with_message_records(messages);

    agent = attach_mcp_tools(agent, overrides, false);

    agent = attach_verifier(agent, no_verify, &gated);
    if let Ok(rec_store) = Store::open_default() {
        agent = agent.with_recorder(Box::new(SqliteRecorder::new(
            rec_store,
            id.to_string(),
            msg_seq,
            change_seq,
        )));
    }

    eprintln!("zorp-agent: resuming session {id}...");
    let outcome = agent.resume();
    finish(outcome, Some((&store, id)));
}

fn undo() {
    let store = match open_store() {
        Some(s) => s,
        None => std::process::exit(1),
    };
    let latest = match store.latest_session() {
        Ok(Some(s)) => s,
        Ok(None) => {
            eprintln!("zorp-agent: no sessions to undo");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("zorp-agent: {e}");
            std::process::exit(1);
        }
    };
    match store.take_last_change(&latest.id) {
        Ok(Some(change)) => {
            let path = PathBuf::from(&latest.repo).join(&change.path);
            let result = match &change.before {
                Some(before) => std::fs::write(&path, before),
                None => std::fs::remove_file(&path),
            };
            match result {
                Ok(()) => println!("reverted {}", change.path),
                Err(e) => {
                    eprintln!("zorp-agent: could not revert {}: {e}", change.path);
                    std::process::exit(1);
                }
            }
        }
        Ok(None) => {
            eprintln!("zorp-agent: no changes to undo");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("zorp-agent: {e}");
            std::process::exit(1);
        }
    }
}

fn diff() {
    let store = match open_store() {
        Some(s) => s,
        None => std::process::exit(1),
    };
    match store.latest_session() {
        Ok(Some(s)) => {
            let changes = store.load_changes(&s.id).unwrap_or_default();
            print!("{}", render_change_summary(&changes));
        }
        Ok(None) => {
            eprintln!("zorp-agent: no sessions");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("zorp-agent: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(not(feature = "mcp"))]
fn attach_mcp_tools(agent: Agent, _overrides: &Overrides, _add_prompt_additions: bool) -> Agent {
    agent
}

#[cfg(feature = "mcp")]
fn attach_mcp_tools(mut agent: Agent, overrides: &Overrides, add_prompt_additions: bool) -> Agent {
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use zorp_mcp::{McpConfig, McpRegistry};

    let file_cfg = McpConfig::from_file(Path::new(".zorp/mcp.toml")).unwrap_or_else(|e| {
        eprintln!("zorp-mcp: config warning: {e}");
        McpConfig::empty()
    });
    let env_cfg = McpConfig::from_env().unwrap_or_else(|e| {
        eprintln!("zorp-mcp: env warning: {e}");
        McpConfig::empty()
    });
    let cli_cfg = mcp_config_from_flags(&overrides.mcp);
    let merged = McpConfig::merged(file_cfg, env_cfg, cli_cfg);

    let mut registry = McpRegistry::new(merged);
    let mcp_tools = registry.discover();
    let prompt_additions = registry.system_prompt_additions();
    let registry_arc = Arc::new(Mutex::new(registry));

    for mcp_tool in mcp_tools {
        let adapter = zorp_agent::mcp_adapter::McpToolAdapter {
            tool: mcp_tool,
            registry: std::sync::Arc::clone(&registry_arc),
        };
        agent = agent.register(Box::new(adapter));
    }
    if add_prompt_additions {
        use zorp_agent::ContentPart;

        for addition in &prompt_additions {
            if let Some(msg) = agent.messages.first_mut() {
                if let Some(ContentPart::Text(text)) = msg.content.last_mut() {
                    text.push_str("\n\n");
                    text.push_str(addition);
                } else {
                    msg.content
                        .push(ContentPart::Text(format!("\n\n{addition}")));
                }
            }
        }
    }
    agent
}

#[cfg(feature = "mcp")]
fn mcp_config_from_flags(flags: &[String]) -> zorp_mcp::McpConfig {
    use std::collections::HashMap;
    use zorp_mcp::config::{ServerConfig, TransportKind, TrustLevel};
    let mut servers = Vec::new();
    for flag in flags {
        let parts: Vec<&str> = flag.splitn(3, ':').collect();
        if parts.len() < 3 {
            eprintln!("zorp-mcp: ignoring malformed --mcp flag: {flag}");
            continue;
        }
        let (transport_str, name, rest) = (parts[0], parts[1], parts[2]);
        let transport = match transport_str {
            "stdio" => TransportKind::Stdio,
            "streamable_http" => TransportKind::StreamableHttp,
            "sse" => TransportKind::Sse,
            other => {
                eprintln!("zorp-mcp: unknown transport '{other}'");
                continue;
            }
        };
        let server = match transport {
            TransportKind::Stdio => {
                let mut p = rest.split(':');
                let command = p.next().unwrap_or("").to_string();
                let args: Vec<String> = p.map(str::to_string).collect();
                ServerConfig {
                    name: name.to_string(),
                    transport,
                    command: Some(command),
                    args,
                    env: HashMap::new(),
                    url: None,
                    headers: HashMap::new(),
                    trust: TrustLevel::Sandbox,
                    timeout_secs: None,
                }
            }
            _ => ServerConfig {
                name: name.to_string(),
                transport,
                command: None,
                args: vec![],
                env: HashMap::new(),
                url: Some(rest.to_string()),
                headers: HashMap::new(),
                trust: TrustLevel::Sandbox,
                timeout_secs: None,
            },
        };
        servers.push(server);
    }
    zorp_mcp::McpConfig { servers }
}

#[cfg(test)]
mod main_tests {
    use super::*;
    use std::path::Path;
    use zorp_agent::Renderer;

    #[derive(Default)]
    struct TestRenderer {
        notices: Vec<String>,
        assistant_replies: Vec<String>,
    }

    impl Renderer for TestRenderer {
        fn tool(&mut self, _name: &str, _summary: &str) {}
        fn verify(&mut self, _command: &str, _passed: bool) {}
        fn notice(&mut self, text: &str) {
            self.notices.push(text.to_string());
        }
        fn assistant(&mut self, text: &str) {
            self.assistant_replies.push(text.to_string());
        }
    }

    fn test_agent(mode: Option<ReasoningMode>) -> Agent {
        let model = HttpModel {
            url: "http://example.test/v1/chat/completions".into(),
            api_key: None,
            model: "test-model".into(),
            provider: Provider::OpenAiCompatible,
            max_tokens: None,
        }
        .with_default_reasoning_mode(mode);
        Agent::new(
            Box::new(model),
            "system".to_string(),
            4,
            std::env::current_dir().unwrap(),
            cancel_token(),
            ApprovalMode::NonInteractive,
        )
    }

    #[derive(Clone)]
    struct FakeModel {
        reply: String,
    }

    impl zorp_agent::Model for FakeModel {
        fn complete(
            &self,
            _messages: &[zorp_agent::Message],
            _tools: &[serde_json::Value],
        ) -> Result<zorp_agent::AssistantMessage, zorp_agent::BoxErr> {
            Ok(zorp_agent::AssistantMessage {
                content: self.reply.clone(),
                tool_calls: vec![],
                finish_reason: "stop".to_string(),
                reasoning_content: None,
            })
        }

        fn clone_box(&self) -> Box<dyn zorp_agent::Model> {
            Box::new(self.clone())
        }
    }

    fn fake_agent(reply: &str) -> Agent {
        Agent::new(
            Box::new(FakeModel {
                reply: reply.to_string(),
            }),
            "system",
            4,
            std::env::current_dir().unwrap(),
            cancel_token(),
            ApprovalMode::NonInteractive,
        )
    }

    fn test_capsules() -> CapsuleState {
        CapsuleState::new(CapsuleRegistry::default(), "system".to_string())
    }

    fn write_capsule(root: &Path, name: &str, description: &str, body: &str) {
        std::fs::create_dir_all(root.join(name)).unwrap();
        std::fs::write(
            root.join(name).join("CAPSULE.md"),
            format!("---\nname: {name}\ndescription: {description}\n---\n{body}"),
        )
        .unwrap();
    }

    fn capsules_from(project_root: &Path) -> CapsuleState {
        let registry =
            CapsuleRegistry::discover(Path::new("/does-not-exist-user-dir"), project_root);
        CapsuleState::new(registry, "system".to_string())
    }

    /// The fixed half of a chat REPL's context. Every command test uses the
    /// same session id and model name, so only the store and the working
    /// directory are worth passing.
    fn test_ctx<'a>(store: &'a Option<Store>, cwd: &'a Path) -> ChatContext<'a> {
        ChatContext {
            store,
            session_id: "s1",
            cwd,
            model_name: "test-model",
        }
    }

    #[test]
    fn load_unknown_capsule_reports_error() {
        let mut agent = test_agent(None);
        let mut capsules = test_capsules();
        let store: Option<Store> = None;
        let mut out = TestRenderer::default();

        let exit = handle_chat_command(
            "/load demo",
            &mut agent,
            test_ctx(&store, Path::new("/repo")),
            &mut capsules,
            &mut out,
        );

        assert!(!exit);
        assert_eq!(
            out.notices,
            vec!["no such capsule: demo (see /capsules)".to_string()]
        );
    }

    #[test]
    fn load_capsule_updates_agent_system_prompt() {
        let dir = tempfile::tempdir().unwrap();
        write_capsule(
            dir.path(),
            "demo",
            "demo capsule",
            "Follow the demo workflow.",
        );
        let mut capsules = capsules_from(dir.path());
        let mut agent = test_agent(None);
        let store: Option<Store> = None;
        let mut out = TestRenderer::default();

        let exit = handle_chat_command(
            "/load demo",
            &mut agent,
            test_ctx(&store, Path::new("/repo")),
            &mut capsules,
            &mut out,
        );

        assert!(!exit);
        assert_eq!(out.notices, vec!["loaded demo".to_string()]);
        let prompt = agent.messages[0].text();
        assert!(prompt.contains("## Capsule: demo"));
        assert!(prompt.contains("Follow the demo workflow."));
    }

    #[test]
    fn unload_capsule_reverts_agent_system_prompt() {
        let dir = tempfile::tempdir().unwrap();
        write_capsule(
            dir.path(),
            "demo",
            "demo capsule",
            "Follow the demo workflow.",
        );
        let mut capsules = capsules_from(dir.path());
        let mut agent = test_agent(None);
        let store: Option<Store> = None;
        let mut out = TestRenderer::default();

        handle_chat_command(
            "/load demo",
            &mut agent,
            test_ctx(&store, Path::new("/repo")),
            &mut capsules,
            &mut out,
        );
        let exit = handle_chat_command(
            "/unload demo",
            &mut agent,
            test_ctx(&store, Path::new("/repo")),
            &mut capsules,
            &mut out,
        );

        assert!(!exit);
        assert_eq!(agent.messages[0].text(), "system");
    }

    #[test]
    fn clear_does_not_unload_active_capsules() {
        let dir = tempfile::tempdir().unwrap();
        write_capsule(
            dir.path(),
            "demo",
            "demo capsule",
            "Follow the demo workflow.",
        );
        let mut capsules = capsules_from(dir.path());
        let mut agent = fake_agent("done!");
        let store: Option<Store> = None;
        let mut out = TestRenderer::default();

        handle_chat_command(
            "/load demo",
            &mut agent,
            test_ctx(&store, Path::new("/repo")),
            &mut capsules,
            &mut out,
        );
        handle_chat_command(
            "hello",
            &mut agent,
            test_ctx(&store, Path::new("/repo")),
            &mut capsules,
            &mut out,
        );
        assert_eq!(agent.messages.len(), 3); // system, user, assistant

        let exit = handle_chat_command(
            "/clear",
            &mut agent,
            test_ctx(&store, Path::new("/repo")),
            &mut capsules,
            &mut out,
        );

        assert!(!exit);
        assert!(agent.messages[0].text().contains("## Capsule: demo"));
        assert_eq!(agent.messages.len(), 1);
    }

    #[test]
    fn capsules_command_marks_active_capsule() {
        let dir = tempfile::tempdir().unwrap();
        write_capsule(dir.path(), "demo", "demo capsule", "body");
        let mut capsules = capsules_from(dir.path());
        let mut agent = test_agent(None);
        let store: Option<Store> = None;
        let mut out = TestRenderer::default();

        handle_chat_command(
            "/load demo",
            &mut agent,
            test_ctx(&store, Path::new("/repo")),
            &mut capsules,
            &mut out,
        );
        out.notices.clear();
        handle_chat_command(
            "/capsules",
            &mut agent,
            test_ctx(&store, Path::new("/repo")),
            &mut capsules,
            &mut out,
        );

        assert_eq!(out.notices, vec!["● demo — demo capsule".to_string()]);
    }

    #[test]
    fn bare_capsule_invocation_loads_without_running_a_prompt() {
        let dir = tempfile::tempdir().unwrap();
        write_capsule(dir.path(), "demo", "demo capsule", "body");
        let mut capsules = capsules_from(dir.path());
        let mut agent = test_agent(None);
        let store: Option<Store> = None;
        let mut out = TestRenderer::default();

        let exit = handle_chat_command(
            "/demo",
            &mut agent,
            test_ctx(&store, Path::new("/repo")),
            &mut capsules,
            &mut out,
        );

        assert!(!exit);
        assert!(capsules.is_active("demo"));
        assert_eq!(out.notices, vec!["loaded demo".to_string()]);
        assert_eq!(agent.messages.len(), 1);
    }

    #[test]
    fn unload_not_loaded_capsule_reports_not_loaded() {
        let dir = tempfile::tempdir().unwrap();
        write_capsule(dir.path(), "demo", "demo capsule", "body");
        let mut capsules = capsules_from(dir.path());
        let mut agent = test_agent(None);
        let store: Option<Store> = None;
        let mut out = TestRenderer::default();

        let exit = handle_chat_command(
            "/unload demo",
            &mut agent,
            test_ctx(&store, Path::new("/repo")),
            &mut capsules,
            &mut out,
        );

        assert!(!exit);
        assert_eq!(out.notices, vec!["demo is not loaded".to_string()]);
    }

    #[test]
    fn capsule_session_lifecycle_load_invoke_unload_exit() {
        let dir = tempfile::tempdir().unwrap();
        write_capsule(
            dir.path(),
            "demo",
            "demo capsule",
            "Follow the demo workflow.",
        );
        let mut capsules = capsules_from(dir.path());
        let mut agent = fake_agent("done!");
        let store: Option<Store> = None;
        let mut out = TestRenderer::default();

        let exit = handle_chat_command(
            "/load demo",
            &mut agent,
            test_ctx(&store, Path::new("/repo")),
            &mut capsules,
            &mut out,
        );
        assert!(!exit);
        assert!(agent.messages[0].text().contains("## Capsule: demo"));

        let exit = handle_chat_command(
            "/demo please help",
            &mut agent,
            test_ctx(&store, Path::new("/repo")),
            &mut capsules,
            &mut out,
        );
        assert!(!exit);
        let prompt_after_invoke = agent.messages[0].text();
        assert_eq!(prompt_after_invoke.matches("## Capsule: demo").count(), 1);
        assert_eq!(agent.messages.len(), 3);
        assert_eq!(out.assistant_replies, vec!["done!".to_string()]);

        let exit = handle_chat_command(
            "/unload demo",
            &mut agent,
            test_ctx(&store, Path::new("/repo")),
            &mut capsules,
            &mut out,
        );
        assert!(!exit);
        assert_eq!(agent.messages[0].text(), "system");

        let exit = handle_chat_command(
            "/exit",
            &mut agent,
            test_ctx(&store, Path::new("/repo")),
            &mut capsules,
            &mut out,
        );
        assert!(exit);
    }

    #[test]
    fn capsule_create_rejects_existing_name_without_calling_the_model() {
        let dir = tempfile::tempdir().unwrap();
        write_capsule(dir.path(), "demo", "demo capsule", "body");
        let mut capsules = capsules_from(dir.path());
        // fake_agent's FakeModel would return this reply if called; assert it wasn't.
        let mut agent =
            fake_agent("```\n---\nname: demo\ndescription: x\n---\nshould not run\n```");
        let store: Option<Store> = None;
        let mut out = TestRenderer::default();

        let exit = handle_chat_command(
            "/capsule-create demo does the thing",
            &mut agent,
            test_ctx(&store, dir.path()),
            &mut capsules,
            &mut out,
        );

        assert!(!exit);
        assert_eq!(
            out.notices,
            vec!["capsule demo already exists (see /capsules)".to_string()]
        );
        // no model turn means no user/assistant messages were appended
        assert_eq!(agent.messages.len(), 1);
    }

    #[test]
    fn capsule_create_rejects_missing_arguments() {
        let mut capsules = test_capsules();
        let mut agent = test_agent(None);
        let store: Option<Store> = None;
        let mut out = TestRenderer::default();

        let exit = handle_chat_command(
            "/capsule-create",
            &mut agent,
            test_ctx(&store, Path::new("/repo")),
            &mut capsules,
            &mut out,
        );

        assert!(!exit);
        assert_eq!(
            out.notices,
            vec!["usage: /capsule-create <name> <what it should do>".to_string()]
        );
    }

    #[test]
    fn capsule_create_rejects_reserved_name() {
        let mut capsules = test_capsules();
        let mut agent = test_agent(None);
        let store: Option<Store> = None;
        let mut out = TestRenderer::default();

        let exit = handle_chat_command(
            "/capsule-create load does the thing",
            &mut agent,
            test_ctx(&store, Path::new("/repo")),
            &mut capsules,
            &mut out,
        );

        assert!(!exit);
        assert_eq!(
            out.notices,
            vec!["load is a reserved command name, choose another".to_string()]
        );
    }

    #[test]
    fn capsule_create_rejects_path_traversal_name() {
        let mut capsules = test_capsules();
        let mut agent = test_agent(None);
        let store: Option<Store> = None;
        let mut out = TestRenderer::default();

        let exit = handle_chat_command(
            "/capsule-create ../../evil do the thing",
            &mut agent,
            test_ctx(&store, Path::new("/repo")),
            &mut capsules,
            &mut out,
        );

        assert!(!exit);
        assert_eq!(agent.messages.len(), 1); // no model call was made
        assert!(out.notices[0].contains("not a valid capsule name"));
    }

    #[test]
    fn capsule_create_rejects_absolute_path_name() {
        let mut capsules = test_capsules();
        let mut agent = test_agent(None);
        let store: Option<Store> = None;
        let mut out = TestRenderer::default();

        let exit = handle_chat_command(
            "/capsule-create /tmp/evil do the thing",
            &mut agent,
            test_ctx(&store, Path::new("/repo")),
            &mut capsules,
            &mut out,
        );

        assert!(!exit);
        assert_eq!(agent.messages.len(), 1); // no model call was made
        assert!(out.notices[0].contains("not a valid capsule name"));
    }

    #[test]
    fn capsule_create_drafts_writes_and_loads_a_new_capsule() {
        let dir = tempfile::tempdir().unwrap();
        let mut capsules = capsules_from(dir.path());
        let mut agent = fake_agent(
            "Sure, here's the capsule:\n```\n---\nname: demo\ndescription: demo capsule\n\
             ---\nFollow the demo workflow.\n```\n",
        );
        let store: Option<Store> = None;
        let mut out = TestRenderer::default();

        let exit = handle_chat_command(
            "/capsule-create demo draft a demo workflow",
            &mut agent,
            test_ctx(&store, dir.path()),
            &mut capsules,
            &mut out,
        );

        assert!(!exit);
        assert!(capsules.is_active("demo"));
        let written = std::fs::read_to_string(
            dir.path()
                .join(".zorp")
                .join("capsules")
                .join("demo")
                .join("CAPSULE.md"),
        )
        .unwrap();
        assert!(written.contains("name: demo"));
        assert!(written.contains("Follow the demo workflow."));
        let prompt = agent.messages[0].text();
        assert!(prompt.contains("## Capsule: demo"));
        assert!(prompt.contains("Follow the demo workflow."));
        assert!(out
            .notices
            .iter()
            .any(|n| n.starts_with("created and loaded capsule demo at")));
    }

    #[test]
    fn capsule_create_reconciles_mismatched_model_provided_name() {
        let dir = tempfile::tempdir().unwrap();
        let mut capsules = capsules_from(dir.path());
        let mut agent =
            fake_agent("```\n---\nname: wrong-name\ndescription: demo capsule\n---\nbody\n```");
        let store: Option<Store> = None;
        let mut out = TestRenderer::default();

        handle_chat_command(
            "/capsule-create demo do the thing",
            &mut agent,
            test_ctx(&store, dir.path()),
            &mut capsules,
            &mut out,
        );

        assert!(capsules.is_active("demo"));
        assert!(!capsules.is_active("wrong-name"));
    }

    #[test]
    fn capsule_create_reports_error_when_model_output_has_no_fence() {
        let dir = tempfile::tempdir().unwrap();
        let mut capsules = capsules_from(dir.path());
        let mut agent = fake_agent("sorry, I won't wrap this in a code block");
        let store: Option<Store> = None;
        let mut out = TestRenderer::default();

        handle_chat_command(
            "/capsule-create demo do the thing",
            &mut agent,
            test_ctx(&store, dir.path()),
            &mut capsules,
            &mut out,
        );

        assert!(!capsules.is_active("demo"));
        assert!(out
            .notices
            .iter()
            .any(|n| n == "capsule draft failed: no fenced code block found in model output"));
        assert!(!dir
            .path()
            .join(".zorp")
            .join("capsules")
            .join("demo")
            .exists());
    }

    struct EnvGuard(Vec<(String, Option<String>)>);

    impl EnvGuard {
        fn set(values: &[(&str, Option<&str>)]) -> Self {
            let previous = values
                .iter()
                .map(|(name, _)| ((*name).to_string(), std::env::var(name).ok()))
                .collect();
            for (name, value) in values {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
            Self(previous)
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (name, value) in self.0.drain(..) {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
    }

    #[test]
    fn reasoning_query_reports_off_when_unset() {
        let mut agent = test_agent(None);
        let store = Some(Store::open_in_memory().unwrap());
        store
            .as_ref()
            .unwrap()
            .create_session_with_reasoning_mode("s1", "chat", "/repo", "test-model", None)
            .unwrap();
        let mut out = TestRenderer::default();

        let exit = handle_chat_command(
            "/reasoning",
            &mut agent,
            test_ctx(&store, Path::new("/repo")),
            &mut test_capsules(),
            &mut out,
        );

        assert!(!exit);
        assert_eq!(out.notices, vec!["reasoning: off".to_string()]);
    }

    #[test]
    fn reasoning_set_updates_agent_and_store() {
        let mut agent = test_agent(None);
        let store = Some(Store::open_in_memory().unwrap());
        store
            .as_ref()
            .unwrap()
            .create_session_with_reasoning_mode("s1", "chat", "/repo", "test-model", None)
            .unwrap();
        let mut out = TestRenderer::default();

        let exit = handle_chat_command(
            "/reasoning high",
            &mut agent,
            test_ctx(&store, Path::new("/repo")),
            &mut test_capsules(),
            &mut out,
        );

        assert!(!exit);
        assert_eq!(agent.session_reasoning_mode(), Some(ReasoningMode::High));
        assert_eq!(
            store
                .as_ref()
                .unwrap()
                .session_reasoning_mode("s1")
                .unwrap(),
            Some(ReasoningMode::High)
        );
        assert_eq!(out.notices, vec!["reasoning set to high".to_string()]);
    }

    #[test]
    fn chat_reasoning_updates_can_be_cleared_with_off() {
        let mut agent = test_agent(Some(ReasoningMode::Medium));
        let store = Some(Store::open_in_memory().unwrap());
        store
            .as_ref()
            .unwrap()
            .create_session_with_reasoning_mode(
                "s1",
                "chat",
                "/repo",
                "test-model",
                Some(ReasoningMode::Medium),
            )
            .unwrap();
        let mut out = TestRenderer::default();

        handle_chat_command(
            "/reasoning off",
            &mut agent,
            test_ctx(&store, Path::new("/repo")),
            &mut test_capsules(),
            &mut out,
        );

        assert_eq!(agent.session_reasoning_mode(), None);
        assert_eq!(
            store
                .as_ref()
                .unwrap()
                .session_reasoning_mode("s1")
                .unwrap(),
            None
        );
        assert_eq!(out.notices, vec!["reasoning turned off".to_string()]);
    }

    #[test]
    fn resume_prefers_persisted_session_reasoning_mode() {
        let _env = EnvGuard::set(&[
            ("ZORP_REASONING_MODE", None),
            ("ZORP_BASE_URL", Some("http://localhost:1234/v1")),
            ("ZORP_MODEL", Some("reasoning-model")),
        ]);
        let store = Store::open_in_memory().unwrap();
        store
            .create_session_with_reasoning_mode(
                "s1",
                "chat",
                "/repo",
                "reasoning-model",
                Some(ReasoningMode::High),
            )
            .unwrap();

        let persisted = store.session_reasoning_mode("s1").unwrap();
        let model = HttpModel::from_env()
            .try_with_env_reasoning_mode(persisted)
            .unwrap();

        assert_eq!(model.session_reasoning_mode(), Some(ReasoningMode::High));
    }

    #[test]
    fn one_shot_run_does_not_depend_on_session_reasoning_state() {
        let _env = EnvGuard::set(&[
            ("ZORP_REASONING_MODE", None),
            ("ZORP_BASE_URL", Some("http://localhost:1234/v1")),
            ("ZORP_MODEL", Some("reasoning-model")),
        ]);

        let model = HttpModel::from_env()
            .try_with_env_reasoning_mode(None)
            .unwrap();

        assert_eq!(model.session_reasoning_mode(), None);
    }

    #[test]
    #[cfg(feature = "otel")]
    fn test_otel_initialization() {
        // init_otel might return None if a global default subscriber has already been set,
        // but we want to check that if we call it, it doesn't panic.
        let _guard = super::otel_init::init_otel();
    }

    #[test]
    fn test_mime_from_extension() {
        use std::path::Path;
        assert_eq!(
            super::mime_from_extension(Path::new("test.png")),
            "image/png"
        );
        assert_eq!(
            super::mime_from_extension(Path::new("test.jpg")),
            "image/jpeg"
        );
        assert_eq!(
            super::mime_from_extension(Path::new("test.jpeg")),
            "image/jpeg"
        );
        assert_eq!(
            super::mime_from_extension(Path::new("test.gif")),
            "image/gif"
        );
        assert_eq!(
            super::mime_from_extension(Path::new("test.webp")),
            "image/webp"
        );
        assert_eq!(
            super::mime_from_extension(Path::new("test.unknown")),
            "image/png"
        );
    }

    #[test]
    fn test_is_image_extension() {
        use std::path::Path;
        assert!(super::is_image_extension(Path::new("test.png")));
        assert!(super::is_image_extension(Path::new("test.jpg")));
        assert!(super::is_image_extension(Path::new("test.jpeg")));
        assert!(super::is_image_extension(Path::new("test.gif")));
        assert!(super::is_image_extension(Path::new("test.webp")));
        assert!(!super::is_image_extension(Path::new("test.txt")));
        assert!(!super::is_image_extension(Path::new("test")));
    }

    #[test]
    fn test_extract_image_refs() {
        let dir = tempfile::tempdir().unwrap();
        let path1 = dir.path().join("img1.png");
        let path2 = dir.path().join("img2.jpg");
        std::fs::write(&path1, b"fake png").unwrap();
        std::fs::write(&path2, b"fake jpg").unwrap();

        let text = "Look at @image img1.png and @img img2.jpg or @image missing.png end";
        let (cleaned, images) = super::extract_image_refs(text, dir.path());

        assert_eq!(
            cleaned,
            "Look at [Image 1] and [Image 2] or @image missing.png end"
        );
        assert_eq!(images.len(), 2);
        assert_eq!(images[0].0, b"fake png");
        assert_eq!(images[0].1, "image/png");
        assert_eq!(images[1].0, b"fake jpg");
        assert_eq!(images[1].1, "image/jpeg");
    }

    #[test]
    fn test_segments_to_parts() {
        use super::Segment;
        use zorp_agent::ContentPart;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ref.png");
        std::fs::write(&path, b"ref_data").unwrap();

        let segments = vec![
            Segment::Text("Hello ".into()),
            Segment::Paste("world. ".into()),
            Segment::Image {
                data: b"img_data".to_vec(),
                mime_type: "image/png".into(),
                index: 1,
            },
            Segment::Text("See @image ref.png".into()),
        ];

        let parts = super::segments_to_parts(&segments, dir.path());
        assert_eq!(parts.len(), 4);

        match &parts[0] {
            ContentPart::Text(t) => assert_eq!(t, "Hello world. "),
            _ => panic!("Expected text part"),
        }
        match &parts[1] {
            ContentPart::Image { data, mime_type } => {
                assert_eq!(data, b"img_data");
                assert_eq!(mime_type, "image/png");
            }
            _ => panic!("Expected image part"),
        }
        match &parts[2] {
            ContentPart::Text(t) => assert_eq!(t, "See [Image 1]"),
            _ => panic!("Expected text part"),
        }
        match &parts[3] {
            ContentPart::Image { data, mime_type } => {
                assert_eq!(data, b"ref_data");
                assert_eq!(mime_type, "image/png");
            }
            _ => panic!("Expected image part"),
        }
    }

    #[cfg(feature = "research")]
    #[test]
    fn get_or_create_track_creates_a_new_track_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        let project = zorp_track::Project::open(dir.path()).unwrap();
        let track_id = zorp_track::id::track_id("does caching help");

        get_or_create_track(&project.store, &track_id, "does caching help").unwrap();

        let track = project.store.get_track(&track_id).unwrap();
        assert_eq!(track.hypothesis, "does caching help");
    }

    #[cfg(feature = "research")]
    #[test]
    fn get_or_create_track_reuses_the_existing_track_on_retry_of_the_same_question() {
        let dir = tempfile::tempdir().unwrap();
        let project = zorp_track::Project::open(dir.path()).unwrap();
        let track_id = zorp_track::id::track_id("does caching help");
        project
            .store
            .create_track(&track_id, "does caching help")
            .unwrap();

        // A retry of the same question must succeed by reusing the row,
        // not fail with a duplicate primary-key error.
        get_or_create_track(&project.store, &track_id, "does caching help").unwrap();
    }

    #[cfg(feature = "research")]
    #[test]
    fn get_or_create_track_errors_instead_of_silently_reusing_a_colliding_track() {
        let dir = tempfile::tempdir().unwrap();
        let project = zorp_track::Project::open(dir.path()).unwrap();
        // Pre-seed a track under an id that a genuinely different question
        // will collide onto (simulating a slug collision directly, rather
        // than searching for two real strings that hash to the same id).
        let track_id = "shared-id";
        project.store.create_track(track_id, "question A").unwrap();

        let err = get_or_create_track(&project.store, track_id, "question B").unwrap_err();
        assert!(
            err.contains("different question"),
            "expected a collision error, got: {err}"
        );

        // The original track's hypothesis must be left untouched, not
        // silently overwritten or reused for question B.
        let track = project.store.get_track(track_id).unwrap();
        assert_eq!(track.hypothesis, "question A");
    }
}
