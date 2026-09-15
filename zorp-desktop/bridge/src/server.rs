use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::sync_channel;
use std::thread;
use tokio::sync::oneshot;
use zorp_web::{serve, ServeOptions};

use std::sync::Mutex;

static RUNNING: AtomicBool = AtomicBool::new(false);
static STOP_TX: Mutex<Option<oneshot::Sender<()>>> = Mutex::new(None);


pub fn choose_port(preferred: u16) -> u16 {
    let bind_addr = format!("127.0.0.1:{preferred}");
    if TcpListener::bind(&bind_addr).is_ok() {
        preferred
    } else {
        0
    }
}

pub fn start_background_server(
    preferred_port: u16,
    bundle_resource_dir: Option<PathBuf>,
) -> Result<SocketAddr, String> {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return Err("server is already running".to_string());
    }

    let port = choose_port(preferred_port);
    let (addr_tx, addr_rx) = sync_channel::<Result<SocketAddr, String>>(1);
    let (stop_tx, stop_rx) = oneshot::channel::<()>();

    if let Ok(mut guard) = STOP_TX.lock() {
        *guard = Some(stop_tx);
    }


    thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(r) => r,
            Err(e) => {
                let _ = addr_tx.send(Err(format!("failed to initialize tokio runtime: {e}")));
                RUNNING.store(false, Ordering::SeqCst);
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
                    tokio::select! {
                        _ = stop_rx => {
                            // Stop requested
                        }
                        res = running.handle => {
                            if let Err(e) = res {
                                eprintln!("zorp server exited with error: {e}");
                            }
                        }
                    }
                }
                Err(e) => {
                    let _ = addr_tx.send(Err(format!("server failed to bind: {e}")));
                }
            }
        });

        RUNNING.store(false, Ordering::SeqCst);
    });

    match addr_rx.recv() {
        Ok(Ok(addr)) => Ok(addr),
        Ok(Err(e)) => Err(e),
        Err(_) => Err("background server startup hung".to_string()),
    }
}

pub fn stop_server() {
    if let Ok(mut guard) = STOP_TX.lock() {
        if let Some(tx) = guard.take() {
            let _ = tx.send(());
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choose_port_falls_back_when_preferred_is_held() {
        let _guard = TcpListener::bind("127.0.0.1:17777").expect("should bind test port");
        assert_eq!(choose_port(17777), 0);
    }
}
