# Native macOS Swift Application for Zorp (Zorp.app)

Date: 2026-09-14. Status: validated design.

## Overview

A 100% native macOS desktop application for Zorp (`Zorp.app`), written in **Swift** and **SwiftUI**, targeting **macOS 14.0+ (Sonoma)** and **macOS 15.0+ (Sequoia)** on both Apple Silicon (`arm64`) and Intel (`x86_64`).

This replaces the initial Tauri v2 webview wrapper with a pure native macOS interface adhering strictly to Apple's Human Interface Guidelines (HIG). The application runs the existing Rust engine (`zorp-web`, `zorp-agent`, `zorp-track`, `zorp-voice`, `zorp-recall`) in-process on a dedicated Tokio background runtime via a minimal C-ABI static library bridge (`zorp-desktop-bridge`). Swift communicates with the embedded server over loopback `127.0.0.1:<port>` via `URLSession` and an asynchronous Server-Sent Events (`AsyncSequence`) stream with automatic `Last-Event-ID` reconnection.

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                                  Zorp.app                                   │
│                                                                             │
│  ┌───────────────────────────────────────────────────────────────────────┐  │
│  │ SwiftUI Native Presentation Layer                                     │  │
│  │ - NavigationSplitView (Sidebar, Conversation Detail, Inspector)       │  │
│  │ - Swift Observation framework (@Observable)                           │  │
│  │ - Native Markdown & Tool Activity Views                               │  │
│  │ - Sandboxed WKWebView (HTML/SVG Artifacts with strict CSP)            │  │
│  │ - AVAudioEngine Live Waveform Recording                               │  │
│  └───────────────────────────────────┬───────────────────────────────────┘  │
│                                      │                                      │
│                                      ▼                                      │
│  ┌───────────────────────────────────────────────────────────────────────┐  │
│  │ ZorpClient (Swift Concurrency / Actor)                                │  │
│  │ - REST API: async/await via URLSession                                │  │
│  │ - Streaming: AsyncThrowingStream<ServerEvent, Error> with Last-Event-ID │  │
│  └───────────────────────────────────┬───────────────────────────────────┘  │
│                                      │                                      │
│                                      ▼ HTTP / SSE                           │
│  ┌───────────────────────────────────────────────────────────────────────┐  │
│  │ Loopback Network Interface (127.0.0.1:7777 -> fallback ephemeral 0)   │  │
│  └───────────────────────────────────▲───────────────────────────────────┘  │
│                                      │ in-process                           │
│  ┌───────────────────────────────────┴───────────────────────────────────┐  │
│  │ In-Process Tokio Runtime (Rust Engine)                                │  │
│  │ - zorp-web::serve (axum router, session store, DuckDB, LanceDB)       │  │
│  │ - zorp-agent, zorp-track, zorp-voice, zorp-recall                     │  │
│  └───────────────────────────────────▲───────────────────────────────────┘  │
│                                      │ C-ABI                                │
│  ┌───────────────────────────────────┴───────────────────────────────────┐  │
│  │ zorp-desktop-bridge (Rust staticlib crate)                            │  │
│  │ - zorp_bridge_repair_path()                                           │  │
│  │ - zorp_bridge_start_server(requested_port, resource_dir, &out_port)   │  │
│  │ - zorp_bridge_stop_server()                                           │  │
│  └───────────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Key Design Decisions & Invariants

### 1. In-Process C-ABI Bridge (`zorp-desktop-bridge`)
* **Single Process, Single Binary**: The Rust engine compiles into a static library (`libzorp_desktop_bridge.a`). The Swift executable links this library directly. On launch, a single background OS thread initializes a multi-threaded Tokio runtime running `zorp_web::serve`.
* **Zero Child Process Overhead**: Eliminates external helper process supervision, IPC signaling issues, and orphaned daemon risks on unexpected quit.
* **Minimal C-ABI Surface**:
  * `int32_t zorp_bridge_repair_path(void)`: Invokes the user's login shell once (`$SHELL -lc 'printf %s "$PATH"'`) to populate Homebrew, pyenv, nvm, and cargo binary paths into the process environment before tools run.
  * `int32_t zorp_bridge_start_server(uint16_t requested_port, const char *resource_dir, uint16_t *out_port)`: Attempts binding to loopback port `7777`; falls back to ephemeral port `0` if occupied. Delivers the bound port synchronously back to Swift.
  * `void zorp_bridge_stop_server(void)`: Signals Tokio runtime cancellation and waits for DuckDB file locks and active session records to flush before process termination.
* **Workspace Isolation**: `zorp-desktop/` remains excluded from the root Cargo workspace (`exclude = ["zorp-desktop"]` in root `Cargo.toml`). Linux CI and `cargo test --workspace` remain fast and hermetic without requiring macOS SDKs or Xcode.

### 2. Networking & Event Protocol (`ZorpClient` & `SSEStream`)
* **`ZorpClient` Actor**: Swift actor managing all REST endpoints (`/api/sessions`, `/api/workspace`, `/api/artifacts`, `/api/voice`, `/api/investigate`).
* **Resilient Event Stream (`SSEStream`)**:
  * Connects to `GET /api/sessions/:id/events` using `URLSession.bytes(for:)`.
  * Tracks monotonic event sequence numbers (`seq: UInt64`).
  * On network drop or resume from sleep, reconnects using standard `Last-Event-ID: <seq>`. The Rust server replays from its append-only backlog without desync or missing frames.
* **Strongly-Typed Domain Events (`ServerEvent`)**:
  * Maps 1:1 to `zorp-web`'s `EventKind` enum:
    * `assistantDelta(text: String)`: Live token streaming.
    * `assistantWithdrawn(events: Int, reask: Int, bound: Int)`: Model response rolled back by harness; Swift UI atomically removes pending tokens without UI flashing.
    * `assistant(text: String)`: Final authoritative response text; triggers final markdown AST generation.
    * `toolStarted(name: String, phrase: String?)` & `tool(name: String, summary: String, phrase: String?)`: Tool execution state transitions.
    * `approvalRequest(id: String, tool: String, arguments: String)`: Tool-level permission gate.
    * `checkpointRequest(id: String, kind: String, prompt: String)`: Research-level track survival gate.
    * `investigateProgress(phase: String, attempt: Int?, of: Int?, ledger: LedgerFrame?)` & `investigateDone`: Multi-turn Aryabhatta progress and ledger telemetry.
    * `reviewerStarted`, `reviewerFinished`, `panelDone`: Adversarial panel review stream.
    * `context(usedTokens: UInt64, limitTokens: UInt64?, source: String)`: Context token usage.

### 3. UI/UX Architecture (`NavigationSplitView`)
* **Sidebar (Leading Pane)**:
  * Projects tree with disclosure chevrons.
  * Session rows showing title, relative timestamp, and turn status indicators.
  * Search field supporting local vector search via `/api/recall/search` and session filtering.
  * Footer displaying current workspace path and Claude-compatible skill count.
* **Conversation Detail (Center Pane)**:
  * **Header**: Session title, branch badge, token context meter ring (differentiating `reported` vs `estimated`), and action buttons for Investigation Bolt and Review Panel.
  * **Message Stream**:
    * Rendered markdown with native typography (SF Pro), inline code spans, syntax-highlighted code blocks with copy buttons.
    * Tool execution disclosure cards showing model-authored phrase, input arguments, and execution output.
    * `ApprovalCard`: Interactive tool permission card with keyboard accelerators (`⌘Y` approve, `⌘D` deny).
    * `CheckpointCard`: High-visibility research survival card (`Continue Track` vs `Kill Track`).
  * **Composer**:
    * Auto-expanding multi-line editor (`Return` to send, `Shift+Return` for newline).
    * Live voice input button with zero-latency audio waveform visualizer powered by `AVAudioEngine`.
    * Turn controls: `Stop` (`Escape`) when turn or investigation is running; "Finish Attempt & Stop" (`stop-after`) during investigations.
* **Inspector (Trailing Pane, `⌘\`)**:
  * Collapsible inspector pane displaying session artifacts (`draft.md`, diagrams, reports).
  * Sandboxed `WKWebView` dedicated to HTML/SVG artifact preview, enforced with `Content-Security-Policy: sandbox` (no script execution, isolated origin).
  * Native Aryabhatta DuckDB ledger table viewer showing conditions, expectations, intervals, and metrics.

### 4. Human-in-the-Loop & System Integration
* **Integrity Invariant for Checkpoints**: Rejection of a research checkpoint kills the track and records it to DuckDB. Answering is defended in code: if the app is closed or window dismissed, it does not record a rejection (`Decider::answered` invariant).
* **Native Notifications (`UserNotifications.framework`)**: Alerts user when long-running investigation runs finish or when an approval/checkpoint requires attention while the app is unfocused.
* **Window State Restoration**: Uses macOS native window state restoration to persist window frame dimensions, divider positions, and active session selection across app launches.
* **Native Keyboard Shortcuts**:
  * `⌘N`: New session
  * `⌘K`: Quick Switcher / Recall semantic search
  * `⌘\`: Toggle Artifact Inspector
  * `⌘[` / `⌘]`: Navigate previous/next session
  * `⌘.` or `Escape`: Stop active turn
  * `⌘,`: Preferences / Settings (model selection, workspace directory, API keys)

---

## Detailed Directory & Code Structure

```
zorp-desktop/
├── Cargo.toml                       # Excluded from root Cargo.toml workspace
├── bridge/                          # Rust static library crate
│   ├── Cargo.toml
│   ├── include/
│   │   └── zorp_bridge.h            # C header exposed to Swift
│   └── src/
│       ├── env.rs                   # Login shell PATH repair & unit tests
│       ├── lib.rs                   # extern "C" FFI export functions
│       └── server.rs                # In-process Tokio server runtime & port fallback
├── Zorp/                            # Native Swift / SwiftUI macOS application
│   ├── Zorp.xcodeproj               # Xcode project (macOS 14.0+ deployment target)
│   ├── Info.plist                   # App metadata, microphone usage description
│   ├── App/
│   │   ├── ZorpApp.swift            # @main entry point & NSApplicationDelegate
│   │   ├── AppCommands.swift        # Menu bar commands & keyboard shortcuts
│   │   └── BridgeService.swift      # Swift wrapper around zorp_bridge.h C functions
│   ├── Network/
│   │   ├── ZorpClient.swift         # Actor handling REST API operations
│   │   └── SSEStream.swift          # AsyncSequence parsing SSE frames with Last-Event-ID
│   ├── Models/
│   │   ├── ServerEvent.swift        # Strongly-typed representations of EventKind
│   │   ├── Session.swift            # Session metadata and summary
│   │   ├── Artifact.swift           # Artifact reference and file listing
│   │   └── Ledger.swift             # Aryabhatta DuckDB experiment frames
│   ├── ViewModels/
│   │   ├── AppState.swift           # Root @Observable: sessions, active session, settings
│   │   ├── SessionViewModel.swift   # Per-session @Observable: messages, stream buffering, gates
│   │   └── VoiceViewModel.swift     # AVAudioEngine audio capture & live level metering
│   └── Views/
│       ├── MainWindowView.swift     # NavigationSplitView 3-pane layout
│       ├── Sidebar/
│       │   ├── SidebarView.swift
│       │   ├── SessionRowView.swift
│       │   └── QuickSwitcherSheet.swift
│       ├── Conversation/
│       │   ├── ConversationView.swift
│       │   ├── MessageBubbleView.swift
│       │   ├── MarkdownView.swift
│       │   └── ToolActivityView.swift
│       ├── Cards/
│       │   ├── ApprovalCardView.swift
│       │   └── CheckpointCardView.swift
│       ├── Composer/
│       │   ├── ComposerView.swift
│       │   └── VoiceMeterView.swift
│       ├── Inspector/
│       │   ├── InspectorView.swift
│       │   ├── SandboxedWebView.swift
│       │   └── LedgerTableView.swift
│       └── Settings/
│           └── SettingsView.swift
└── scripts/
    ├── build-app.sh                 # Compiles universal bridge staticlib & builds Zorp.app
    └── package-dmg.sh               # Codesigns and packages universal Zorp.dmg
```

---

## Build Pipeline & Automation

### 1. Multi-Architecture Universal Binary Compilation (`scripts/build-app.sh`)
1. Compiles `zorp-desktop/bridge` for both Apple Silicon and Intel:
   ```bash
   cargo build --manifest-path zorp-desktop/bridge/Cargo.toml --release --target aarch64-apple-darwin
   cargo build --manifest-path zorp-desktop/bridge/Cargo.toml --release --target x86_64-apple-darwin
   ```
2. Combines them into a universal static library using `lipo`:
   ```bash
   lipo -create \
     zorp-desktop/bridge/target/aarch64-apple-darwin/release/libzorp_desktop_bridge.a \
     zorp-desktop/bridge/target/x86_64-apple-darwin/release/libzorp_desktop_bridge.a \
     -output zorp-desktop/Zorp/libzorp_desktop_bridge.a
   ```
3. Compiles the Xcode project via `xcodebuild`:
   ```bash
   xcodebuild -project zorp-desktop/Zorp/Zorp.xcodeproj \
              -scheme Zorp \
              -configuration Release \
              -archivePath zorp-desktop/build/Zorp.xcarchive archive
   xcodebuild -exportArchive \
              -archivePath zorp-desktop/build/Zorp.xcarchive \
              -exportPath zorp-desktop/build/export \
              -exportOptionsPlist zorp-desktop/Zorp/ExportOptions.plist
   ```

### 2. Packaging & Distribution (`scripts/package-dmg.sh`)
* Creates a universal `.dmg` with the Zorp icon and `/Applications` drag-to-install symlink using `hdiutil` or `create-dmg`.
* Embedded `web/` assets are bundled into `Zorp.app/Contents/Resources/web/`.

### 3. Continuous Integration (`.github/workflows/desktop.yml`)
* Runs on `macos-latest`.
* Triggers when changes occur in `zorp-desktop/**`, `zorp-web/**`, or on release version tags.
* Verifies:
  1. Bridge unit tests (`cargo test --manifest-path zorp-desktop/bridge/Cargo.toml`).
  2. Swift tests (`xcodebuild test -project zorp-desktop/Zorp/Zorp.xcodeproj -scheme ZorpTests`).
  3. Release tag version check matching root `Cargo.toml`.

---

## Verification & Testing Strategy

1. **Bridge Unit Tests (Rust)**:
   * Test PATH repair parsing: tests empty PATH, longer PATH adoption, and rejection of degraded paths.
   * Test port contention: binds loopback port 7777 and asserts `zorp_bridge_start_server` falls back to ephemeral port 0 without hanging.
2. **Swift Concurrency & Streaming Unit Tests**:
   * `SSEStreamTests`: Feeds mock SSE chunk fixtures into `SSEStream` and asserts strongly-typed decoding of `assistantDelta`, `tool`, `approvalRequest`, and rollback on `assistantWithdrawn`.
   * `SessionViewModelTests`: Verifies that message deltas buffer smoothly and settle into an immutable markdown message when `assistant` arrives.
3. **Research Safety & Gate Tests**:
   * Verifies that dismissing or closing an approval card does not emit a false checkpoint rejection.
   * Asserts `POST /api/sessions/:id/investigate/stop-after` winds down an active run without invalidating attempt counters.
4. **Security Isolation**:
   * Asserts `SandboxedWebView` renders with `Content-Security-Policy: sandbox`, verifying that embedded scripts cannot access local files or execute arbitrary JavaScript.
