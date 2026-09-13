# Native Mac App for Zorp (Zorp.app)

Date: 2026-09-12. Status: validated design (Issue #238).

## Overview

A native macOS application for zorp: `Zorp.app`, double-clickable, in the Dock, shipped as a universal `.dmg` on GitHub releases. It hosts the existing web UI in a native window, with the `zorp-web` server running inside the same process on a dedicated Tokio runtime. No terminal launch, no `install.sh`, and no manual navigation to `http://127.0.0.1:7777`.

---

## Key Design Principles & Architecture

```
                    ┌────────────────────────────────────────┐
                    │                Zorp.app                │
                    │                                        │
                    │   ┌────────────────────────────────┐   │
                    │   │        Native Window           │   │
                    │   │   - WKWebView (System)         │   │
                    │   │   - TitleBarStyle: Overlay     │   │
                    │   │   - Inset traffic lights       │   │
                    │   │   - URL: http://127.0.0.1:port │   │
                    │   └───────────────▲────────────────┘   │
                    │                   │ HTTP / SSE         │
                    │   ┌───────────────┴────────────────┐   │
                    │   │      In-Process Server         │   │
                    │   │   - tokio runtime on thread    │   │
                    │   │   - zorp_web::serve(...)       │   │
                    │   │   - Port 7777 -> fallback 0    │   │
                    │   │   - Shared ~/.config/zorp      │   │
                    │   └────────────────────────────────┘   │
                    │                   │                    │
                    │   ┌───────────────▼────────────────┐   │
                    │   │      Repaired Environment      │   │
                    │   │   - PATH from login shell      │   │
                    │   │   - Single-instance mutex      │   │
                    │   │   - Window state persistence   │   │
                    │   └────────────────────────────────┘   │
                    └────────────────────────────────────────┘
```

### 1. Tauri v2 (No Electron, No External Sidecar)
- **Single Process, Single Binary**: The desktop wrapper links `zorp-web` and `zorp-agent` as Rust libraries. One process starts, binds the loopback server, and manages the window. No orphan background servers on unexpected quit.
- **Lightweight**: Uses Apple's system `WKWebView` (~10–15 MB bundle size instead of 150+ MB with Electron).
- **Toolchain**: Builds with macOS Command Line Tools alone (no full Xcode required).
- **Workspace Isolation**: `zorp-desktop/` is its own Cargo project with a separate `Cargo.lock`, excluded from the root `Cargo.toml` (`exclude = ["zorp-desktop"]`). This prevents `wry`, `tao`, `objc2`, and desktop dependencies from slowing down Linux CI or breaking `cargo test --workspace`.

### 2. Same-Origin Loopback Binding
- The webview loads `http://127.0.0.1:<port>/` directly rather than a custom `tauri://` origin.
- Eliminates CORS configurations, API token handshakes, or changes to API fetch paths in `web/`.
- Binding strategy: Attempts port `7777` first (maintaining `localStorage` compatibility with CLI `zorp-web`). If occupied (e.g. CLI is running), it binds port `0` (kernel ephemeral assignment) and logs the chosen port.
- Native error dialog: If socket binding fails entirely, the app presents a native error dialog and exits cleanly instead of showing an unresponsive blank screen.

### 3. PATH Environment Repair (`env.rs`)
- Finder/Dock launched applications inherit a stripped environment (`/usr/bin:/bin:/usr/sbin:/sbin`). CLI utilities such as `git` (Homebrew), `ollama`, `python3`, and `node` are not in that default PATH.
- At launch (before starting the server or agent tools), `zorp-desktop` executes `$SHELL -lc 'printf %s "$PATH"'` (falling back to `/bin/zsh`) with a 2-second timeout.
- If the output is non-empty and longer than the current process PATH, `std::env::set_var("PATH", ...)` updates the process environment so all shell commands spawned by the agent inherit full tool availability.

### 4. Native Look and Chrome Integration
- `tauri.conf.json` sets `titleBarStyle: Overlay`, `hiddenTitle: true`, and insets traffic lights with `trafficLightPosition: { x: 16, y: 18 }`.
- Tauri initialization script executes `document.documentElement.classList.add('desktop')` before page load.
- In `web/styles.css`, targeted rules under `html.desktop` add padding to the sidebar top and header so inset traffic lights do not collide with the wordmark or navigation controls. In an ordinary browser, `html.desktop` is absent, preserving existing layout.
- `tauri-plugin-window-state` persists window dimensions and position between launches and handles unplugged monitors gracefully.
- App icon: Rasterized from `docs-site/book/favicon-de23e50b.svg` (1024x1024 PNG) using `cargo tauri icon` into `zorp-desktop/icons/`.

### 5. Single Instance & Clean Lifecycle
- `tauri-plugin-single-instance`: A subsequent double-click focuses the existing window rather than starting a second server instance competing for the SQLite session database (`~/.local/share/zorp/conversations.db`).
- Window close quits the process, terminating the server thread and its tokio runtime.

---

## Detailed Components & Implementation Steps

### Phase 1: Library Refactoring (`zorp-web`)
1. In `zorp-web/src/lib.rs`, define:
   ```rust
   pub struct ServeOptions {
       pub bind: String,
       pub port: u16,
       pub token: Option<String>,
       pub ui_dir: Option<PathBuf>,
       pub workspace: Option<PathBuf>,
       pub allow_origin: Vec<String>,
   }

   pub struct Running {
       pub addr: SocketAddr,
       pub future: Pin<Box<dyn Future<Output = Result<(), std::io::Error>> + Send>>,
   }

   pub enum ServeError {
       Security(String),
       Bind(std::io::Error),
   }

   pub fn serve(options: ServeOptions) -> Result<Running, ServeError>;
   ```
2. Parameterize `find_ui(explicit: Option<PathBuf>, additional_candidates: &[PathBuf]) -> Option<PathBuf>` in `zorp-web/src/lib.rs` so `zorp-desktop` can pass its macOS bundle resource directory (`Contents/Resources/web/`).
3. Refactor `zorp-web/src/main.rs` to parse CLI arguments into `ServeOptions`, call `zorp_web::serve(options)`, print startup messages, and await the running server future.
4. Verify `cargo test -p zorp-web` passes without regressions.

### Phase 2: Scaffold `zorp-desktop/`
1. Create `zorp-desktop/Cargo.toml` with:
   - Package version `0.4.1` (synchronized with workspace version).
   - Dependencies: `tauri` (v2), `tauri-plugin-single-instance`, `tauri-plugin-window-state`, `zorp-web` (path `../zorp-web`), `zorp-agent` (path `../zorp-agent`), `tokio`.
2. Add `build.rs` calling `tauri_build::build()`.
3. Create `zorp-desktop/tauri.conf.json`:
   - `productName`: `"Zorp"`
   - `identifier`: `"dev.zorp.app"`
   - `version`: `"0.4.1"`
   - Window: Overlay title bar, hidden title, inset traffic lights, size 1200x800.
   - Bundle resources: `["../web/index.html", "../web/styles.css", "../web/dist/**", "../web/fonts/**"]`.
4. Add `exclude = ["zorp-desktop"]` to the root `Cargo.toml`.
5. Add `zorp-desktop/target` to `.gitignore`.

### Phase 3: Server Boot & PATH Repair in Desktop
1. Implement `zorp-desktop/src/env.rs`: `login_shell_path()` and `repair_path()`, with unit test mocking shell output.
2. Implement `zorp-desktop/src/main.rs`:
   - Invoke `env::repair_path()`.
   - Start background thread with Tokio multi-thread runtime.
   - Try binding port `7777`, fallback to `0`.
   - Call `zorp_web::serve(options)` and transmit bound address over channel.
   - In Tauri `setup` hook, navigate webview to `http://127.0.0.1:<port>/`.
   - Wire `tauri-plugin-single-instance` to focus existing window.
   - Wire `tauri-plugin-window-state`.

### Phase 4: Styling & Native Chrome
1. In `tauri.conf.json`, configure webview initialization script:
   ```javascript
   document.documentElement.classList.add('desktop');
   ```
2. In `web/styles.css`, add rules under `html.desktop` for sidebar and top bar padding to account for inset window traffic lights.
3. Test locally with `cargo tauri dev`.

### Phase 5: Release Workflow, Version Checking & Packaging
1. Update `scripts/check-release-version.sh` to verify `zorp-desktop/tauri.conf.json` and `zorp-desktop/Cargo.toml` match the target release tag. Add integration test in `tests/release_version_check.rs`.
2. Update `.github/workflows/ci.yml`:
   - Add `desktop` job running on `macos-latest` when PRs touch `zorp-desktop/**` or `zorp-web/**`. Runs `npm run build` in `web/`, then `cargo check` and `cargo test` in `zorp-desktop/`.
3. Create `.github/workflows/desktop.yml`:
   - `workflow_dispatch` trigger with `tag` and `publish: false/true`.
   - Runs on `macos-latest`.
   - Builds universal binary `.dmg` (`cargo tauri build --target universal-apple-darwin`).
   - Generates SHA-256 checksum.
   - Ad-hoc signed by default; attaches to GitHub release via `gh release upload --clobber` when `publish: true`.
4. Update `README.md`, `CLAUDE.md`, and `AGENTS.md` with desktop instructions, Gatekeeper bypass (`xattr -d com.apple.quarantine`), and shared storage locations (`~/.config/zorp`). Add decisions entry to `docs/DECISIONS.md`.

---

## Non-Goals
- In-app auto-updater (GitHub releases serve as update channel).
- Homebrew Cask (deferred to post-v0.5.0 release).
- Keychain storage for API keys (persisted in-process or via env; local models like Ollama and oMLX require no keys).
- Windows/Linux desktop packaging (deferred to future issues; architecture does not preclude them).
