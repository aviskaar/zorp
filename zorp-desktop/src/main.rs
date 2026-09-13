#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod env;
mod server;

use std::path::PathBuf;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

fn main() {
    env::repair_path();

    let port = server::choose_port();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .setup(move |app| {
            let resource_dir: Option<PathBuf> = app.path().resource_dir().ok();
            let (addr, _err_rx) = match server::start_background_server(port, resource_dir) {
                Ok(res) => res,
                Err(err) => {
                    eprintln!("zorp-desktop fatal error: {err}");
                    std::process::exit(1);
                }
            };

            let target_url = format!("http://127.0.0.1:{}/", addr.port());
            let url: url::Url = target_url.parse().expect("valid loopback url");

            let init_script = "document.documentElement.classList.add('desktop');";

            let win = WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url))
                .title("Zorp")
                .inner_size(1200.0, 800.0)
                .min_inner_size(800.0, 600.0)
                .initialization_script(init_script)
                .build()?;

            let _ = win.show();
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running zorp desktop application");
}
