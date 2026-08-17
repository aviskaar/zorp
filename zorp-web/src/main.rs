use clap::Parser;
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
}

fn is_loopback(bind: &str) -> bool {
    bind == "127.0.0.1" || bind == "localhost" || bind == "::1"
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
    eprintln!("zorp-web: listening on http://{addr}");
    let state = zorp_web::state::AppState::with_token(cli.token.clone());
    if let Err(e) = axum::serve(listener, api::router_with_state(state)).await {
        eprintln!("zorp-web: server error: {e}");
        std::process::exit(1);
    }
}
