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
    // The artifact pane reads from the directory the server was started in,
    // which is the directory the agent already works in. If that cannot be
    // determined the pane is simply switched off, which is a better answer
    // than picking somewhere and serving from it.
    let workspace = std::env::current_dir().ok();
    match &workspace {
        Some(dir) => println!("zorp-web: serving artifacts from {}", dir.display()),
        None => eprintln!(
            "zorp-web: could not determine the working directory, \
             so the artifact pane is switched off"
        ),
    }
    let mut state = zorp_web::state::AppState::with_token(cli.token.clone())
        .with_allowed_origins(cli.allow_origin.clone());
    if let Some(dir) = workspace {
        state = state.with_workspace(dir);
    }
    // Restore whatever model settings were last saved through the UI. Only
    // done here, not inside `AppState::new`/`with_token` themselves, so the
    // test suite stays hermetic against whatever a developer's own machine
    // happens to have in ~/.config/zorp/web.toml.
    if let Some(persisted) = zorp_web::settings::load() {
        state.settings.lock().unwrap().load_persisted(persisted);
    }
    if let Err(e) = axum::serve(listener, api::router_with_ui(state, ui)).await {
        eprintln!("zorp-web: server error: {e}");
        std::process::exit(1);
    }
}
