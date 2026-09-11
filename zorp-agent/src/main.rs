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
    /// Look at earlier conversations before answering this one.
    ///
    /// Per message and off by default, the same as the browser's tick box
    /// and for the same reason: it spends context and it puts text from
    /// old conversations, tool results and fetched pages included, in front
    /// of the model. A thing to choose each time, not a mode to leave on.
    #[cfg(feature = "memory")]
    #[arg(long, global = true)]
    recall: bool,
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

#[derive(Default)]
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
enum ConfigAction {
    /// Set a saved value. Use `unset` to remove one.
    Set {
        /// One of: model, base-url, provider, max-tokens.
        key: String,
        value: String,
    },
    /// Remove a saved value, so the chain falls through to the default.
    Unset {
        /// One of: model, base-url, provider, max-tokens.
        key: String,
    },
    /// Print the path to the saved settings file.
    Path,
}

#[derive(Subcommand)]
enum ProjectAction {
    /// Make a project.
    New { name: String },
    /// Remove a project. **This deletes no conversation.**
    Rm { id: String },
}

#[derive(Subcommand)]
enum Command {
    /// Start an interactive chat session.
    Chat,
    /// Continue a previous session. With no id, the most recent one.
    ///
    /// An id may be given as a unique prefix, the way git takes a short
    /// sha. An ambiguous prefix lists the candidates rather than guessing.
    Resume { id: Option<String> },
    /// List the conversations in the store, newest first.
    ///
    /// The browser and the terminal share one store, so this shows both.
    Sessions {
        /// How many to show. Defaults to 20.
        #[arg(long)]
        limit: Option<usize>,
        /// Show every conversation, however many there are.
        #[arg(long)]
        all: bool,
        /// Only conversations filed under this project, by id or name.
        #[arg(long)]
        project: Option<String>,
    },
    /// Revert the most recent recorded file change.
    Undo,
    /// Print a summary of the latest session's file changes.
    Diff,
    /// Scaffold a new flavor manifest at ./.zorp/flavors/<name>.toml.
    New { name: String },
    /// Search your own conversations, by meaning rather than by spelling.
    ///
    /// The index is over the store both surfaces share, so this finds
    /// conversations you had in the browser too. Every vector is made by a
    /// model on this machine and nothing typed here is sent anywhere else.
    #[cfg(feature = "recall")]
    Recall {
        /// What to look for. Omit with --index to only bring the index up
        /// to date.
        query: Vec<String>,
        /// How many results.
        #[arg(long)]
        limit: Option<usize>,
        /// Only conversations filed under this project.
        #[arg(long)]
        project: Option<String>,
        /// Bring the index up to date first.
        #[arg(long)]
        index: bool,
    },
    /// Review one piece of material with several reviewers at once.
    ///
    /// Each reads it from a different code-defined angle, none sees what
    /// the others said, and the agreement between them is counted in code
    /// afterwards. Reviewers get a read-only tool set and cannot change
    /// what they are reviewing.
    Panel {
        /// The file to review. With none, reads stdin.
        path: Option<PathBuf>,
        /// A short name for the material, carried into the report.
        #[arg(long)]
        label: Option<String>,
        /// Which lenses to run, by name. Repeatable. Defaults to all.
        #[arg(long = "lens")]
        lenses: Vec<String>,
        /// List the lenses and exit.
        #[arg(long)]
        list_lenses: bool,
        /// Print each reviewer's whole answer rather than a preview.
        #[arg(long)]
        full: bool,
    },
    /// Show the effective configuration and where each value came from, or
    /// change what is saved.
    ///
    /// The saved file is shared with the browser, so configuring zorp is
    /// one job rather than two. The API key is never written to it.
    Config {
        #[command(subcommand)]
        action: Option<ConfigAction>,
    },
    /// Group conversations. A project is a label and nothing more.
    ///
    /// Nothing about a conversation changes when it joins one, and
    /// removing a project deletes no conversation.
    Projects {
        #[command(subcommand)]
        action: Option<ProjectAction>,
    },
    /// Say what this build can do and whether it can reach anything.
    ///
    /// Exits non-zero when something it checked came back bad, so it is
    /// usable in a script and safe to paste into a bug report: no key, no
    /// token and no manifest contents are printed.
    Doctor,
    /// Delete a conversation and everything recorded under it.
    ///
    /// Takes a unique id prefix. This removes messages and recorded file
    /// changes, so it asks first unless --yes is passed.
    Rm {
        id: String,
        /// Delete without asking.
        #[arg(long)]
        yes: bool,
        /// Delete even when the stored status says a turn is running.
        #[arg(long)]
        force: bool,
    },
    /// Copy a conversation up to one of its answers into a new one.
    ///
    /// Prints the new id, so the next thing to type is `resume <new id>`.
    Branch {
        id: String,
        /// Which answer to branch at, counted from one. Defaults to the
        /// most recent, which is what a terminal can see.
        #[arg(long)]
        answer: Option<usize>,
        /// Branch even when the stored status says a turn is running.
        #[arg(long)]
        force: bool,
    },
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
    /// Audit a co-written draft against the track's evidence record and
    /// revise what the record does not support.
    #[cfg(feature = "research")]
    Critique {
        question: String,
        /// Most revision rounds to run. The audit always runs once, so 0
        /// reports what is wrong and leaves the draft alone. Falls back
        /// to ZORP_CRITIQUE_ROUNDS, then to 2.
        #[arg(long = "critique-rounds")]
        critique_rounds: Option<usize>,
    },
    /// Match a co-written draft against real venues.
    #[cfg(feature = "research")]
    Deliver { question: String },
    /// Run the instruction with a main model, have reviewer models test it,
    /// and send corroborated findings back for revision. Roles come from
    /// the TOML file named by ZORP_ENSEMBLE.
    #[cfg(feature = "ensemble")]
    Ensemble {
        /// The task, as a plain run takes it.
        instruction: String,
    },
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
        Some(Command::Resume { id }) => resume(id.as_deref(), cli.yes, cli.no_verify, &overrides),
        Some(Command::Sessions {
            limit,
            all,
            project,
        }) => list_sessions(limit, all, project.as_deref()),
        Some(Command::Undo) => undo(),
        Some(Command::Diff) => diff(),
        Some(Command::New { name }) => scaffold(&name),
        #[cfg(feature = "recall")]
        Some(Command::Recall {
            query,
            limit,
            project,
            index,
        }) => recall_command(&query.join(" "), limit, project.as_deref(), index),
        Some(Command::Panel {
            path,
            label,
            lenses,
            list_lenses,
            full,
        }) => panel_command(path, label, lenses, list_lenses, full, &overrides),
        Some(Command::Config { action }) => config(action, &overrides),
        Some(Command::Projects { action }) => projects(action),
        Some(Command::Doctor) => doctor(&overrides),
        Some(Command::Rm { id, yes, force }) => remove_session(&id, yes || cli.yes, force),
        Some(Command::Branch { id, answer, force }) => branch_session(&id, answer, force),
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
        Some(Command::Critique {
            question,
            critique_rounds,
        }) => critique(&question, critique_rounds, cli.yes, &overrides),
        #[cfg(feature = "research")]
        Some(Command::Deliver { question }) => deliver(&question, cli.yes, &overrides),
        #[cfg(feature = "ensemble")]
        Some(Command::Ensemble { instruction }) => {
            ensemble(&instruction, cli.yes, cli.no_verify, &overrides)
        }
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
            #[cfg(feature = "memory")]
            let use_recall = cli.recall;
            #[cfg(not(feature = "memory"))]
            let use_recall = false;
            run(
                cli.task.join(" "),
                &cli.images,
                cli.yes,
                cli.no_verify,
                use_recall,
                &overrides,
            );
        }
    }
}

const SCAFFOLD_TEMPLATE: &str = r#"name = "{name}"

# All keys are optional; omitted keys inherit from the layer below.
# api_key is NEVER read from a manifest. Set ZORP_API_KEY in the environment.
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

// One definition, in `line_editor`, because the editor moves a cursor
// through these and `segments_to_parts` turns them into a message. Two
// would drift.
use zorp_agent::line_editor::Segment;

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

/// One recall hit, as a line, plus the id under it.
///
/// The id goes on its own line rather than in a column, because it is what
/// the next command takes and a column of ids is the thing people copy
/// wrongly.
#[cfg(feature = "recall")]
fn recall_lines(
    hits: &[zorp_recall::Hit],
    names: &std::collections::HashMap<String, String>,
) -> Vec<String> {
    let mut out = Vec::new();
    for hit in hits {
        let name = names
            .get(&hit.conversation_id)
            .cloned()
            .unwrap_or_else(|| hit.title.clone());
        // The role is load bearing. An assistant line is a model's earlier
        // output and is not a checked fact, and a list that drew the two
        // the same way would be saying they are the same kind of thing.
        let who = zorp_agent::recall::attribution(hit.role == "user");
        out.push(name);
        out.push(format!(
            "  {}  message {}, written by {who}",
            zorp_agent::sessions::short(&hit.conversation_id),
            hit.seq
        ));
        out.push(format!("  {}", one_line(&hit.snippet)));
        out.push(String::new());
    }
    out
}

/// A snippet on one line, with the invisible characters gone.
///
/// A stored message can be a pasted file or a page the agent fetched, so a
/// bidirectional override in one would reorder every line drawn after it.
#[cfg(feature = "recall")]
fn one_line(text: &str) -> String {
    let flat = zorp_agent::sessions::scrub(text);
    if flat.chars().count() <= 160 {
        return flat;
    }
    format!("{}...", flat.chars().take(157).collect::<String>())
}

/// `zorp-agent recall`.
///
/// A person reading their own history. The model gets nothing from this
/// command: that is `--recall` on a turn, which is a different feature and
/// a different risk.
#[cfg(feature = "recall")]
fn recall_command(query: &str, limit: Option<usize>, project: Option<&str>, index: bool) {
    if index {
        eprintln!("zorp-agent: bringing the index up to date...");
        match zorp_agent::recall::reindex() {
            Ok(report) => eprintln!(
                "zorp-agent: indexed {}, skipped {}, removed {}",
                report.indexed, report.skipped, report.removed
            ),
            Err(e) => {
                // The error already names the missing local embedder and
                // says nothing was sent anywhere, so it is passed through
                // whole rather than summarized.
                eprintln!("zorp-agent: {e}");
                std::process::exit(1);
            }
        }
        if query.trim().is_empty() {
            return;
        }
    }
    if query.trim().is_empty() {
        eprintln!("zorp-agent: nothing to search for. Pass a query, or --index on its own.");
        std::process::exit(2);
    }

    let store = open_store();
    let wanted = match (project, &store) {
        (None, _) => None,
        (Some(wanted), Some(store)) => {
            let known = store.projects().unwrap_or_default();
            match known.iter().find(|p| p.id == wanted || p.name == wanted) {
                Some(row) => Some(row.id.clone()),
                None => {
                    eprintln!("zorp-agent: no project matching '{wanted}'");
                    std::process::exit(1);
                }
            }
        }
        (Some(_), None) => std::process::exit(1),
    };

    let hits = match zorp_agent::recall::search(
        query,
        limit.unwrap_or(zorp_agent::recall::DEFAULT_LIMIT),
        wanted.as_deref(),
    ) {
        Ok(hits) => hits,
        Err(e) => {
            // A CLI that cannot reach the local embedder says so and
            // searches nothing. It does not fall back to anything, and
            // there is nothing to fall back to.
            eprintln!("zorp-agent: {e}");
            std::process::exit(1);
        }
    };

    if hits.is_empty() {
        println!("Nothing matched. `zorp-agent recall --index` brings the index up to date.");
        return;
    }

    // The display title when something wrote one, and the verbatim first
    // message otherwise, which is what the index carries as a title anyway.
    let names: std::collections::HashMap<String, String> = store
        .as_ref()
        .and_then(|s| s.sessions().ok())
        .unwrap_or_default()
        .into_iter()
        .map(|row| (row.id.clone(), zorp_agent::sessions::name(&row)))
        .collect();

    for line in recall_lines(&hits, &names) {
        println!("{line}");
    }
    println!("Continue one with `zorp-agent resume <id>`.");
}

/// Queue this conversation on the index, the way a finished browser turn
/// does, so working in the terminal keeps the index warm rather than
/// leaving it to the next time somebody opens the browser.
///
/// On its own thread and best effort: a missing local embedder must not
/// slow or fail a turn that already answered.
#[cfg(feature = "recall")]
fn feed_recall(session_id: &str) {
    let session_id = session_id.to_string();
    std::thread::spawn(move || {
        let _ = zorp_agent::recall::feed_session(&session_id);
    });
}

#[cfg(not(feature = "recall"))]
fn feed_recall(_session_id: &str) {}

/// Look up what earlier conversations said about this message, tell the
/// person what came back, and hand the text to the run.
///
/// Everything about the result reaches the terminal first, including the
/// case where nothing was found and the case where no local embedder
/// answered. A recall nobody can see is a model that knows things for
/// reasons nobody can check.
///
/// An assistant line is labelled as a model's earlier output, everywhere it
/// surfaces, including here. It is a thing that was said, not a thing that
/// was checked.
#[cfg(feature = "memory")]
fn recall_into_turn(use_recall: bool, message: &str, out: &mut dyn Renderer) -> Option<String> {
    if !use_recall {
        return None;
    }
    match zorp_agent::memory::recall_for(message, zorp_agent::memory::DEFAULT_PASSAGES, None) {
        Err(e) => {
            // Not an error card, and not a silent fall through either. The
            // turn goes ahead without memory, because refusing to answer
            // over a search index being down is the wrong trade, and the
            // person is told in the library's own words, which already name
            // the missing local embedder and say nothing was sent anywhere.
            out.notice(&format!("memory was asked for and could not be used: {e}"));
            None
        }
        Ok(found) => {
            if found.citations.is_empty() {
                out.notice("memory found nothing relevant in earlier conversations");
                return None;
            }
            out.notice(&format!(
                "recalled {} line{} from earlier conversations:",
                found.citations.len(),
                if found.citations.len() == 1 { "" } else { "s" }
            ));
            for citation in &found.citations {
                // The same four provenance fields the model is shown, so
                // what a person can inspect is what the model was given.
                let author = zorp_agent::recall::attribution(citation.author == "you");
                out.notice(&format!(
                    "  {}  {}  message {}, written by {author}",
                    zorp_agent::sessions::short(&citation.conversation_id),
                    zorp_agent::sessions::scrub(&citation.title),
                    citation.seq
                ));
            }
            found.block
        }
    }
}

#[cfg(not(feature = "memory"))]
fn recall_into_turn(_use_recall: bool, _message: &str, _out: &mut dyn Renderer) -> Option<String> {
    None
}

/// The lenses somebody asked for, or all of them.
///
/// An unrecognized name falls back to the whole panel rather than to an
/// empty one, because a panel of nobody is five confident answers about
/// nothing that still cost five requests. The names that were not
/// recognized are reported so a typo is visible.
fn resolve_lenses(requested: &[String], out: &mut dyn Renderer) -> Vec<zorp_agent::Lens> {
    let all = zorp_agent::default_lenses();
    if requested.is_empty() {
        return all;
    }
    let unknown: Vec<&String> = requested
        .iter()
        .filter(|r| !all.iter().any(|l| &l.name == *r))
        .collect();
    for name in &unknown {
        out.notice(&format!("no lens called '{name}'"));
    }
    let chosen: Vec<zorp_agent::Lens> = all
        .iter()
        .filter(|l| requested.iter().any(|r| r == &l.name))
        .cloned()
        .collect();
    if chosen.is_empty() {
        out.notice("running the whole panel instead");
        return all;
    }
    chosen
}

/// Tell the terminal which reviewers have started and finished.
///
/// A panel takes a while and its whole point is that several things are
/// happening at once, so a caller that can only see the finished report has
/// nothing to show for the first minute. Called from several reviewer
/// threads, hence the mutex around the renderer.
struct TerminalPanelObserver(std::sync::Mutex<Box<dyn Renderer>>);

impl zorp_agent::PanelObserver for TerminalPanelObserver {
    fn reviewer_started(&self, lens: &str) {
        if let Ok(mut out) = self.0.lock() {
            out.notice(&format!("  {lens}: reading"));
        }
    }

    fn reviewer_finished(&self, verdict: &zorp_agent::ReviewerVerdict) {
        if let Ok(mut out) = self.0.lock() {
            out.notice(&format!(
                "  {}: {} finding{}",
                verdict.lens,
                verdict.findings.len(),
                if verdict.findings.len() == 1 { "" } else { "s" }
            ));
        }
    }

    fn reviewer_failed(&self, lens: &str, why: &str) {
        if let Ok(mut out) = self.0.lock() {
            out.notice(&format!("  {lens}: failed, {why}"));
        }
    }
}

/// `zorp-agent panel`.
///
/// A person types this, which is the same bound the browser's button has:
/// one launch, one panel, a fixed number of reviewers, none of which has a
/// panel of its own. There is no tool that reaches this and there must
/// never be one.
fn panel_command(
    path: Option<PathBuf>,
    label: Option<String>,
    lenses: Vec<String>,
    list_lenses: bool,
    full: bool,
    overrides: &Overrides,
) {
    let color = std::io::stderr().is_terminal();
    let mut out = LineRenderer::new(std::io::stderr(), color);

    if list_lenses {
        for lens in zorp_agent::default_lenses() {
            println!("{}: {}", lens.name, lens.instruction);
        }
        return;
    }

    let (label, body) = match &path {
        Some(path) => {
            let body = std::fs::read_to_string(path).unwrap_or_else(|e| {
                eprintln!("zorp-agent: cannot read {}: {e}", path.display());
                std::process::exit(2);
            });
            (label.unwrap_or_else(|| path.display().to_string()), body)
        }
        None => {
            let mut body = String::new();
            use std::io::Read as _;
            if std::io::stdin().read_to_string(&mut body).is_err() {
                eprintln!("zorp-agent: could not read stdin");
                std::process::exit(2);
            }
            (label.unwrap_or_else(|| "stdin".to_string()), body)
        }
    };

    // Five reviewers asked to review nothing produce five confident answers
    // about nothing, which costs five requests and reads exactly like a
    // real panel.
    if body.trim().is_empty() {
        eprintln!("zorp-agent: nothing to review: the material is empty");
        std::process::exit(2);
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let (user_flavor, project_flavor) = resolve_flavor(overrides);
    let merged = user_flavor.merge(project_flavor);
    let (base_url, model_name) = resolve_host_and_model(overrides, &merged);
    let provider = resolve_provider(overrides, &merged).unwrap_or_else(|e| {
        eprintln!("zorp-agent: {e}");
        std::process::exit(2);
    });
    if model_name.is_empty() {
        eprintln!("zorp-agent: no model set. Use --model, ZORP_MODEL, or a flavor.");
        std::process::exit(2);
    }
    let model = HttpModel {
        url: zorp_agent::join_url(&base_url, provider.path_suffix()),
        api_key: std::env::var("ZORP_API_KEY").ok().filter(|k| !k.is_empty()),
        model: model_name,
        provider,
        max_tokens: resolve_max_tokens(overrides, &merged),
    }
    .try_with_env_reasoning_mode(None)
    .unwrap_or_else(|e| {
        eprintln!("zorp-agent: {e}");
        std::process::exit(2);
    });

    let config = zorp_agent::PanelConfig {
        lenses: resolve_lenses(&lenses, &mut out),
        ..zorp_agent::PanelConfig::default()
    };
    let target = zorp_agent::Target {
        label: label.clone(),
        body,
    };
    out.notice(&format!(
        "panel on {label}: {} reviewers",
        config.lenses.len()
    ));

    let cancel = install_cancel();
    let observer = TerminalPanelObserver(std::sync::Mutex::new(Box::new(LineRenderer::new(
        std::io::stderr(),
        color,
    ))));
    // `AutoApprove` is not a loosening here. Reviewers get a read-only tool
    // set and no tool in it is approval gated, so there is nothing for a
    // human to approve and nothing for a prompt to park on. The allow list
    // in `zorp_agent::panel` is what does the work, and the caller's own
    // `--yes` never reaches a reviewer: this is a fixed value, not the
    // approval mode this command was invoked with.
    let report = zorp_agent::panel::run(
        &model,
        &target,
        &config,
        cwd,
        cancel,
        ApprovalMode::AutoApprove,
        &observer,
    );

    for line in zorp_agent::panel::report_lines(&report, full) {
        println!("{line}");
    }
    // A panel that could not finish is not a panel whose numbers mean what
    // they look like, so it is worth an exit code too.
    if !report.is_complete() {
        std::process::exit(1);
    }
}

/// `zorp-agent config`, and its three actions.
fn config(action: Option<ConfigAction>, overrides: &Overrides) {
    match action {
        None => print_config(overrides),
        Some(ConfigAction::Path) => println!("{}", zorp_agent::config::path().display()),
        Some(ConfigAction::Set { key, value }) => config_write(&key, Some(&value)),
        Some(ConfigAction::Unset { key }) => config_write(&key, None),
    }
}

/// Print the effective configuration and, for each value, where it came
/// from.
///
/// The provenance is the useful half. A person debugging why they are
/// talking to the wrong model can already see the value: it is in the
/// wrong answers they are getting. What they cannot see is which of the
/// flag, the variable, the flavor and the file won.
fn print_config(overrides: &Overrides) {
    use zorp_agent::config;

    let saved = config::load().unwrap_or_default();
    let (_, project_flavor) = resolve_flavor(overrides);
    let (user_flavor, _) = resolve_flavor(overrides);
    let merged = user_flavor.merge(project_flavor);

    let base_url = config::resolve(
        overrides.base_url.as_deref(),
        "ZORP_BASE_URL",
        merged.base_url.as_deref(),
        saved.base_url.as_deref(),
        "http://localhost:11434/v1",
    );
    let model = config::resolve(
        overrides.model.as_deref(),
        "ZORP_MODEL",
        merged.model.as_deref(),
        saved.model.as_deref(),
        "(not set)",
    );
    let provider = config::resolve(
        overrides.provider.as_deref(),
        "ZORP_PROVIDER",
        merged.provider.map(|p| p.name()),
        saved.provider.as_deref(),
        "openai",
    );
    let max_tokens = config::resolve(
        overrides.max_tokens.map(|v| v.to_string()).as_deref(),
        "ZORP_MAX_TOKENS",
        merged.max_tokens.map(|v| v.to_string()).as_deref(),
        saved.max_tokens.map(|v| v.to_string()).as_deref(),
        "(provider default)",
    );

    let rows = [
        ("model", &model),
        ("base url", &base_url),
        ("provider", &provider),
        ("max tokens", &max_tokens),
    ];
    let width = rows.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
    // The values are padded too, so the provenance starts in one column.
    // That is the column somebody is reading down: the values differ by
    // definition, and a ragged "(from ...)" is what makes four of them hard
    // to compare.
    let value_width = rows.iter().map(|(_, r)| r.value.len()).max().unwrap_or(0);
    for (key, resolved) in rows {
        println!(
            "{key:<width$}  {:<value_width$}  (from {})",
            resolved.value,
            resolved.source.describe()
        );
    }

    // Said and never shown. The key is the one thing that is not in the
    // file and must not be, and a person still needs to know whether one
    // is set.
    println!(
        "{:<width$}  {}",
        "api key",
        if std::env::var("ZORP_API_KEY")
            .map(|k| !k.trim().is_empty())
            .unwrap_or(false)
        {
            "set in $ZORP_API_KEY, and never written to the file"
        } else {
            "not set"
        }
    );
    println!("\nsaved settings: {}", zorp_agent::config::path().display());
}

/// Write one saved value, or remove it.
///
/// The keys are the four sharable settings and nothing else. There is
/// deliberately no way to write an API key here: it is not on `Saved`, so
/// there is nothing for a key to be written into.
fn config_write(key: &str, value: Option<&str>) {
    let mut saved = zorp_agent::config::load().unwrap_or_default();
    let normalized = key.trim().to_ascii_lowercase().replace('_', "-");
    match normalized.as_str() {
        "model" => saved.model = value.map(str::to_string),
        "base-url" | "baseurl" | "url" => saved.base_url = value.map(str::to_string),
        "provider" => {
            if let Some(v) = value {
                // Parsed before it is written, so an unusable value is a
                // refusal now rather than a confusing failure on the next
                // run.
                if v.parse::<Provider>().is_err() {
                    eprintln!("zorp-agent: unknown provider '{v}'. Use openai or anthropic.");
                    std::process::exit(2);
                }
            }
            saved.provider = value.map(str::to_string);
        }
        "max-tokens" | "maxtokens" => match value {
            Some(v) => match v.parse::<u32>() {
                Ok(n) => saved.max_tokens = Some(n),
                Err(_) => {
                    eprintln!("zorp-agent: max-tokens must be a number, got '{v}'");
                    std::process::exit(2);
                }
            },
            None => saved.max_tokens = None,
        },
        "api-key" | "apikey" | "key" => {
            eprintln!(
                "zorp-agent: the API key is never written to a file. Set $ZORP_API_KEY instead."
            );
            std::process::exit(2);
        }
        other => {
            eprintln!(
                "zorp-agent: unknown setting '{other}'. One of: model, base-url, provider, \
                 max-tokens."
            );
            std::process::exit(2);
        }
    }
    match zorp_agent::config::save(&saved) {
        Ok(()) => {
            let path = zorp_agent::config::path();
            match value {
                Some(v) => println!("{normalized} = {v}  ({})", path.display()),
                None => println!("{normalized} unset  ({})", path.display()),
            }
        }
        Err(e) => {
            eprintln!(
                "zorp-agent: could not write {}: {e}",
                zorp_agent::config::path().display()
            );
            std::process::exit(1);
        }
    }
}

/// The project somebody meant, by id, id prefix, or exact name.
///
/// A name as well as an id because a person reading a listing has the name
/// in front of them and would otherwise have to go and copy an id. An id
/// wins over a name, and an exact name wins over a prefix, so nothing here
/// can be shadowed by a project somebody later names after an id.
fn resolve_project<'a>(
    projects: &'a [zorp_agent::ProjectRow],
    wanted: &str,
) -> Option<&'a zorp_agent::ProjectRow> {
    if let Some(row) = projects.iter().find(|p| p.id == wanted) {
        return Some(row);
    }
    if let Some(row) = projects.iter().find(|p| p.name == wanted) {
        return Some(row);
    }
    let prefixed: Vec<&zorp_agent::ProjectRow> = projects
        .iter()
        .filter(|p| p.id.starts_with(wanted))
        .collect();
    if prefixed.len() == 1 {
        return Some(prefixed[0]);
    }
    // Case insensitive name, last, so `kitchen` finds `Kitchen rebuild`
    // only when nothing more exact did.
    let lowered = wanted.to_lowercase();
    let named: Vec<&zorp_agent::ProjectRow> = projects
        .iter()
        .filter(|p| p.name.to_lowercase() == lowered)
        .collect();
    if named.len() == 1 {
        return Some(named[0]);
    }
    None
}

/// `zorp-agent projects`.
fn projects(action: Option<ProjectAction>) {
    let mut store = match open_store() {
        Some(s) => s,
        None => std::process::exit(1),
    };
    match action {
        None => {
            let rows = store.projects().unwrap_or_default();
            if rows.is_empty() {
                println!("No projects yet. Run `zorp-agent projects new \"<name>\"` to make one.");
                return;
            }
            let width = rows
                .iter()
                .map(|p| zorp_agent::sessions::short(&p.id).len())
                .max()
                .unwrap_or(8);
            for row in rows {
                let count = store
                    .sessions_in_project(&row.id)
                    .map(|ids| ids.len())
                    .unwrap_or(0);
                println!(
                    "{:<width$}  {:>3}  {}",
                    zorp_agent::sessions::short(&row.id),
                    count,
                    row.name
                );
            }
        }
        Some(ProjectAction::New { name }) => {
            // The same rules the browser applies, from the same function,
            // rather than a second copy: control characters and the
            // bidirectional overrides go, whitespace runs collapse, and an
            // empty result is a refusal. An override in a listing reorders
            // every row drawn after it.
            let name = zorp_agent::title::scrub(&name);
            if name.is_empty() {
                eprintln!("zorp-agent: a project needs a name");
                std::process::exit(2);
            }
            let limit = zorp_agent::MAX_PROJECT_NAME;
            if name.chars().count() > limit {
                eprintln!("zorp-agent: a project name is at most {limit} characters");
                std::process::exit(2);
            }
            let id = zorp_agent::new_session_id();
            match store.create_project(&id, &name) {
                Ok(_) => println!("{}  {name}", zorp_agent::sessions::short(&id)),
                Err(e) => {
                    eprintln!("zorp-agent: {e}");
                    std::process::exit(1);
                }
            }
        }
        Some(ProjectAction::Rm { id }) => {
            let known = store.projects().unwrap_or_default();
            let Some(row) = resolve_project(&known, &id) else {
                eprintln!("zorp-agent: no project matching '{id}'");
                std::process::exit(1);
            };
            let (row_id, row_name) = (row.id.clone(), row.name.clone());
            let count = store
                .sessions_in_project(&row_id)
                .map(|ids| ids.len())
                .unwrap_or(0);
            match store.delete_project(&row_id) {
                Ok(true) => {
                    // Said out loud, because it is the thing a person is
                    // afraid of and it is the thing that does not happen.
                    println!("removed the project '{row_name}'");
                    println!(
                        "{count} conversation{} {} kept and no longer filed under it.",
                        if count == 1 { "" } else { "s" },
                        if count == 1 { "was" } else { "were" }
                    );
                }
                Ok(false) => {
                    eprintln!("zorp-agent: no project matching '{id}'");
                    std::process::exit(1);
                }
                Err(e) => {
                    eprintln!("zorp-agent: {e}");
                    std::process::exit(1);
                }
            }
        }
    }
}

/// Build the doctor's report, without printing it.
///
/// Split from `doctor` so the shape can be tested and so `/doctor` in the
/// chat REPL prints exactly the same thing rather than a second version of
/// it that drifts.
fn doctor_report(overrides: &Overrides) -> zorp_agent::doctor::Report {
    use zorp_agent::doctor::{Check, Report};

    let mut report = Report::default();
    report.checks.push(zorp_agent::doctor::feature_check());

    // What the real paths would resolve, through the same functions they
    // use, so this reports on the configuration that would actually run.
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let (user_flavor, project_flavor) = resolve_flavor(overrides);
    let merged = user_flavor.merge(project_flavor);
    let (base_url, model_name) = resolve_host_and_model(overrides, &merged);
    let provider = resolve_provider(overrides, &merged).unwrap_or_default();

    report.checks.push(Check::note(
        "endpoint",
        format!("{base_url} ({provider:?})"),
    ));
    if model_name.is_empty() {
        report.checks.push(Check::bad(
            "model",
            "no model set. Use --model, ZORP_MODEL, or a flavor.",
        ));
    } else {
        report.checks.push(Check::note("model", model_name.clone()));
    }
    report
        .checks
        .push(zorp_agent::doctor::api_key_check(&base_url));

    // A probe is a network call, so it goes through the same client every
    // other request goes through, with the same timeouts.
    report.checks.push(probe_endpoint(&base_url));

    for (name, path) in zorp_agent::doctor::state_paths() {
        report
            .checks
            .push(Check::note(name, path.display().to_string()));
    }
    report
        .checks
        .push(Check::note("workspace", cwd.display().to_string()));

    // The tools, observed rather than re-derived. `web_search_availability`
    // is the same function the registration site uses, which is what makes
    // its answer worth trusting.
    let policy = build_policy(overrides.approval.as_deref(), &merged, &cwd);
    let search = zorp_agent::web_search_availability(&policy);
    report.checks.push(if search.available {
        Check::ok("web_search", search.detail)
    } else {
        Check::off("web_search", search.detail)
    });

    // The model is never called: registration does not touch it, and this
    // agent exists only to be asked what it registered.
    let agent = Agent::new(
        Box::new(HttpModel {
            url: zorp_agent::join_url(&base_url, provider.path_suffix()),
            api_key: None,
            model: model_name.clone(),
            provider,
            max_tokens: None,
        }),
        String::new(),
        1,
        cwd,
        cancel_token(),
        ApprovalMode::NonInteractive,
    )
    .register_builtins_filtered(merged.tools.enabled.as_deref());
    report
        .checks
        .push(Check::note("tools", agent.tool_names().join(", ")));

    report
}

/// How long the reachability probe waits before calling an endpoint
/// unreachable. Long enough for a cold local runtime to answer, short
/// enough that somebody running `doctor` because something is wrong is not
/// left wondering whether it is wrong too.
const PROBE_TIMEOUT_SECS: u64 = 10;

/// Ask the configured endpoint whether it is there.
///
/// Through `zorp::http_agent`, which is the client every real request uses
/// and carries the same timeouts. A doctor that reached an endpoint the
/// real path would refuse would be worse than no doctor: it would report
/// healthy on the one configuration that cannot work.
///
/// A `GET` on the models listing, because it is the cheapest thing an
/// OpenAI-compatible endpoint answers and it costs no tokens. A 401 is a
/// reachable endpoint that wants a key, which is a different fault from a
/// refused connection and is reported as such.
///
/// Its own agent, the way `zorp-web`'s settings probes and `zorp-search`
/// each build one. `zorp::http_agent` is for model traffic and its read
/// timeout is 900 seconds by default, which is right for an answer a model
/// is still writing and wrong for a question whose whole job is to come
/// back quickly: an endpoint that accepts the connection and then says
/// nothing would hold this for fifteen minutes, and a diagnostic that hangs
/// is worse than one that says it could not tell.
fn probe_endpoint(base_url: &str) -> zorp_agent::doctor::Check {
    use zorp_agent::doctor::Check;

    let url = zorp_agent::join_url(base_url, "models");
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(PROBE_TIMEOUT_SECS))
        .timeout_read(std::time::Duration::from_secs(PROBE_TIMEOUT_SECS))
        .build();
    let mut request = agent.get(&url);
    if let Ok(key) = std::env::var("ZORP_API_KEY") {
        if !key.trim().is_empty() {
            request = request.set("Authorization", &format!("Bearer {key}"));
        }
    }
    match request.call() {
        Ok(response) => Check::ok(
            "reachable",
            format!("{} answered {}", url, response.status()),
        ),
        Err(ureq::Error::Status(401 | 403, _)) => Check::bad(
            "reachable",
            format!("{url} answered but refused the credentials"),
        ),
        // Anything else with a status is a server that is there and did not
        // like this particular request, which is not what this is asking.
        Err(ureq::Error::Status(code, _)) => {
            Check::ok("reachable", format!("{url} answered {code}"))
        }
        Err(e) => Check::bad("reachable", format!("{url} did not answer: {e}")),
    }
}

/// `zorp-agent doctor`.
fn doctor(overrides: &Overrides) {
    let report = doctor_report(overrides);
    for line in report.lines() {
        println!("{line}");
    }
    if !report.healthy() {
        std::process::exit(1);
    }
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

/// The whole resolution chain for one setting, in one place.
///
/// Flag, environment variable, flavor, saved file, default. The saved file
/// is the new step and it sits below the flavor on purpose: a project that
/// pins a model means it for that project, where the file is a person's
/// standing preference across all of them. Nothing that used to win stops
/// winning, which was the point.
///
/// `saved` is passed in rather than read here so a caller that resolves
/// several settings reads the file once.
fn pick_with(
    flag: Option<&str>,
    env: &'static str,
    flavor: Option<&str>,
    saved: Option<&str>,
    default: &str,
) -> String {
    zorp_agent::config::resolve(flag, env, flavor, saved, default).value
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

fn report_outcome(
    outcome: &Outcome,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> &'static str {
    match outcome {
        Outcome::Complete(answer) => {
            let rendered = render_assistant_text(answer, std::io::stdout().is_terminal());
            let _ = writeln!(stdout, "{rendered}");
            "done"
        }
        Outcome::StepLimit => {
            let _ = writeln!(stderr, "zorp-agent: {}", outcome.describe());
            "step_limit"
        }
        Outcome::VerificationFailed { .. } => {
            let _ = writeln!(stderr, "zorp-agent: {}", outcome.describe());
            "verification_failed"
        }
        Outcome::Error(e) => {
            let _ = writeln!(stderr, "zorp-agent: {e}");
            "error"
        }
        Outcome::Cancelled => {
            let _ = writeln!(stderr, "zorp-agent: {}", outcome.describe());
            "cancelled"
        }
        Outcome::RepeatedAction => {
            let _ = writeln!(stderr, "zorp-agent: {}", outcome.describe());
            "repeated_action"
        }
        Outcome::Blocked => {
            let _ = writeln!(
                stderr,
                "zorp-agent: {}. Re-run with --yes to auto-approve edits and \
                 commands, or set an approval preset in a flavor.",
                outcome.describe()
            );
            "blocked"
        }
    }
}

fn finish(outcome: Outcome, store_status: Option<(&Store, &str)>) {
    let mut stdout = std::io::stdout();
    let mut stderr = std::io::stderr();
    let status = report_outcome(&outcome, &mut stdout, &mut stderr);
    if let Some((store, id)) = store_status {
        let _ = store.set_status(id, status);
    }
    if !matches!(outcome, Outcome::Complete(_)) {
        let _ = stdout.flush();
        let _ = stderr.flush();
        std::process::exit(1);
    }
}

#[cfg(test)]
mod finish_tests {
    use super::*;

    #[test]
    fn refused_request_is_reported_with_the_agent_prefix_before_exit() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let status = report_outcome(
            &Outcome::Error("prefill rejected".into()),
            &mut stdout,
            &mut stderr,
        );

        assert_eq!(status, "error");
        assert!(stdout.is_empty());
        assert_eq!(
            String::from_utf8(stderr).unwrap(),
            "zorp-agent: prefill rejected\n"
        );
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
    // Read once for both, rather than once per setting.
    let saved = zorp_agent::config::load().unwrap_or_default();
    let base_url = pick_with(
        overrides.base_url.as_deref(),
        "ZORP_BASE_URL",
        merged.base_url.as_deref(),
        saved.base_url.as_deref(),
        "http://localhost:11434/v1",
    );
    let mut model_name = pick_with(
        overrides.model.as_deref(),
        "ZORP_MODEL",
        merged.model.as_deref(),
        saved.model.as_deref(),
        "",
    );

    if model_name.is_empty() && base_url.contains("localhost:11434") {
        let tags_url = base_url.replace("/v1", "/api/tags");
        // Plain GET, so it cannot go through `zorp_raw`, which only knows
        // how to POST a JSON body. It goes through the shared agent anyway:
        // that is the one place timeouts are decided, and a bare
        // `ureq::get` has none, so an Ollama that accepts the connection and
        // then says nothing would hold the CLI here before it had printed a
        // single line.
        if let Ok(res) = zorp::http_agent().get(&tags_url).call() {
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
    if let Some(provider) = merged.provider {
        return Ok(provider);
    }
    if let Some(saved) = zorp_agent::config::load().and_then(|s| s.provider) {
        if !saved.trim().is_empty() {
            return saved.parse();
        }
    }
    Ok(Provider::default())
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
        .or_else(|| zorp_agent::config::load().and_then(|s| s.max_tokens))
}

fn run(
    task: String,
    images: &[PathBuf],
    auto_approve: bool,
    no_verify: bool,
    use_recall: bool,
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
    let merged = user_flavor.merge(project_flavor);
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

    // Before the model is called, never after. A recall run afterwards would
    // be a search for what the answer turned out to need, which is a
    // different thing from what the question asked for, and the model would
    // already have answered without it.
    //
    // The block goes on the end of the seed, which is what keeps it out of
    // the store: `with_message_records` counts what it is handed as already
    // persisted, so `sync` never offers it to the recorder. Persisted, it
    // would be re-embedded and recalled next turn, which is the tail eating
    // this whole design avoids.
    let mut out = LineRenderer::new(std::io::stderr(), std::io::stderr().is_terminal());
    if let Some(block) = recall_into_turn(use_recall, &task, &mut out) {
        let mut records: Vec<zorp_agent::MessageRecord> = agent
            .messages
            .iter()
            .cloned()
            .map(zorp_agent::MessageRecord::from)
            .collect();
        records.push(zorp_agent::Message::user(block).into());
        agent = agent.with_message_records(records);
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

/// `zorp-agent ensemble`. Builds the main agent exactly as `run` does, with
/// the roster's main model in place of the configured one, and one model
/// per reviewer on the same endpoint and key. Then hands everything to the
/// loop, which is the only thing that launches a run or a review.
#[cfg(feature = "ensemble")]
fn ensemble(instruction: &str, auto_approve: bool, no_verify: bool, overrides: &Overrides) {
    use zorp_agent::ensemble::{
        self, EnsembleConfig, Roles, Roster, DEFAULT_REVIEW_STEPS, LOG_DIR_VAR, REVIEW_STEPS_VAR,
        ROSTER_VAR,
    };

    let roster_path = std::env::var(ROSTER_VAR)
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            eprintln!("zorp-agent: ensemble needs {ROSTER_VAR} naming a roles file");
            std::process::exit(2);
        });
    let roster = Roster::load(Path::new(&roster_path)).unwrap_or_else(|e| {
        eprintln!("zorp-agent: {e}");
        std::process::exit(2);
    });
    // Zero is refused the way a roster's `rounds = 0` is. With no steps
    // every reviewer comes back unusable and the roster empties itself by
    // round two, and nothing in the record names the variable that did it.
    let review_steps = match std::env::var(REVIEW_STEPS_VAR)
        .ok()
        .filter(|v| !v.is_empty())
    {
        None => DEFAULT_REVIEW_STEPS,
        Some(v) => match v.parse::<usize>() {
            Ok(n) if n > 0 => n,
            _ => {
                eprintln!(
                    "zorp-agent: {REVIEW_STEPS_VAR} must be a whole number of at least 1, not {v:?}"
                );
                std::process::exit(2);
            }
        },
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
    let merged = user_flavor.merge(project_flavor);
    let system = compose_system_with_persona(&cwd, persona(&cwd, &merged).as_deref());
    // The roster names every model this subcommand ever uses, so the base
    // URL is all this needs from resolve_host_and_model's job. Calling it
    // for that alone would resolve a model name too, and an empty one
    // against the default Ollama URL walks into an interactive model
    // picker that blocks on stdin, a prompt --yes does not skip and whose
    // answer would be thrown away regardless.
    let base_url = pick_with(
        overrides.base_url.as_deref(),
        "ZORP_BASE_URL",
        merged.base_url.as_deref(),
        zorp_agent::config::load()
            .unwrap_or_default()
            .base_url
            .as_deref(),
        "http://localhost:11434/v1",
    );
    let provider = resolve_provider(overrides, &merged).unwrap_or_else(|e| {
        eprintln!("zorp-agent: {e}");
        std::process::exit(2);
    });
    let api_key = std::env::var("ZORP_API_KEY").ok().filter(|s| !s.is_empty());
    let max_tokens = resolve_max_tokens(overrides, &merged);
    let url = join_url(&base_url, provider.path_suffix());
    let model_for = |name: &str| -> zorp_agent::ConfiguredHttpModel {
        HttpModel {
            url: url.clone(),
            api_key: api_key.clone(),
            model: name.to_string(),
            provider,
            max_tokens,
        }
        .try_with_env_reasoning_mode(merged.reasoning_mode)
        .unwrap_or_else(|e| {
            eprintln!("zorp-agent: {e}");
            std::process::exit(2);
        })
    };
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
    let mut main = Agent::new(
        Box::new(model_for(&roster.main)),
        system,
        steps,
        cwd.clone(),
        cancel.clone(),
        approval.clone(),
    )
    .register_builtins_filtered(merged.tools.enabled.as_deref())
    .with_policy(build_policy(overrides.approval.as_deref(), &gated, &cwd));
    main = attach_mcp_tools(main, overrides, true);
    main = attach_verifier(main, no_verify, &gated);
    let recorder_store = open_store();
    if let Some(store) = &recorder_store {
        if let Err(e) = store.create_session(
            &session_id,
            instruction,
            &cwd.display().to_string(),
            &roster.main,
        ) {
            eprintln!("zorp-agent: could not create session: {e}");
        } else if let Ok(rec_store) = Store::open_default() {
            main = main.with_recorder(Box::new(SqliteRecorder::new(
                rec_store,
                session_id.clone(),
                0,
                0,
            )));
        }
    }

    let reviewers: Vec<Box<dyn zorp_agent::Model>> = roster
        .reviewers
        .iter()
        .map(|name| Box::new(model_for(name)) as Box<dyn zorp_agent::Model>)
        .collect();
    let log_dir = std::env::var(LOG_DIR_VAR)
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| cwd.join("scratch").join("ensemble").join(&session_id));
    let config = EnsembleConfig {
        roster,
        review_steps,
        log_dir,
    };
    eprintln!(
        "zorp-agent: ensemble, main {} with {} reviewers, {} rounds, record in {}",
        config.roster.main,
        config.roster.reviewers.len(),
        config.roster.rounds,
        config.log_dir.display()
    );
    let finished = ensemble::run(
        &config,
        Roles { main, reviewers },
        instruction,
        &cwd,
        cancel,
        approval,
    );
    eprintln!(
        "zorp-agent: ensemble stopped: {}; {} open findings",
        finished.record.stopped,
        finished.record.open_at_end.len()
    );
    let status_target = recorder_store.as_ref().map(|s| (s, session_id.as_str()));
    finish(finished.outcome, status_target);
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
    let mut system = zorp_agent::investigate::SYSTEM_PREAMBLE.to_string();
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
const CRITIQUE_SYSTEM_PREAMBLE: &str = "\
You are auditing a draft against a fixed research run record. You cannot \
add evidence to that record, and you cannot change the hypothesis, the \
metric, or the kill threshold: they are pre-registered, and only a human \
moves them. Work only from the record you are handed.";

/// The critic gets no tools at all. It is a text task over a draft and a
/// ledger, both of which arrive in the prompt, so a tool is not a
/// capability it needs, only one it could misuse.
#[cfg(feature = "research")]
const CRITIQUE_TOOLS: &[String] = &[];

/// How many revision rounds to allow: the flag, else
/// `ZORP_CRITIQUE_ROUNDS`, else the built-in default. An unparseable env
/// var falls through to the default rather than failing the run, matching
/// how ZORP_MAX_STEPS is read.
#[cfg(feature = "research")]
fn resolve_critique_rounds(flag: Option<usize>, env: Option<String>) -> usize {
    flag.or_else(|| env.and_then(|v| v.trim().parse().ok()))
        .unwrap_or(zorp_agent::critique::DEFAULT_MAX_REVISIONS)
}

#[cfg(feature = "research")]
fn critique(
    question: &str,
    critique_rounds: Option<usize>,
    auto_approve: bool,
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
    let mut system = CRITIQUE_SYSTEM_PREAMBLE.to_string();
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
    .register_builtins_filtered(Some(CRITIQUE_TOOLS))
    .with_policy(build_policy(overrides.approval.as_deref(), &gated, &cwd));

    let project = match zorp_track::Project::open(&cwd) {
        Ok(p) => p,
        Err(e) => {
            // Exit 1, not 2, for the same reason co-write does: a store
            // that will not open is a runtime failure, not a usage error.
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
    let rounds =
        resolve_critique_rounds(critique_rounds, std::env::var("ZORP_CRITIQUE_ROUNDS").ok());

    match zorp_agent::critique::run(&mut agent, &project, &track_id, rounds, &checkpoint_mode) {
        Ok(report) => {
            if report.was_clean() {
                println!(
                    "critique: the draft is supported by the record as it stands; nothing changed. Notes at .zorp/tracks/{track_id}/critique.md"
                );
            } else {
                println!(
                    "critique: {} finding(s), {} left after {} revision round(s). {} Notes at .zorp/tracks/{track_id}/critique.md",
                    report.initial(),
                    report.remaining(),
                    report.rounds.len() - 1,
                    if report.draft_changed {
                        format!("draft.md revised, original kept at .zorp/tracks/{track_id}/draft.pre-critique.md.")
                    } else {
                        "draft.md left unchanged.".to_string()
                    }
                );
            }
            if !report.approved {
                println!("critique: not yet accepted; the draft and the notes are both on disk");
            }
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
/doctor              say what this build can do and whether it can reach anything
/compact             summarize the older conversation so it fits the window
/compact <what>      the same, steered toward what you want kept
/diff                summarize this session's file changes
/status              show session id and status
/undo                revert the last recorded file change
/approve             auto-approve edits and commands this session
/deny                deny edits and commands this session
/clear               forget the conversation (keep system prompt)
/reasoning           show the active session reasoning mode
/reasoning <mode>    set reasoning mode for future turns in this session
/branch [n]          fork this conversation at answer n (default: the latest)
/recall <query>      search your own conversations by meaning
/panel [path]        review the last answer, or a file, with several reviewers at once
/project             say which project this conversation is in
/project <name>      file it under that project, or `none` to take it out
/capsules            list available and loaded capsules
/skills              list the skills the model can load
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
    /// What the flags said, so `/doctor` reports on the configuration this
    /// session is actually running under rather than on the environment
    /// alone.
    overrides: &'a Overrides,
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
    let merged = user_flavor.merge(project_flavor);
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
    out.notice("zorp-agent chat. /help for commands, /exit to quit");

    let ctx = ChatContext {
        store: &store,
        session_id: &session_id,
        cwd: &cwd,
        model_name: &model_name,
        overrides,
    };

    if !std::io::stdin().is_terminal() {
        chat_line_loop(&mut agent, ctx, &mut capsules, &mut out);
        return;
    }

    let mut line = zorp_agent::line_editor::Line::new();
    let mut history = zorp_agent::line_editor::History::load();
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
            // The visible width of everything before the cursor, which is
            // not the character offset once a paste is drawn as a marker.
            let mut prompt = String::from("› ");
            let mut before = 0usize;
            let mut seen = 0usize;
            for seg in line.segments() {
                let (drawn, width) = match seg {
                    Segment::Text(t) => (t.clone(), t.chars().count()),
                    Segment::Paste(s) => {
                        let marker = format!("[pasted +{} characters]", s.chars().count());
                        let width = marker.chars().count();
                        (marker, width)
                    }
                    Segment::Image { index, .. } => {
                        let marker = format!("[Image {index}]");
                        let width = marker.chars().count();
                        (marker, width)
                    }
                };
                let occupies = match seg {
                    Segment::Text(t) => t.chars().count(),
                    _ => 1,
                };
                if seen + occupies <= line.cursor() {
                    before += width;
                } else if seen < line.cursor() {
                    // Inside a text segment: only the part before the
                    // cursor counts.
                    before += line.cursor() - seen;
                }
                seen += occupies;
                prompt.push_str(&drawn);
            }
            let _ = crossterm::execute!(
                std::io::stdout(),
                crossterm::terminal::Clear(crossterm::terminal::ClearType::CurrentLine)
            );
            print!("\r{}", prompt);
            // Back to where the cursor is, so Left and Right land where a
            // person can see them.
            let _ = crossterm::execute!(
                std::io::stdout(),
                crossterm::cursor::MoveToColumn((before + 2) as u16)
            );
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
                            // A trailing backslash continues the line
                            // rather than sending it. Shift-Enter is not
                            // reported as distinct from Enter by most
                            // terminals, so a backslash is what works
                            // everywhere.
                            if line.wants_continuation() {
                                line.continue_line();
                                println!("\r");
                                redraw = true;
                                continue;
                            }
                            if line.is_blank() {
                                redraw = true;
                                continue;
                            }
                            println!("\r");
                            history.remember(&line.flat());
                            // Convert segments to content parts
                            use zorp_agent::ContentPart;
                            let parts = segments_to_parts(line.segments(), &cwd);

                            line.clear();
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
                                    // Pure text command, so the existing handler takes it
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
                            line.backspace();
                            redraw = true;
                        }
                        crossterm::event::KeyCode::Delete => {
                            line.delete_forward();
                            redraw = true;
                        }
                        crossterm::event::KeyCode::Left
                            if key.modifiers.contains(crossterm::event::KeyModifiers::ALT) =>
                        {
                            line.word_left();
                            redraw = true;
                        }
                        crossterm::event::KeyCode::Right
                            if key.modifiers.contains(crossterm::event::KeyModifiers::ALT) =>
                        {
                            line.word_right();
                            redraw = true;
                        }
                        crossterm::event::KeyCode::Left => {
                            line.left();
                            redraw = true;
                        }
                        crossterm::event::KeyCode::Right => {
                            line.right();
                            redraw = true;
                        }
                        crossterm::event::KeyCode::Home => {
                            line.home();
                            redraw = true;
                        }
                        crossterm::event::KeyCode::End => {
                            line.end();
                            redraw = true;
                        }
                        crossterm::event::KeyCode::Up => {
                            if let Some(previous) = history.previous(&line.flat()) {
                                line.set_text(&previous);
                            }
                            redraw = true;
                        }
                        crossterm::event::KeyCode::Down => {
                            if let Some(next) = history.forward() {
                                line.set_text(&next);
                            }
                            redraw = true;
                        }
                        crossterm::event::KeyCode::Tab => {
                            let flat = line.flat();
                            let names = capsules.registry().names();
                            let candidates = zorp_agent::line_editor::complete(
                                &flat,
                                zorp_agent::CHAT_COMMANDS,
                                &names,
                            );
                            match candidates.len() {
                                0 => {}
                                1 => {
                                    line.set_text(&format!("{} ", candidates[0]));
                                }
                                _ => {
                                    // Fill in as much as every candidate
                                    // shares, then show the rest rather
                                    // than guessing between them.
                                    let shared =
                                        zorp_agent::line_editor::common_prefix(&candidates);
                                    if shared.chars().count() > flat.chars().count() {
                                        line.set_text(&shared);
                                    } else {
                                        println!("\r");
                                        println!("\r{}", candidates.join("  "));
                                    }
                                }
                            }
                            redraw = true;
                        }
                        crossterm::event::KeyCode::Char('a')
                            if key
                                .modifiers
                                .contains(crossterm::event::KeyModifiers::CONTROL) =>
                        {
                            line.home();
                            redraw = true;
                        }
                        crossterm::event::KeyCode::Char('e')
                            if key
                                .modifiers
                                .contains(crossterm::event::KeyModifiers::CONTROL) =>
                        {
                            line.end();
                            redraw = true;
                        }
                        crossterm::event::KeyCode::Char('w')
                            if key
                                .modifiers
                                .contains(crossterm::event::KeyModifiers::CONTROL) =>
                        {
                            line.delete_word_back();
                            redraw = true;
                        }
                        crossterm::event::KeyCode::Char('u')
                            if key
                                .modifiers
                                .contains(crossterm::event::KeyModifiers::CONTROL) =>
                        {
                            line.delete_to_start();
                            redraw = true;
                        }
                        crossterm::event::KeyCode::Char('k')
                            if key
                                .modifiers
                                .contains(crossterm::event::KeyModifiers::CONTROL) =>
                        {
                            line.delete_to_end();
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
                                        line.push_opaque(Segment::Image {
                                            data: png_buf,
                                            mime_type: "image/png".into(),
                                            index: image_counter,
                                        });
                                        used_clipboard = true;
                                    }
                                }
                            }
                            if !used_clipboard {
                                // Fall through to normal 'v' character
                                line.insert('v');
                            }
                            redraw = true;
                        }
                        crossterm::event::KeyCode::Char(c) => {
                            line.insert(c);
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
                            line.push_opaque(Segment::Image {
                                data,
                                mime_type,
                                index: image_counter,
                            });
                        } else {
                            line.push_opaque(Segment::Paste(s));
                        }
                    } else {
                        line.push_opaque(Segment::Paste(s));
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
        overrides,
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
        ChatCommand::Compact(focus) => {
            // A person asking is the trigger, so this works whether or not
            // the window is known. The summary goes to the `compactions`
            // table and never into the transcript on disk: what is sent
            // shrinks, what was said does not.
            let asked = agent.compactable_messages();
            if asked == 0 {
                out.notice(zorp_agent::compaction::NOT_ENOUGH);
            } else {
                out.notice(&format!("Summarizing {asked} older messages..."));
                match agent.compact_now(focus) {
                    Some(done) => out.notice(&format!(
                        "{done} older messages are now a summary. The full transcript is \
                         still on disk."
                    )),
                    None => out.notice(zorp_agent::compaction::NOT_ENOUGH),
                }
            }
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
        ChatCommand::Doctor => {
            // The same report the subcommand prints, from the same
            // function, so the two cannot drift into saying different
            // things about one machine.
            out.notice(&doctor_report(overrides).lines().join("\n"));
        }
        ChatCommand::Branch(answer) => {
            // Against the store rather than the transcript in memory,
            // because `branch_session` copies stored rows and the answer
            // numbering it uses is the store's. Counting what is on screen
            // would put the two out of step the moment a turn failed to
            // persist.
            match store.as_ref() {
                None => out.notice("no session store, so there is nothing to branch"),
                Some(s) => {
                    let total = s
                        .load_messages(session_id)
                        .unwrap_or_default()
                        .iter()
                        .filter(|m| m.role == "assistant" && !m.text().trim().is_empty())
                        .count();
                    if total == 0 {
                        out.notice("this conversation has no answers to branch at yet");
                    } else {
                        let at = answer.unwrap_or(total);
                        if at > total {
                            out.notice(&format!(
                                "this conversation has {total} answer{}, so /branch {at} is \
                                 out of range",
                                if total == 1 { "" } else { "s" }
                            ));
                        } else {
                            let new_id = zorp_agent::new_session_id();
                            match Store::open_default()
                                .and_then(|mut fresh| fresh.branch_session(session_id, at, &new_id))
                            {
                                Ok(true) => out.notice(&format!(
                                    "branched at answer {at} of {total}. Continue it with \
                                     `zorp-agent resume {}`",
                                    zorp_agent::sessions::short(&new_id)
                                )),
                                Ok(false) => {
                                    out.notice(&format!("this conversation has no answer {at}"))
                                }
                                Err(e) => out.notice(&format!("could not branch: {e}")),
                            }
                        }
                    }
                }
            }
        }
        ChatCommand::Recall(query) => {
            // A person reading their own history. Nothing from this reaches
            // the model: that is `--recall` on a turn, which is a different
            // feature and a different risk.
            #[cfg(feature = "recall")]
            {
                if query.trim().is_empty() {
                    out.notice("usage: /recall <what you are looking for>");
                } else {
                    match zorp_agent::recall::search(
                        &query,
                        zorp_agent::recall::DEFAULT_LIMIT,
                        None,
                    ) {
                        Err(e) => out.notice(&e.to_string()),
                        Ok(hits) if hits.is_empty() => out.notice("nothing matched"),
                        Ok(hits) => {
                            let names = store
                                .as_ref()
                                .and_then(|s| s.sessions().ok())
                                .unwrap_or_default()
                                .into_iter()
                                .map(|row| (row.id.clone(), zorp_agent::sessions::name(&row)))
                                .collect();
                            out.notice(&recall_lines(&hits, &names).join("\n"));
                        }
                    }
                }
            }
            #[cfg(not(feature = "recall"))]
            {
                let _ = query;
                out.notice(
                    "this zorp-agent was built without the recall feature, so there is \
                     nothing to search",
                );
            }
        }
        ChatCommand::Panel(path) => {
            // The last answer by default, because that is what a person in
            // a conversation means by "review this". A path reviews the
            // file instead.
            let material = match &path {
                Some(path) => std::fs::read_to_string(path)
                    .map(|body| (path.clone(), body))
                    .map_err(|e| format!("cannot read {path}: {e}")),
                None => agent
                    .messages
                    .iter()
                    .rev()
                    .find(|m| m.role == "assistant" && !m.text().trim().is_empty())
                    .map(|m| ("the last answer".to_string(), m.text().into_owned()))
                    .ok_or_else(|| "this conversation has no answer to review yet".to_string()),
            };
            match material {
                Err(why) => out.notice(&why),
                Ok((label, body)) => {
                    let config = zorp_agent::PanelConfig::default();
                    out.notice(&format!(
                        "panel on {label}: {} reviewers",
                        config.lenses.len()
                    ));
                    // A reviewer gets strictly less than the panel that
                    // launched it: a read-only allow list, and never this
                    // session's approval mode. `AutoApprove` is a fixed
                    // value here and not the caller's, because there is
                    // nothing approval gated in that tool set to approve.
                    let report = zorp_agent::panel::run(
                        agent.model().clone_box().as_ref(),
                        &zorp_agent::Target { label, body },
                        &config,
                        cwd.to_path_buf(),
                        cancel_token(),
                        ApprovalMode::AutoApprove,
                        &zorp_agent::SilentObserver,
                    );
                    out.notice(&zorp_agent::panel::report_lines(&report, false).join("\n"));
                }
            }
        }
        ChatCommand::Project(wanted) => {
            // Against a fresh handle, because filing is a write and the
            // REPL holds its store immutably.
            match Store::open_default() {
                Err(e) => out.notice(&format!("no session store: {e}")),
                Ok(mut fresh) => {
                    let known = fresh.projects().unwrap_or_default();
                    match wanted.as_deref() {
                        None => {
                            let current = fresh.session_project(session_id).unwrap_or_default();
                            match current.and_then(|id| {
                                known.iter().find(|p| p.id == id).map(|p| p.name.clone())
                            }) {
                                Some(name) => out.notice(&format!("in project '{name}'")),
                                None => out.notice(
                                    "not in a project. /project <name> files it, and \
                                     /projects lists them.",
                                ),
                            }
                        }
                        Some("none") | Some("off") => {
                            match fresh.set_session_project(session_id, None) {
                                Ok(zorp_agent::SetProject::Done) => {
                                    out.notice("taken out of its project")
                                }
                                Ok(_) => out.notice("this conversation is not in the store yet"),
                                Err(e) => out.notice(&format!("could not change it: {e}")),
                            }
                        }
                        Some(wanted) => match resolve_project(&known, wanted) {
                            None => out.notice(&format!(
                                "no project matching '{wanted}'. /projects lists them."
                            )),
                            Some(row) => {
                                let (id, name) = (row.id.clone(), row.name.clone());
                                match fresh.set_session_project(session_id, Some(&id)) {
                                    Ok(zorp_agent::SetProject::Done) => {
                                        out.notice(&format!("filed under '{name}'"))
                                    }
                                    Ok(zorp_agent::SetProject::NoSuchSession) => {
                                        out.notice("this conversation is not in the store yet")
                                    }
                                    Ok(zorp_agent::SetProject::NoSuchProject) => {
                                        out.notice("that project is gone")
                                    }
                                    Err(e) => out.notice(&format!("could not file it: {e}")),
                                }
                            }
                        },
                    }
                }
            }
        }
        ChatCommand::Capsules => out.notice(&capsules.list_display()),
        ChatCommand::Skills => {
            // Read from disk now rather than from whatever was discovered
            // when the session started: a skill can be added to a directory
            // while the REPL is sitting there, and the next turn would see
            // it. Reporting a stale list would be worse than reporting
            // none.
            let scopes = zorp_skill::scope_dirs_from_env(cwd);
            let (registry, warnings) = zorp_skill::SkillRegistry::discover(&scopes);
            for warning in &warnings {
                out.notice(warning);
            }
            if registry.is_empty() {
                out.notice(
                    "no skills found. Put a directory holding a SKILL.md under \
                     ~/.claude/skills, under .claude/skills here, or wherever \
                     ZORP_SKILLS_DIR points.",
                );
            } else {
                // The same index the model is shown in the `skill` tool's
                // description, so what a person reads here is what the
                // model has to choose from.
                out.notice(&registry.index());
            }
        }
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
                feed_recall(session_id);
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
            out.notice(&format!("unknown command '/{name}'. Try /help"));
        }
        ChatCommand::Say(text) => {
            if !text.is_empty() {
                run_and_render(agent, &text, out);
                // After the answer, so the person has it before anything
                // else is asked of the model, and only if this conversation
                // has no name yet.
                spawn_titling(session_id, agent.config().model);
            }
        }
    }
    exit
}

/// Name this conversation, if it still needs a name.
///
/// One model call per conversation, not per turn: `title_session_in` reads
/// `display_title` first and does nothing when there is one, and that read
/// survives a restart because it asks the store rather than remembering.
///
/// On its own thread, because the person is already typing the next thing
/// and a sidebar label is not worth making them wait for. The thread opens
/// its own store handle for the same reason `zorp-web`'s does: the one in
/// the REPL is borrowed and this outlives the borrow.
///
/// Every failure is the same failure: nothing is written and the first
/// message keeps showing. A conversation with no title is not a broken
/// conversation, which is why nothing here is reported.
fn spawn_titling(session_id: &str, model: Box<dyn zorp_agent::Model>) {
    if !zorp_agent::title::enabled() {
        return;
    }
    let session_id = session_id.to_string();
    std::thread::spawn(move || {
        let Ok(store) = Store::open_default() else {
            return;
        };
        zorp_agent::title::title_session_in(&store, &session_id, |question, answer| {
            // No reasoning mode, and never the one the person set for their
            // own work: a sidebar label is not worth a thinking budget.
            let reply = model
                .complete(&zorp_agent::title::prompt(question, answer), &[])
                .ok()?;
            Some(reply.content)
        });
    });
}

/// The same, run to completion rather than spawned.
///
/// `resume` exits the process as soon as it has printed, so a background
/// thread would be killed before it wrote anything.
///
/// The one-shot path deliberately does not do this. A one-shot is one
/// command whose whole value is that it answers and exits, and a second
/// model call for a label would double what it costs and how long it takes.
/// A conversation started that way gets its name the first time anybody
/// comes back to it, which is the first time the name is worth anything.
fn title_now(session_id: &str, model: Box<dyn zorp_agent::Model>) {
    if !zorp_agent::title::enabled() {
        return;
    }
    let Ok(store) = Store::open_default() else {
        return;
    };
    zorp_agent::title::title_session_in(&store, session_id, |question, answer| {
        let reply = model
            .complete(&zorp_agent::title::prompt(question, answer), &[])
            .ok()?;
        Some(reply.content)
    });
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

/// Print the conversations in the store, newest first.
///
/// The store is shared with the browser, so this is one list and not the
/// terminal's own. A conversation started in a sidebar is in here, and a
/// conversation started here is in that sidebar.
fn list_sessions(limit: Option<usize>, all: bool, project: Option<&str>) {
    let store = match open_store() {
        Some(s) => s,
        None => std::process::exit(1),
    };
    let known = store.projects().unwrap_or_default();
    // A filter takes an id or a name, because a person reading a listing
    // has the name in front of them and the id is the thing they would
    // have to go and look up.
    let wanted = match project {
        None => None,
        Some(wanted) => match resolve_project(&known, wanted) {
            Some(row) => Some(row.id.clone()),
            None => {
                eprintln!("zorp-agent: no project matching '{wanted}'");
                eprintln!("zorp-agent: run `zorp-agent projects` to see what there is");
                std::process::exit(1);
            }
        },
    };
    let mut rows = match store.sessions() {
        Ok(rows) => rows,
        Err(e) => {
            eprintln!("zorp-agent: {e}");
            std::process::exit(1);
        }
    };
    if let Some(wanted) = &wanted {
        rows.retain(|row| row.project_id.as_deref() == Some(wanted.as_str()));
    }
    if rows.is_empty() {
        // Not an error and not an empty screen. Somebody who has just
        // installed this needs to be told that is what they are looking at.
        match project {
            Some(wanted) => println!("No conversations in '{wanted}'."),
            None => println!("No conversations yet. Run `zorp-agent chat` to start one."),
        }
        return;
    }

    // Which project each conversation is in, once, rather than a lookup
    // per row.
    let name_of: std::collections::HashMap<&str, &str> = known
        .iter()
        .map(|p| (p.id.as_str(), p.name.as_str()))
        .collect();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let shown = if all {
        rows.len()
    } else {
        limit
            .unwrap_or(zorp_agent::sessions::DEFAULT_LIMIT)
            .min(rows.len())
    };
    for row in rows.iter().take(shown) {
        let line = zorp_agent::sessions::line(row, now);
        // The project a conversation is in, when it is in one and the
        // listing is not already filtered to that project, where saying it
        // on every row is noise.
        match row
            .project_id
            .as_deref()
            .filter(|_| wanted.is_none())
            .and_then(|id| name_of.get(id))
        {
            Some(name) => println!("{line}  [{name}]"),
            None => println!("{line}"),
        }
    }
    if shown < rows.len() {
        println!("\n{} more. Pass --limit <n>, or --all.", rows.len() - shown);
    }
}

/// The conversation `resume` was asked for, or the most recent one.
///
/// Exits rather than returning on anything it cannot resolve, because every
/// caller is `main` and every failure is the same shape: say what happened
/// in a line a person can act on, and stop.
fn resolve_session(store: &Store, wanted: Option<&str>) -> zorp_agent::SessionRow {
    let rows = match store.sessions() {
        Ok(rows) => rows,
        Err(e) => {
            eprintln!("zorp-agent: {e}");
            std::process::exit(1);
        }
    };
    let Some(wanted) = wanted else {
        // No id at all: the most recent conversation, which is the thing
        // somebody wants far more often than any particular one.
        let Some(row) = rows.into_iter().next() else {
            eprintln!("zorp-agent: no conversations yet. Run `zorp-agent chat` to start one.");
            std::process::exit(1);
        };
        return row;
    };
    match zorp_agent::sessions::resolve(&rows, wanted) {
        Ok(row) => row.clone(),
        Err(zorp_agent::sessions::ResolveError::NotFound) => {
            eprintln!("zorp-agent: no session '{wanted}'");
            eprintln!("zorp-agent: run `zorp-agent sessions` to see what there is");
            std::process::exit(1);
        }
        Err(zorp_agent::sessions::ResolveError::Ambiguous(ids)) => {
            // Never a guess. The wrong one drops somebody into a stranger's
            // thread and the transcript looks perfectly plausible.
            eprintln!(
                "zorp-agent: '{wanted}' matches {} conversations:",
                ids.len()
            );
            for id in ids {
                eprintln!("  {id}");
            }
            std::process::exit(1);
        }
    }
}

/// Whether the stored status says a turn is in flight, and what it says.
///
/// The browser's own refusal reads a live map of the threads in its process.
/// A CLI cannot see those threads, and a second `zorp-agent` cannot see the
/// first one's, so the only shared signal is this column.
///
/// It is worth exactly as much as the writes behind it, and the writes were
/// half missing: `create_session` wrote `running` and only the CLI ever
/// wrote anything else, so every conversation the browser made read as
/// running for the rest of its life. `zorp-web` now writes the closing
/// status too, which makes this meaningful going forward. It does not make
/// it reliable: a process killed mid-turn leaves `running` behind with
/// nothing running. So this refuses and says what it read, and `--force`
/// gets past it, rather than trapping a conversation forever on the word of
/// a column nobody updated.
fn running_status(store: &Store, id: &str) -> Option<String> {
    match store.session_status(id) {
        Ok(Some(status)) if status == "running" => Some(status),
        _ => None,
    }
}

/// Ask before removing something that cannot be brought back.
fn confirm(question: &str) -> bool {
    use std::io::Write as _;
    eprint!("{question} [y/N] ");
    let _ = std::io::stderr().flush();
    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer).is_err() {
        return false;
    }
    matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

/// Delete a conversation and everything recorded under it.
///
/// The one command here that destroys something, so it is the one that
/// asks. `--yes` answers in advance, for a script.
fn remove_session(wanted: &str, yes: bool, force: bool) {
    let mut store = match open_store() {
        Some(s) => s,
        None => std::process::exit(1),
    };
    let row = resolve_session(&store, Some(wanted));
    let id = row.id.clone();
    let label = zorp_agent::sessions::name(&row);

    if !force {
        if let Some(status) = running_status(&store, &id) {
            eprintln!(
                "zorp-agent: {} is recorded as '{status}'. Another turn may be writing to it.",
                zorp_agent::sessions::short(&id)
            );
            eprintln!("zorp-agent: pass --force to delete it anyway.");
            std::process::exit(1);
        }
    }

    if !yes
        && !confirm(&format!(
            "Delete \"{label}\" and everything recorded under it?"
        ))
    {
        eprintln!("zorp-agent: nothing deleted");
        return;
    }

    match store.delete_session(&id) {
        Ok(true) => println!("deleted {} ({label})", zorp_agent::sessions::short(&id)),
        Ok(false) => {
            eprintln!("zorp-agent: no session '{wanted}'");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("zorp-agent: {e}");
            std::process::exit(1);
        }
    }
}

/// How many answers a conversation has, counted the way `branch_session`
/// counts them: an assistant message with text, from one, in seq order.
///
/// Counted here so `--answer` can default to the latest and so an out of
/// range number can say how many there are instead of failing blankly.
fn answer_count(store: &Store, id: &str) -> usize {
    store
        .load_messages(id)
        .unwrap_or_default()
        .iter()
        .filter(|m| m.role == "assistant" && !m.text().trim().is_empty())
        .count()
}

/// Copy a conversation up to one of its answers into a new one.
///
/// The browser branches per answer because the page has the answers on
/// screen to click. A terminal does not, so the honest shape is a number
/// that defaults to the most recent answer.
fn branch_session(wanted: &str, answer: Option<usize>, force: bool) {
    let mut store = match open_store() {
        Some(s) => s,
        None => std::process::exit(1),
    };
    let row = resolve_session(&store, Some(wanted));
    let id = row.id.clone();

    if !force {
        if let Some(status) = running_status(&store, &id) {
            eprintln!(
                "zorp-agent: {} is recorded as '{status}'. A copy taken while a turn is \
                 writing is not the conversation you can see.",
                zorp_agent::sessions::short(&id)
            );
            eprintln!("zorp-agent: pass --force to branch it anyway.");
            std::process::exit(1);
        }
    }

    let total = answer_count(&store, &id);
    if total == 0 {
        eprintln!(
            "zorp-agent: {} has no answers to branch at",
            zorp_agent::sessions::short(&id)
        );
        std::process::exit(1);
    }
    let answer = answer.unwrap_or(total);
    if answer == 0 || answer > total {
        eprintln!(
            "zorp-agent: {} has {total} answer{}, so --answer {answer} is out of range",
            zorp_agent::sessions::short(&id),
            if total == 1 { "" } else { "s" }
        );
        std::process::exit(1);
    }

    let new_id = zorp_agent::new_session_id();
    match store.branch_session(&id, answer, &new_id) {
        Ok(true) => {
            println!("{new_id}");
            eprintln!(
                "zorp-agent: branched {} at answer {answer} of {total}. Continue it with \
                 `zorp-agent resume {}`.",
                zorp_agent::sessions::short(&id),
                zorp_agent::sessions::short(&new_id)
            );
        }
        Ok(false) => {
            eprintln!(
                "zorp-agent: {} has no answer {answer}",
                zorp_agent::sessions::short(&id)
            );
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("zorp-agent: {e}");
            std::process::exit(1);
        }
    }
}

fn resume(wanted: Option<&str>, auto_approve: bool, no_verify: bool, overrides: &Overrides) {
    let store = match open_store() {
        Some(s) => s,
        None => std::process::exit(1),
    };
    let chosen = resolve_session(&store, wanted);
    let id: &str = &chosen.id;
    // Say which one, because `resume` with no id and `resume` with a prefix
    // both picked it rather than being handed it.
    if wanted.map(|w| w != id).unwrap_or(true) {
        eprintln!(
            "zorp-agent: resuming {} ({})",
            zorp_agent::sessions::short(id),
            zorp_agent::sessions::name(&chosen)
        );
    }
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
    let merged = user_flavor.merge(project_flavor);
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
    // The record is replayed through the same planner the browser uses, so
    // both surfaces resume a session the same way: the current system prompt
    // rather than whatever one was in force when the session started, no
    // dangling tool call for the provider to refuse, and the oldest material
    // dropped first when the window will not hold it.
    let budget = zorp_agent::ContextBudget::from_env();
    let latest = store.latest_compaction(id).unwrap_or_default();
    let plan = zorp_agent::plan_seed(messages, &system, &budget, latest.as_ref());
    if let Some(notice) = plan.report.notice() {
        eprintln!("zorp-agent: {notice}");
    }
    let mut agent = Agent::new(
        Box::new(model),
        system,
        steps,
        cwd.clone(),
        cancel,
        approval,
    )
    .with_context_budget(budget)
    .register_builtins_filtered(merged.tools.enabled.as_deref())
    .with_policy(build_policy(overrides.approval.as_deref(), &gated, &cwd))
    .with_message_records(plan.records);

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
    let model = agent.config().model;
    let outcome = agent.resume();
    title_now(id, model);
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
        // Leaked rather than threaded through every call site: it is one
        // small struct per test process and it keeps thirty callers from
        // growing an argument none of them cares about.
        let overrides: &'static Overrides = Box::leak(Box::new(Overrides::default()));
        ChatContext {
            store,
            session_id: "s1",
            cwd,
            model_name: "test-model",
            overrides,
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

        assert_eq!(out.notices, vec!["● demo: demo capsule".to_string()]);
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
    fn the_critique_round_bound_comes_from_the_flag_then_the_env_then_the_default() {
        assert_eq!(resolve_critique_rounds(Some(5), None), 5);
        // The flag wins over the environment, the same precedence
        // --max-steps has.
        assert_eq!(resolve_critique_rounds(Some(5), Some("9".to_string())), 5);
        assert_eq!(resolve_critique_rounds(None, Some("4".to_string())), 4);
        assert_eq!(
            resolve_critique_rounds(None, None),
            zorp_agent::critique::DEFAULT_MAX_REVISIONS
        );
    }

    #[cfg(feature = "research")]
    #[test]
    fn a_zero_critique_round_bound_is_honoured_rather_than_read_as_unset() {
        // Zero means "audit and record, do not revise", which is a real
        // request. Treating it as absent would silently start revising.
        assert_eq!(resolve_critique_rounds(Some(0), None), 0);
        assert_eq!(resolve_critique_rounds(None, Some("0".to_string())), 0);
    }

    #[cfg(feature = "research")]
    #[test]
    fn an_unparseable_critique_round_env_var_falls_back_to_the_default() {
        assert_eq!(
            resolve_critique_rounds(None, Some("lots".to_string())),
            zorp_agent::critique::DEFAULT_MAX_REVISIONS
        );
    }

    #[cfg(feature = "research")]
    #[test]
    fn the_critic_is_configured_with_no_tools_at_all() {
        // The pass hands the model the draft and the ledger in the
        // prompt. A tool is not something it needs, only something it
        // could reach the record with.
        assert!(CRITIQUE_TOOLS.is_empty());
        let agent = Agent::new(
            Box::new(zorp_agent::HttpModel {
                url: "http://127.0.0.1:1/v1/chat/completions".into(),
                api_key: None,
                model: "m".into(),
                provider: zorp_agent::Provider::OpenAiCompatible,
                max_tokens: None,
            }),
            "system",
            5,
            std::env::temp_dir(),
            zorp_agent::cancel_token(),
            ApprovalMode::AutoApprove,
        )
        .register_builtins_filtered(Some(CRITIQUE_TOOLS));
        assert!(
            agent.tool_names().is_empty(),
            "critique registered tools: {:?}",
            agent.tool_names()
        );
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
