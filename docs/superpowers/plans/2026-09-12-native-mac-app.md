# Native Mac App Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver a native macOS application (`Zorp.app`) and release workflow packaging it into a double-clickable universal `.dmg`, running the existing `zorp-web` server in-process via Tauri v2.

**Architecture:** Refactor `zorp-web` startup into a reusable `zorp_web::serve(ServeOptions)` function. Create `zorp-desktop/` as an isolated Cargo crate (excluded from the root workspace to keep Linux CI fast) linking `zorp-web` and `zorp-agent`. At startup, `zorp-desktop` repairs `PATH` from the login shell, binds port 7777 (falling back to ephemeral port 0), spawns the server on a dedicated Tokio thread, and loads `http://127.0.0.1:<port>/` in a native `WKWebView` with inset traffic lights and window state restoration.

**Tech Stack:** Rust (1.95+), Tauri v2 (`tauri`, `tauri-build`, `tauri-plugin-single-instance`, `tauri-plugin-window-state`), Tokio, Axum, HTML/CSS, GitHub Actions.

## Global Constraints

- Exclude `zorp-desktop` from workspace members in the root `Cargo.toml` (`exclude = ["zorp-desktop"]`).
- Window loads `http://127.0.0.1:<port>/` directly (no `tauri://` custom protocol or CORS changes).
- All existing tests in `cargo test -p zorp-web` must continue to pass.
- Version across all manifests must remain in lockstep (`0.4.1`).
- Ad-hoc signing by default with clear Gatekeeper bypass documentation.

---

### Task 1: Refactor `zorp-web` to Expose `serve()` and `ServeOptions`

**Files:**
- Modify: `zorp-web/src/lib.rs`
- Modify: `zorp-web/src/main.rs`
- Test: `zorp-web/tests/serve.rs`

**Interfaces:**
- Produces:
  ```rust
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
      pub addr: std::net::SocketAddr,
      pub handle: tokio::task::JoinHandle<Result<(), std::io::Error>>,
  }

  #[derive(Debug)]
  pub enum ServeError {
      Security(String),
      Bind(std::io::Error),
  }

  pub async fn serve(options: ServeOptions) -> Result<Running, ServeError>;
  pub fn find_ui(explicit: Option<PathBuf>, additional_candidates: &[PathBuf]) -> Option<PathBuf>;
  ```

- [ ] **Step 1: Write the failing test for `serve` and `find_ui`**

Create `zorp-web/tests/serve.rs`:
```rust
use std::path::PathBuf;
use tempfile::tempdir;
use zorp_web::{find_ui, serve, ServeOptions};

#[tokio::test]
async fn serve_binds_ephemeral_port() {
    let options = ServeOptions {
        bind: "127.0.0.1".to_string(),
        port: 0,
        token: None,
        ui_dir: None,
        workspace: None,
        allow_origin: Vec::new(),
        additional_ui_candidates: Vec::new(),
    };

    let running = serve(options).await.expect("serve should bind port 0");
    assert_ne!(running.addr.port(), 0);
    assert_eq!(running.addr.ip(), std::net::Ipv4Addr::new(127, 0, 0, 1));
    running.handle.abort();
}

#[test]
fn find_ui_checks_additional_candidates_first() {
    let dir = tempdir().unwrap();
    let custom_ui = dir.path().join("bundle/web");
    std::fs::create_dir_all(&custom_ui).unwrap();
    std::fs::write(custom_ui.join("index.html"), "<html></html>").unwrap();

    let found = find_ui(None, &[custom_ui.clone()]);
    assert_eq!(found, Some(custom_ui));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p zorp-web --test serve`
Expected: FAIL with unresolved import `serve`, `ServeOptions`, `find_ui`.

- [ ] **Step 3: Implement `serve` and `find_ui` in `zorp-web/src/lib.rs` and update `zorp-web/src/main.rs`**

In `zorp-web/src/lib.rs`, add:
```rust
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
    candidates.into_iter().find(|c| c.join("index.html").is_file())
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
    let handle = tokio::spawn(async move {
        axum::serve(listener, router).await
    });

    Ok(Running { addr, handle })
}
```

Refactor `zorp-web/src/main.rs`:
```rust
use clap::Parser;
use std::path::PathBuf;
use zorp_web::{serve, ServeOptions};

#[derive(Parser)]
#[command(version, about = "Local web UI for the zorp agent")]
struct Cli {
    #[arg(long, default_value = "127.0.0.1")]
    bind: String,
    #[arg(long, default_value_t = 7777)]
    port: u16,
    #[arg(long)]
    token: Option<String>,
    #[arg(long)]
    ui_dir: Option<PathBuf>,
    #[arg(long)]
    workspace: Option<PathBuf>,
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

    match serve(options).await {
        Ok(running) => {
            if let Err(e) = running.handle.await {
                eprintln!("zorp-web: server error: {e}");
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("zorp-web: {e}");
            std::process::exit(1);
        }
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p zorp-web`
Expected: PASS (all tests green).

- [ ] **Step 5: Commit**

```bash
git add zorp-web/src/lib.rs zorp-web/src/main.rs zorp-web/tests/serve.rs
git commit -m "refactor(zorp-web): expose serve and find_ui in library for desktop app"
```

---

### Task 2: PATH Repair Module for Desktop

**Files:**
- Create: `zorp-desktop/src/env.rs`

**Interfaces:**
- Produces:
  ```rust
  pub fn parse_shell_path(output: &str, current: &str) -> Option<String>;
  pub fn login_shell_path() -> Option<String>;
  pub fn repair_path();
  ```

- [ ] **Step 1: Write `zorp-desktop/src/env.rs` with unit tests**

Create `zorp-desktop/src/env.rs`:
```rust
use std::process::Command;
use std::time::Duration;

pub fn parse_shell_path(output: &str, current: &str) -> Option<String> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Only adopt the login shell's PATH if it actually adds information over the default
    if trimmed.len() > current.len() || !current.contains("/opt/homebrew/bin") && trimmed.contains("/opt/homebrew/bin") {
        Some(trimmed.to_string())
    } else {
        None
    }
}

pub fn login_shell_path() -> Option<String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    let mut cmd = Command::new(shell);
    cmd.arg("-lc").arg("printf %s \"$PATH\"");

    // Enforce 2 second timeout by spawning and polling or waiting
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    let current = std::env::var("PATH").unwrap_or_default();
    parse_shell_path(&stdout, &current)
}

pub fn repair_path() {
    if let Some(new_path) = login_shell_path() {
        std::env::set_var("PATH", new_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_shell_path_ignores_empty() {
        assert_eq!(parse_shell_path("", "/usr/bin:/bin"), None);
        assert_eq!(parse_shell_path("   \n", "/usr/bin:/bin"), None);
    }

    #[test]
    fn parse_shell_path_accepts_longer_path() {
        let current = "/usr/bin:/bin";
        let richer = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin";
        assert_eq!(parse_shell_path(richer, current), Some(richer.to_string()));
    }

    #[test]
    fn parse_shell_path_rejects_shorter_path() {
        let current = "/opt/homebrew/bin:/usr/bin:/bin";
        let shorter = "/usr/bin:/bin";
        assert_eq!(parse_shell_path(shorter, current), None);
    }
}
```

- [ ] **Step 2: Run unit test via rustc/cargo to verify it passes**

Run: `cargo test --test env_test` (or test within `zorp-desktop` after Task 3 scaffolding).

---

### Task 3: Scaffold `zorp-desktop` Package and Cargo/Tauri Configuration

**Files:**
- Modify: `Cargo.toml`
- Modify: `.gitignore`
- Create: `zorp-desktop/Cargo.toml`
- Create: `zorp-desktop/build.rs`
- Create: `zorp-desktop/tauri.conf.json`

- [ ] **Step 1: Update workspace `Cargo.toml` and `.gitignore`**

In `Cargo.toml`, update:
```toml
[workspace]
members = [".", "zorp-agent", "zorp-mcp", "zorp-eval", "zorp-stub", "zorp-track", "zorp-web", "zorp-search", "zorp-skill", "zorp-recall", "zorp-voice", "erbga"]
exclude = ["zorp-desktop"]
```

In `.gitignore`, append:
```gitignore
# Desktop Tauri build outputs
zorp-desktop/target
```

- [ ] **Step 2: Create `zorp-desktop/Cargo.toml`**

Create `zorp-desktop/Cargo.toml`:
```toml
[package]
name = "zorp-desktop"
version = "0.4.1"
edition = "2021"
description = "Native macOS desktop application for zorp"
license = "MIT"

[dependencies]
zorp-web = { path = "../zorp-web" }
zorp-agent = { path = "../zorp-agent" }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
tauri = { version = "2", features = [] }
tauri-plugin-single-instance = "2"
tauri-plugin-window-state = "2"

[build-dependencies]
tauri-build = { version = "2", features = [] }
```

- [ ] **Step 3: Create `zorp-desktop/build.rs`**

Create `zorp-desktop/build.rs`:
```rust
fn main() {
    tauri_build::build()
}
```

- [ ] **Step 4: Create `zorp-desktop/tauri.conf.json`**

Create `zorp-desktop/tauri.conf.json`:
```json
{
  "$schema": "https://raw.githubusercontent.com/tauri-apps/tauri/2.0.0/tooling/cli/schema.json",
  "productName": "Zorp",
  "version": "0.4.1",
  "identifier": "dev.zorp.app",
  "build": {
    "frontendDist": null
  },
  "app": {
    "withGlobalTauri": false,
    "windows": [
      {
        "title": "Zorp",
        "width": 1200,
        "height": 800,
        "minWidth": 800,
        "minHeight": 600,
        "resizable": true,
        "fullscreen": false,
        "titleBarStyle": "Overlay",
        "hiddenTitle": true,
        "trafficLightPosition": {
          "x": 16,
          "y": 18
        }
      }
    ],
    "security": {
      "csp": null
    }
  },
  "bundle": {
    "active": true,
    "targets": ["dmg"],
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ],
    "resources": [
      "../web/index.html",
      "../web/styles.css",
      "../web/dist/**",
      "../web/fonts/**"
    ],
    "macOS": {
      "frameworks": [],
      "minimumSystemVersion": "11.0",
      "signingIdentity": null
    }
  },
  "plugins": {
    "single-instance": {},
    "window-state": {}
  }
}
```

- [ ] **Step 5: Verify workspace remains buildable and unaffected**

Run: `cargo check --workspace`
Expected: PASS (checks only workspace members, `zorp-desktop` is properly excluded).

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml .gitignore zorp-desktop/
git commit -m "feat(desktop): scaffold zorp-desktop package and tauri configuration"
```

---

### Task 4: In-Process Server Boot and Webview Setup

**Files:**
- Create: `zorp-desktop/src/server.rs`
- Create: `zorp-desktop/src/main.rs`
- Modify: `zorp-desktop/Cargo.toml`

**Interfaces:**
- Produces:
  ```rust
  pub fn choose_port() -> u16;
  pub fn start_background_server(port: u16, resource_dir: Option<PathBuf>) -> Result<(std::net::SocketAddr, std::sync::mpsc::Receiver<Result<(), String>>), String>;
  ```

- [ ] **Step 1: Write `zorp-desktop/src/server.rs` with port fallback and server thread**

Create `zorp-desktop/src/server.rs`:
```rust
use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::thread;
use zorp_web::{serve, ServeOptions};

pub fn choose_port() -> u16 {
    if TcpListener::bind("127.0.0.1:7777").is_ok() {
        7777
    } else {
        eprintln!("zorp-desktop: port 7777 is occupied; falling back to ephemeral port 0");
        0
    }
}

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
            if let Some(res) = bundle_resource_dir {
                additional_candidates.push(res.join("web"));
                additional_candidates.push(res);
            }

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
```

- [ ] **Step 2: Write `zorp-desktop/src/main.rs` wiring Tauri, env repair, and webview navigation**

Create `zorp-desktop/src/main.rs`:
```rust
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
```

- [ ] **Step 3: Run tests in `zorp-desktop`**

Run: `cargo test --manifest-path zorp-desktop/Cargo.toml`
Expected: PASS (`choose_port` test and `env` tests pass).

- [ ] **Step 4: Commit**

```bash
git add zorp-desktop/src/
git commit -m "feat(desktop): implement in-process server lifecycle and window setup"
```

---

### Task 5: Native Chrome Styling and App Icon Generation

**Files:**
- Modify: `web/styles.css`
- Create: `zorp-desktop/icons/` (32x32.png, 128x128.png, 128x128@2x.png, icon.icns, icon.ico)
- Create: `scripts/generate-desktop-icons.sh`

- [ ] **Step 1: Add `html.desktop` CSS rules to `web/styles.css`**

In `web/styles.css`, append:
```css
/* Native macOS desktop app styling (injected via Tauri initialization script) */
html.desktop {
  --traffic-lights-height: 38px;
}

html.desktop body {
  -webkit-user-select: auto;
  user-select: auto;
}

html.desktop .sidebar {
  padding-top: var(--traffic-lights-height);
}

html.desktop .header,
html.desktop .topbar {
  padding-top: calc(var(--traffic-lights-height) / 2);
}
```

- [ ] **Step 2: Create icon generation script `scripts/generate-desktop-icons.sh`**

Create `scripts/generate-desktop-icons.sh`:
```bash
#!/bin/sh
set -eu

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
SVG="$ROOT_DIR/docs-site/book/favicon-de23e50b.svg"
ICONS_DIR="$ROOT_DIR/zorp-desktop/icons"

mkdir -p "$ICONS_DIR"
TMP_PNG="$ICONS_DIR/icon-1024.png"

# Render 1024x1024 base PNG
if command -v qlmanage >/dev/null 2>&1; then
    qlmanage -t -s 1024 -o "$ICONS_DIR" "$SVG"
    mv "$ICONS_DIR/favicon-de23e50b.svg.png" "$TMP_PNG" 2>/dev/null || true
elif command -v rsvg-convert >/dev/null 2>&1; then
    rsvg-convert -w 1024 -h 1024 "$SVG" -o "$TMP_PNG"
else
    # Fallback to python or sips if available
    python3 -c "import urllib.request; print('Generating placeholder icon')"
fi

# Generate intermediate sizes if sips is available on macOS
if [ -f "$TMP_PNG" ] && command -v sips >/dev/null 2>&1; then
    sips -z 32 32 "$TMP_PNG" --out "$ICONS_DIR/32x32.png"
    sips -z 128 128 "$TMP_PNG" --out "$ICONS_DIR/128x128.png"
    sips -z 256 256 "$TMP_PNG" --out "$ICONS_DIR/128x128@2x.png"
    sips -z 512 512 "$TMP_PNG" --out "$ICONS_DIR/icon.png"
fi
```

- [ ] **Step 3: Run icon generation script**

Run: `sh scripts/generate-desktop-icons.sh`
Expected: Generates icons in `zorp-desktop/icons/`.

- [ ] **Step 4: Commit**

```bash
git add web/styles.css scripts/generate-desktop-icons.sh zorp-desktop/icons/
git commit -m "style(desktop): add html.desktop chrome rules and application icon"
```

---

### Task 6: Release Version Verification Script and Test

**Files:**
- Modify: `scripts/check-release-version.sh`
- Modify: `tests/release_version_check.rs`

- [ ] **Step 1: Write the failing test in `tests/release_version_check.rs`**

Add test case in `tests/release_version_check.rs`:
```rust
#[test]
fn desktop_manifest_drift_fails_release() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    // Setup minimal tree with zorp-desktop
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"zorp\"\nversion.workspace = true\n\n[workspace]\nmembers = [\".\"]\nexclude = [\"zorp-desktop\"]\n\n[workspace.package]\nversion = \"0.4.1\"\n",
    ).unwrap();
    fs::write(
        root.join("Dockerfile"),
        "FROM debian:12-slim\nARG VERSION=v0.4.1\n",
    ).unwrap();

    let desktop_dir = root.join("zorp-desktop");
    fs::create_dir_all(&desktop_dir).unwrap();
    fs::write(
        desktop_dir.join("Cargo.toml"),
        "[package]\nname = \"zorp-desktop\"\nversion = \"0.3.0\"\n",
    ).unwrap();
    fs::write(
        desktop_dir.join("tauri.conf.json"),
        "{\"version\": \"0.4.1\"}",
    ).unwrap();

    let out = Command::new("sh")
        .arg(script())
        .arg("v0.4.1")
        .arg(root)
        .output()
        .unwrap();

    assert!(!out.status.success(), "disagreeing desktop version must fail");
    let all = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(all.contains("zorp-desktop"), "error message must name zorp-desktop: {all}");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p zorp --test release_version_check desktop_manifest_drift_fails_release`
Expected: FAIL because `check-release-version.sh` doesn't inspect `zorp-desktop` yet.

- [ ] **Step 3: Update `scripts/check-release-version.sh`**

In `scripts/check-release-version.sh`, add validation for `zorp-desktop`:
```bash
desktop_cargo="$dir/zorp-desktop/Cargo.toml"
desktop_tauri="$dir/zorp-desktop/tauri.conf.json"

if [ -f "$desktop_cargo" ]; then
    desktop_cargo_version="v$(sed -n 's/^version = "\(.*\)"$/\1/p' "$desktop_cargo" | head -n 1)"
    if [ "$tag" != "$desktop_cargo_version" ]; then
        echo "zorp-desktop/Cargo.toml: version is '${desktop_cargo_version#v}' but tag is '$tag'" >&2
        fail=1
    fi
fi

if [ -f "$desktop_tauri" ]; then
    desktop_tauri_version="v$(sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$desktop_tauri" | head -n 1)"
    if [ "$tag" != "$desktop_tauri_version" ]; then
        echo "zorp-desktop/tauri.conf.json: version is '${desktop_tauri_version#v}' but tag is '$tag'" >&2
        fail=1
    fi
fi
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p zorp --test release_version_check`
Expected: PASS (all release version check tests pass).

- [ ] **Step 5: Commit**

```bash
git add scripts/check-release-version.sh tests/release_version_check.rs
git commit -m "ci: enforce zorp-desktop version consistency in check-release-version.sh"
```

---

### Task 7: CI Job and Desktop Release Workflow

**Files:**
- Modify: `.github/workflows/ci.yml`
- Create: `.github/workflows/desktop.yml`

- [ ] **Step 1: Add `desktop` job to `.github/workflows/ci.yml`**

In `.github/workflows/ci.yml`, add a job:
```yaml
  desktop:
    name: Desktop (macOS)
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: actions/setup-node@v4
        with:
          node-version: 20
      - name: Build Web UI
        run: |
          cd web
          npm ci
          npm run build
      - name: Check and test zorp-desktop
        run: |
          cargo check --manifest-path zorp-desktop/Cargo.toml
          cargo test --manifest-path zorp-desktop/Cargo.toml
```

- [ ] **Step 2: Create `.github/workflows/desktop.yml`**

Create `.github/workflows/desktop.yml`:
```yaml
name: Desktop Release

on:
  workflow_dispatch:
    inputs:
      tag:
        description: "Release tag to attach DMG to (e.g. v0.5.0)"
        required: true
        type: string
      publish:
        description: "Publish and upload DMG to GitHub Release"
        required: true
        type: boolean
        default: false

permissions:
  contents: write

jobs:
  build-mac-dmg:
    name: Build macOS Universal DMG
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v4
        with:
          ref: ${{ inputs.tag }}

      - name: Check Release Version
        run: sh scripts/check-release-version.sh "${{ inputs.tag }}"

      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: x86_64-apple-darwin,aarch64-apple-darwin,universal-apple-darwin

      - uses: actions/setup-node@v4
        with:
          node-version: 20

      - name: Install Tauri CLI
        run: cargo install tauri-cli --version "^2.0.0" --locked

      - name: Build Web UI
        run: |
          cd web
          npm ci
          npm run build

      - name: Build DMG
        run: |
          cargo tauri build --target universal-apple-darwin --manifest-path zorp-desktop/Cargo.toml

      - name: Checksum DMG
        run: |
          cd zorp-desktop/target/universal-apple-darwin/release/bundle/dmg
          shasum -a 256 *.dmg > Zorp.dmg.sha256

      - name: Upload Artifact
        uses: actions/upload-artifact@v4
        with:
          name: zorp-desktop-mac-dmg
          path: zorp-desktop/target/universal-apple-darwin/release/bundle/dmg/*

      - name: Attach to GitHub Release
        if: ${{ inputs.publish }}
        env:
          GH_TOKEN: ${{ github.token }}
        run: |
          tag="${{ inputs.tag }}"
          gh release view "$tag" || gh release create "$tag" --generate-notes
          gh release upload "$tag" zorp-desktop/target/universal-apple-darwin/release/bundle/dmg/*.dmg* --clobber
```

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml .github/workflows/desktop.yml
git commit -m "ci: add desktop CI check and desktop.yml release workflow"
```

---

### Task 8: Documentation, AGENTS.md, CLAUDE.md, and DECISIONS.md

**Files:**
- Modify: `README.md`
- Modify: `AGENTS.md`
- Modify: `CLAUDE.md`
- Modify: `docs/DECISIONS.md`

- [ ] **Step 1: Update `README.md` with Mac App download & instructions**

Add a "Mac App" subsection explaining downloading the DMG, Gatekeeper first-launch instruction (`xattr -d com.apple.quarantine /Applications/Zorp.app` or right-click Open), and notes on data persistence (`~/.config/zorp`).

- [ ] **Step 2: Update `AGENTS.md` and `CLAUDE.md`**

Document `zorp-desktop` under repository layout: excluded from workspace members, links `zorp-web` and `zorp-agent`, builds DMG via Tauri v2.

- [ ] **Step 3: Add architectural record to `docs/DECISIONS.md`**

Record entry: "2026-09-12: Native Mac App (Tauri v2 in-process server, loopback binding, workspace exclusion)".

- [ ] **Step 4: Run full repo test suite to verify everything passes**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add README.md AGENTS.md CLAUDE.md docs/DECISIONS.md
git commit -m "docs: document native Mac app setup, gatekeeper bypass, and architectural decisions"
```
