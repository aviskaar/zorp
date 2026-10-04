---
status: accepted
date: 2026-09-12
---

# native Mac app (`zorp-desktop`) runs `zorp-web` in-process via Tauri v2

**Decision:** A native macOS desktop application (`Zorp.app`) shipped as a double-clickable universal `.dmg` on GitHub releases. It wraps Apple's system `WKWebView` using Tauri v2 and runs `zorp-web::serve` in-process on a background Tokio runtime, binding loopback port 7777 (falling back to ephemeral port 0 if occupied). `zorp-desktop/` is its own Cargo crate, excluded from the workspace (`exclude = ["zorp-desktop"]` in root `Cargo.toml`) with its own `Cargo.lock`. See `docs/superpowers/specs/2026-09-12-native-mac-app-design.md`.

**Why in-process Tauri and not Electron or a sidecar binary.** Tauri links `zorp-web` and `zorp-agent` as libraries into a single unified binary, keeping bundle size under ~15 MB and avoiding child process supervision, zombie servers on unexpected quits, and health polling delays. Electron would add a 150 MB Chromium runtime and a duplicate Node toolchain for a UI that is already plain TypeScript. A sidecar binary requires discovering the executable path inside the `.app` bundle, process spawning, port contention management, and clean SIGTERM handlers on quit. In-process, the app knows the bound port before opening the window, and the server terminates with the process.

**Why excluded from the workspace.** Tauri pulls in `wry`, `tao`, `objc2`, and desktop system bindings. Putting `zorp-desktop` into the root Cargo workspace would slow down every CI job and break `cargo test --workspace` on Linux runners that lack GTK/WebKit2GTK dev headers. Keeping it excluded preserves fast, hermetic Linux builds and tests while letting macOS CI compile `zorp-desktop` independently.

**What it rules out:** Sidecar binary architectures, Electron runtimes, custom protocol schemes (`tauri://`) that require CORS allowances, and running multiple conflicting servers on the same SQLite session database (prevented via `tauri-plugin-single-instance`).
