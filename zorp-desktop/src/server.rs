use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::thread;
use zorp_web::{serve, ServeOptions};

/// Chooses the port to listen on: port 7777 if available, otherwise falls back
/// to port 0 (ephemeral port chosen by the OS).
pub fn choose_port() -> u16 {
    if TcpListener::bind("127.0.0.1:7777").is_ok() {
        7777
    } else {
        eprintln!("zorp-desktop: port 7777 is occupied; falling back to ephemeral port 0");
        0
    }
}

/// Spawns the `zorp-web` server on a background thread with its own Tokio runtime.
/// Delivers the resolved bound `SocketAddr` synchronously before returning.
pub fn start_background_server(
    port: u16,
    bundle_resource_dir: Option<PathBuf>,
) -> Result<(SocketAddr, Receiver<Result<(), String>>), String> {
    let (addr_tx, addr_rx) = std::sync::mpsc::sync_channel::<Result<SocketAddr, String>>(1);
    let (err_tx, err_rx) = channel::<Result<(), String>>();

    thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(r) => r,
            Err(e) => {
                let _ = addr_tx.send(Err(format!("failed to initialize tokio runtime: {e}")));
                return;
            }
        };

        rt.block_on(async move {
            let mut additional_candidates = Vec::new();
            if let Some(ref res) = bundle_resource_dir {
                additional_candidates.push(res.join("_up_").join("web"));
                additional_candidates.push(res.join("web"));
                additional_candidates.push(res.clone());
            }
            additional_candidates.push(PathBuf::from("../web"));
            additional_candidates.push(PathBuf::from("web"));

            let options = ServeOptions {
                bind: "127.0.0.1".to_string(),
                port,
                token: None,
                ui_dir: None,
                workspace: None,
                allow_origin: Vec::new(),
                additional_ui_candidates: additional_candidates,
            };

            match serve(options).await {
                Ok(running) => {
                    let _ = addr_tx.send(Ok(running.addr));
                    if let Err(e) = running.handle.await {
                        let _ = err_tx.send(Err(format!("server task failed: {e}")));
                    } else {
                        let _ = err_tx.send(Ok(()));
                    }
                }
                Err(e) => {
                    let _ = addr_tx.send(Err(format!("server failed to bind: {e}")));
                }
            }
        });
    });

    match addr_rx.recv() {
        Ok(Ok(addr)) => Ok((addr, err_rx)),
        Ok(Err(e)) => Err(e),
        Err(_) => Err("background server thread hung during startup".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choose_port_falls_back_when_7777_is_held() {
        let _guard = TcpListener::bind("127.0.0.1:7777").expect("should bind 7777 for test");
        assert_eq!(choose_port(), 0);
    }
}
