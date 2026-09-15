//! Local web UI server for the zorp agent.
//!
//! The server constructs a real `Agent` rather than shelling out to the CLI,
//! so flavors, approval presets, the hard denylist and session persistence all
//! apply unchanged. See
//! `docs/superpowers/specs/2026-08-17-zorp-web-ui-design.md`.

pub mod api;
pub mod approval;
pub mod artifacts;
pub mod auth;
pub mod compaction;
pub mod documents;
pub mod event;
#[cfg(feature = "research")]
pub mod investigate;
#[cfg(feature = "memory")]
pub mod memory;
pub mod panel;
pub mod pdf;
#[cfg(feature = "research")]
pub mod prereg_infer;
#[cfg(feature = "recall")]
pub mod recall;
pub mod renderer;
pub mod settings;
pub mod state;
pub mod title;
pub mod tool_safety;
pub mod train;
pub mod turn;
pub mod voice;
pub mod workspace;

use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct ServeOptions {
    pub bind: String,
    pub port: u16,
    pub token: Option<String>,
    pub ui_dir: Option<PathBuf>,
    pub workspace: Option<PathBuf>,
    pub allow_origin: Vec<String>,
    pub additional_ui_candidates: Vec<PathBuf>,
}

pub struct Running {
    pub addr: SocketAddr,
    pub handle: tokio::task::JoinHandle<Result<(), std::io::Error>>,
}

#[derive(Debug)]
pub enum ServeError {
    Security(String),
    Bind(std::io::Error),
}

impl std::fmt::Display for ServeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Security(msg) => write!(f, "security error: {msg}"),
            Self::Bind(e) => write!(f, "bind error: {e}"),
        }
    }
}

impl std::error::Error for ServeError {}

pub fn is_loopback(bind: &str) -> bool {
    bind == "127.0.0.1" || bind == "localhost" || bind == "::1"
}

pub fn find_ui(explicit: Option<PathBuf>, additional_candidates: &[PathBuf]) -> Option<PathBuf> {
    let explicit = explicit.or_else(|| std::env::var_os("ZORP_UI_DIR").map(PathBuf::from));
    if let Some(dir) = explicit {
        if dir.join("index.html").is_file() {
            return Some(dir);
        }
        eprintln!(
            "zorp-web: no index.html in {}; serving the API only",
            dir.display()
        );
        return None;
    }
    let mut candidates = Vec::new();
    candidates.extend(additional_candidates.iter().cloned());
    if let Some(home) = std::env::var_os("HOME") {
        candidates.push(PathBuf::from(home).join(".local/share/zorp/web"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("web"));
        }
    }
    candidates.push(PathBuf::from("web"));
    candidates
        .into_iter()
        .find(|c| c.join("index.html").is_file())
}

pub async fn serve(options: ServeOptions) -> Result<Running, ServeError> {
    if !is_loopback(&options.bind) && options.token.is_none() {
        return Err(ServeError::Security(format!(
            "--bind {} would expose agent-driven shell access to this machine; --token is required with it",
            options.bind
        )));
    }
    let addr_str = format!("{}:{}", options.bind, options.port);
    let listener = tokio::net::TcpListener::bind(&addr_str)
        .await
        .map_err(ServeError::Bind)?;
    let addr = listener.local_addr().map_err(ServeError::Bind)?;

    let ui = find_ui(options.ui_dir.clone(), &options.additional_ui_candidates);
    eprintln!("zorp-web: listening on http://{addr}");
    match &ui {
        Some(dir) => eprintln!("zorp-web: serving the chat UI from {}", dir.display()),
        None => eprintln!(
            "zorp-web: no chat UI found, serving the API only. Install it, or pass --ui-dir."
        ),
    }

    let mut state = state::AppState::with_token(options.token.clone())
        .with_allowed_origins(options.allow_origin.clone())
        .with_own_port(addr.port());
    if let Some(dir) = options.workspace.clone() {
        state = state.with_workspace(dir);
    }
    if let Some(persisted) = settings::load() {
        state.settings.lock().unwrap().load_persisted(persisted);
    }

    match state.workspace() {
        Ok(chosen) => println!(
            "zorp-web: working in {} (from {})",
            chosen.path.display(),
            chosen.source.describe()
        ),
        Err(workspace::Unusable::Unset) => eprintln!(
            "zorp-web: no workspace chosen, so turns are refused until there is one. Pass --workspace, set ZORP_WORKSPACE, or pick a directory in the browser."
        ),
        Err(workspace::Unusable::Refused { source, reason }) => eprintln!(
            "zorp-web: the workspace from {} cannot be used: {reason}",
            source.describe()
        ),
    }

    #[cfg(feature = "recall")]
    {
        state = state.with_recall_indexer(Some(recall::IndexerHandle::start_from_env()));
    }

    let router = api::router_with_ui(state, ui);
    let handle = tokio::spawn(async move { axum::serve(listener, router).await });

    Ok(Running { addr, handle })
}
