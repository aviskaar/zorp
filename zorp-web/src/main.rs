use clap::Parser;
use std::path::PathBuf;
use zorp_web::api;

#[derive(Parser)]
#[command(version, about = "Local web UI for the zorp agent")]
struct Cli {
    /// Interface to listen on. Anything other than loopback requires --token,
    /// because a reachable zorp-web is agent-driven shell access to this
    /// machine.
    #[arg(long, default_value = "127.0.0.1")]
    bind: String,
    #[arg(long, default_value_t = 7777)]
    port: u16,
    /// Shared secret, required when binding to a non-loopback interface.
    #[arg(long)]
    token: Option<String>,
    /// Directory holding the chat UI's static files. Found automatically
    /// when installed. Also settable with ZORP_UI_DIR, the same variable
    /// install.sh uses to choose where to put them.
    #[arg(long)]
    ui_dir: Option<PathBuf>,
    /// Directory the agent works in, and the one the artifact pane serves
    /// from. Beats ZORP_WORKSPACE and whatever was last chosen in the
    /// browser. Nothing is assumed when none of the three is set: the
    /// server starts, serves the UI, and refuses to run work until a
    /// directory is chosen.
    #[arg(long)]
    workspace: Option<PathBuf>,
    /// Origin a browser may call the API from, repeatable. Needed only when
    /// the UI is served from somewhere other than this server, which is the
    /// container split. Pass `null` for an index.html opened off disk.
    ///
    /// Nothing is allowed by default. A page served by this server shares
    /// its origin and needs no entry here; naming an origin is how a
    /// different one gets in, and until it is named it cannot drive the
    /// agent.
    #[arg(long = "allow-origin", value_name = "ORIGIN")]
    allow_origin: Vec<String>,
}

fn is_loopback(bind: &str) -> bool {
    bind == "127.0.0.1" || bind == "localhost" || bind == "::1"
}

/// Where the UI's files are, in the order worth trying.
///
/// A directory only counts if `index.html` is actually in it. An empty or
/// half-written directory that resolves and then serves 404s is the failure
/// this whole function exists to avoid.
fn find_ui(explicit: Option<PathBuf>) -> Option<PathBuf> {
    let explicit = explicit.or_else(|| std::env::var_os("ZORP_UI_DIR").map(PathBuf::from));
    if let Some(dir) = explicit {
        // Asked for by name: if it is wrong, say so rather than silently
        // falling through to a different UI than the one requested.
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
    // Where install.sh puts it.
    if let Some(home) = std::env::var_os("HOME") {
        candidates.push(PathBuf::from(home).join(".local/share/zorp/web"));
    }
    // Beside the binary, which is how the release tarball is laid out if
    // someone runs it straight out of the extracted directory.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("web"));
        }
    }
    // Running from a checkout.
    candidates.push(PathBuf::from("web"));
    candidates
        .into_iter()
        .find(|c| c.join("index.html").is_file())
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if !is_loopback(&cli.bind) && cli.token.is_none() {
        eprintln!(
            "zorp-web: --bind {} would expose agent-driven shell access to this \
             machine; --token is required with it",
            cli.bind
        );
        std::process::exit(2);
    }
    let addr = format!("{}:{}", cli.bind, cli.port);
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("zorp-web: cannot bind {addr}: {e}");
            std::process::exit(1);
        }
    };
    let ui = find_ui(cli.ui_dir.clone());
    eprintln!("zorp-web: listening on http://{addr}");
    match &ui {
        Some(dir) => eprintln!("zorp-web: serving the chat UI from {}", dir.display()),
        // Said out loud. Silence here is how "open this URL" turned into a
        // 404 that looked like a broken install rather than a missing flag.
        None => eprintln!(
            "zorp-web: no chat UI found, serving the API only. \
             Install it, or pass --ui-dir."
        ),
    }
    let mut state = zorp_web::state::AppState::with_token(cli.token.clone())
        .with_allowed_origins(cli.allow_origin.clone())
        .with_own_port(cli.port);
    if let Some(dir) = cli.workspace.clone() {
        state = state.with_workspace(dir);
    }
    // Restore whatever model settings were last saved through the UI. Only
    // done here, not inside `AppState::new`/`with_token` themselves, so the
    // test suite stays hermetic against whatever a developer's own machine
    // happens to have in ~/.config/zorp/web.toml.
    //
    // Before the workspace is reported, because the saved workspace path
    // comes out of this same file.
    if let Some(persisted) = zorp_web::settings::load() {
        state.settings.lock().unwrap().load_persisted(persisted);
    }
    // Said out loud either way. The agent writes files, and a person has to
    // know where. Nothing is guessed when nobody chose: the server runs and
    // refuses work until one is picked, because falling back to whatever
    // directory this process happens to be in is how zorp's own source tree
    // filled up with rendered PDFs.
    match state.workspace() {
        Ok(chosen) => println!(
            "zorp-web: working in {} (from {})",
            chosen.path.display(),
            chosen.source.describe()
        ),
        Err(zorp_web::workspace::Unusable::Unset) => eprintln!(
            "zorp-web: no workspace chosen, so turns are refused until there is one. \
             Pass --workspace, set ZORP_WORKSPACE, or pick a directory in the browser."
        ),
        Err(zorp_web::workspace::Unusable::Refused { source, reason }) => eprintln!(
            "zorp-web: the workspace from {} cannot be used: {reason}",
            source.describe()
        ),
    }
    // Starting the worker only creates a thread and returns. Its first sweep
    // runs there, never in front of binding the server or accepting a turn.
    #[cfg(feature = "recall")]
    {
        state = state.with_recall_indexer(Some(zorp_web::recall::IndexerHandle::start_from_env()));
    }
    if let Err(e) = axum::serve(listener, api::router_with_ui(state, ui)).await {
        eprintln!("zorp-web: server error: {e}");
        std::process::exit(1);
    }
}
