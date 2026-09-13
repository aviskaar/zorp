use clap::Parser;
use std::path::PathBuf;
use zorp_web::{serve, ServeError, ServeOptions};

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

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let options = ServeOptions {
        bind: cli.bind,
        port: cli.port,
        token: cli.token,
        ui_dir: cli.ui_dir,
        workspace: cli.workspace,
        allow_origin: cli.allow_origin,
        additional_ui_candidates: Vec::new(),
    };

    let running = match serve(options).await {
        Ok(r) => r,
        Err(ServeError::Security(msg)) => {
            eprintln!("zorp-web: {msg}");
            std::process::exit(2);
        }
        Err(ServeError::Bind(e)) => {
            eprintln!("zorp-web: cannot bind: {e}");
            std::process::exit(1);
        }
    };

    if let Err(e) = running.handle.await {
        eprintln!("zorp-web: server error: {e}");
        std::process::exit(1);
    }
}
