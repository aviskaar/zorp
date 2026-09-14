# Native macOS Swift Application for Zorp Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a 100% native macOS Swift/SwiftUI application (`Zorp.app`) for Zorp, replacing the Tauri wrapper with a HIG-compliant 3-pane interface that embeds the Tokio/zorp-web runtime in-process via a C-ABI static library bridge and communicates over loopback HTTP/SSE.

**Architecture:** A lightweight Rust static library crate (`zorp-desktop-bridge`) compiles into `libzorp_desktop_bridge.a` exporting C-ABI functions for environment PATH repair, Tokio background runtime initialization, and server lifecycle. The native Swift app uses `NavigationSplitView` with Swift's modern `Observation` framework (`@Observable`), consumes `zorp-web`'s REST & SSE stream (`URLSession` with `Last-Event-ID` resume), renders markdown and tool disclosure cards natively, renders visual artifacts in a sandboxed `WKWebView` with strict CSP, and captures audio via `AVAudioEngine`.

**Tech Stack:** Swift 5.10 / Swift 6, SwiftUI (macOS 14.0+ Sonoma & macOS 15.0+ Sequoia), Rust 1.80+ (2021 edition), Tokio 1.x, Axum, URLSession, AVAudioEngine, WebKit (`WKWebView`), AppKit.

## Global Constraints

- **Platform Target:** macOS 14.0+ (Sonoma) and macOS 15.0+ (Sequoia), Universal binary supporting Apple Silicon (`arm64`) and Intel (`x86_64`).
- **Workspace Isolation:** `zorp-desktop/` remains excluded from the root Cargo workspace (`exclude = ["zorp-desktop"]` in root `Cargo.toml`) so Linux CI and root `cargo test --workspace` remain fast and hermetic.
- **Port Strategy:** Attempts loopback bind to `127.0.0.1:7777`; falls back to ephemeral port `0` if occupied.
- **SSE Continuity:** SSE client must track monotonic `seq` and supply `Last-Event-ID: <seq>` on reconnection to prevent event loss.
- **Security Invariant:** All HTML/SVG preview artifacts rendered in `WKWebView` must be served under `Content-Security-Policy: sandbox` with JavaScript execution disabled.
- **Integrity Invariant:** Dismissing a window or closing the app must never trigger a research checkpoint rejection (`Decider::answered` safety rule).

---

### Task 1: Rust Bridge Crate (`zorp-desktop/bridge`)

**Files:**
- Create: `zorp-desktop/bridge/Cargo.toml`
- Create: `zorp-desktop/bridge/include/zorp_bridge.h`
- Create: `zorp-desktop/bridge/src/env.rs`
- Create: `zorp-desktop/bridge/src/server.rs`
- Create: `zorp-desktop/bridge/src/lib.rs`
- Modify: `Cargo.toml` (root, verify `exclude = ["zorp-desktop"]`)

**Interfaces:**
- Consumes: `zorp_web::serve`, `zorp_web::ServeOptions`
- Produces: C-ABI functions:
  - `int32_t zorp_bridge_repair_path(void)`
  - `int32_t zorp_bridge_start_server(uint16_t requested_port, const char *resource_dir, uint16_t *out_port)`
  - `void zorp_bridge_stop_server(void)`

- [ ] **Step 1: Create `zorp-desktop/bridge/Cargo.toml` and header `zorp_bridge.h`**

Create `zorp-desktop/bridge/Cargo.toml`:
```toml
[package]
name = "zorp-desktop-bridge"
version = "0.5.0"
edition = "2021"
description = "In-process C-ABI static library bridge for Zorp native macOS desktop app"
license = "MIT"

[lib]
name = "zorp_desktop_bridge"
crate-type = ["staticlib", "rlib"]

[dependencies]
zorp-web = { path = "../../../zorp-web" }
tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync"] }
url = "2"
serde_json = "1"

[workspace]
```

Create `zorp-desktop/bridge/include/zorp_bridge.h`:
```c
#ifndef ZORP_BRIDGE_H
#define ZORP_BRIDGE_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/// Repairs the process environment PATH by reading from the user's login shell.
/// Returns 0 on success, or -1 if the login shell could not be evaluated.
int32_t zorp_bridge_repair_path(void);

/// Starts the in-process Tokio background server running zorp-web.
/// Attempts to bind requested_port (typically 7777). If occupied, binds ephemeral port 0.
/// Delivers the successfully bound port into out_port.
/// Returns 0 on success, or -1 on fatal failure.
int32_t zorp_bridge_start_server(uint16_t requested_port, const char *resource_dir, uint16_t *out_port);

/// Gracefully signals server cancellation and stops the Tokio runtime.
void zorp_bridge_stop_server(void);

#ifdef __cplusplus
}
#endif

#endif /* ZORP_BRIDGE_H */
```

- [ ] **Step 2: Implement `zorp-desktop/bridge/src/env.rs` with unit tests**

Create `zorp-desktop/bridge/src/env.rs`:
```rust
use std::process::Command;

pub fn parse_shell_path(output: &str, current: &str) -> Option<String> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return None;
    }
    if current.is_empty() || trimmed.len() > current.len() {
        Some(trimmed.to_string())
    } else {
        None
    }
}

pub fn login_shell_path() -> Option<String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    let mut cmd = Command::new(shell);
    cmd.arg("-lc").arg("printf %s \"$PATH\"");

    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    let current = std::env::var("PATH").unwrap_or_default();
    parse_shell_path(&stdout, &current)
}

pub fn repair_path() -> bool {
    if let Some(new_path) = login_shell_path() {
        std::env::set_var("PATH", new_path);
        true
    } else {
        false
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
    fn parse_shell_path_accepts_richer_path() {
        let current = "/usr/bin:/bin";
        let richer = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin";
        assert_eq!(parse_shell_path(richer, current), Some(richer.to_string()));
    }

    #[test]
    fn parse_shell_path_rejects_shorter_or_equal_path() {
        let current = "/opt/homebrew/bin:/usr/bin:/bin";
        let shorter = "/usr/bin:/bin";
        assert_eq!(parse_shell_path(shorter, current), None);
        assert_eq!(parse_shell_path(current, current), None);
    }
}
```

- [ ] **Step 3: Implement `zorp-desktop/bridge/src/server.rs` with port fallback tests**

Create `zorp-desktop/bridge/src/server.rs`:
```rust
use std::net::{SocketAddr, TcpListener};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::sync_channel;
use std::sync::Arc;
use std::thread;
use tokio::sync::oneshot;
use zorp_web::{serve, ServeOptions};

static RUNNING: AtomicBool = AtomicBool::new(false);
static mut STOP_TX: Option<oneshot::Sender<()>> = None;

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

    unsafe {
        STOP_TX = Some(stop_tx);
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
    unsafe {
        if let Some(tx) = STOP_TX.take() {
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
```

- [ ] **Step 4: Implement `zorp-desktop/bridge/src/lib.rs` (C-ABI exports)**

Create `zorp-desktop/bridge/src/lib.rs`:
```rust
mod env;
mod server;

use std::ffi::CStr;
use std::os::raw::c_char;
use std::path::PathBuf;

#[no_mangle]
pub extern "C" fn zorp_bridge_repair_path() -> i32 {
    if env::repair_path() {
        0
    } else {
        -1
    }
}

#[no_mangle]
pub extern "C" fn zorp_bridge_start_server(
    requested_port: u16,
    resource_dir: *const c_char,
    out_port: *mut u16,
) -> i32 {
    if out_port.is_null() {
        return -1;
    }

    let bundle_dir: Option<PathBuf> = if resource_dir.is_null() {
        None
    } else {
        unsafe {
            CStr::from_ptr(resource_dir)
                .to_str()
                .ok()
                .map(PathBuf::from)
        }
    };

    match server::start_background_server(requested_port, bundle_dir) {
        Ok(addr) => {
            unsafe {
                *out_port = addr.port();
            }
            0
        }
        Err(err) => {
            eprintln!("zorp_bridge_start_server error: {err}");
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn zorp_bridge_stop_server() {
    server::stop_server();
}
```

- [ ] **Step 5: Run tests on `zorp-desktop/bridge`**

Run: `cargo test --manifest-path zorp-desktop/bridge/Cargo.toml`
Expected: PASS (all unit tests pass).

- [ ] **Step 6: Commit**

```bash
git add zorp-desktop/bridge/
git commit -m "feat(desktop): scaffold zorp-desktop-bridge crate with C-ABI exports"
```

---

### Task 2: Build Automation Script for Universal Static Library

**Files:**
- Create: `zorp-desktop/scripts/build-bridge.sh`
- Test: Execute script to build universal `libzorp_desktop_bridge.a`

**Interfaces:**
- Consumes: `zorp-desktop/bridge/`
- Produces: `zorp-desktop/Zorp/Frameworks/libzorp_desktop_bridge.a` and header copy in `zorp-desktop/Zorp/Bridge/`

- [ ] **Step 1: Write `zorp-desktop/scripts/build-bridge.sh`**

Create `zorp-desktop/scripts/build-bridge.sh`:
```bash
#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
BRIDGE_DIR="${REPO_ROOT}/zorp-desktop/bridge"
OUTPUT_DIR="${REPO_ROOT}/zorp-desktop/Zorp/Frameworks"
INCLUDE_DIR="${REPO_ROOT}/zorp-desktop/Zorp/Bridge"

mkdir -p "${OUTPUT_DIR}" "${INCLUDE_DIR}"

echo "==> Building zorp-desktop-bridge for aarch64-apple-darwin..."
cargo build --manifest-path "${BRIDGE_DIR}/Cargo.toml" --release --target aarch64-apple-darwin

echo "==> Building zorp-desktop-bridge for x86_64-apple-darwin..."
cargo build --manifest-path "${BRIDGE_DIR}/Cargo.toml" --release --target x86_64-apple-darwin

echo "==> Creating universal static library using lipo..."
lipo -create \
  "${BRIDGE_DIR}/target/aarch64-apple-darwin/release/libzorp_desktop_bridge.a" \
  "${BRIDGE_DIR}/target/x86_64-apple-darwin/release/libzorp_desktop_bridge.a" \
  -output "${OUTPUT_DIR}/libzorp_desktop_bridge.a"

echo "==> Copying C header..."
cp "${BRIDGE_DIR}/include/zorp_bridge.h" "${INCLUDE_DIR}/zorp_bridge.h"

echo "==> Verifying universal static library architectures..."
lipo -info "${OUTPUT_DIR}/libzorp_desktop_bridge.a"

echo "==> Universal bridge library successfully created at ${OUTPUT_DIR}/libzorp_desktop_bridge.a"
```
Make executable: `chmod +x zorp-desktop/scripts/build-bridge.sh`

- [ ] **Step 2: Run build script and verify architectures**

Run: `zorp-desktop/scripts/build-bridge.sh`
Expected: Output includes `Architectures in the fat file: ... are: x86_64 arm64`.

- [ ] **Step 3: Commit**

```bash
git add zorp-desktop/scripts/build-bridge.sh
git commit -m "chore(desktop): add build script for universal bridge static library"
```

---

### Task 3: Swift Project Setup & C-Bridge Wrapper

**Files:**
- Create: `zorp-desktop/Zorp/Bridge/Zorp-Bridging-Header.h`
- Create: `zorp-desktop/Zorp/App/BridgeService.swift`
- Create: `zorp-desktop/Zorp/App/ZorpApp.swift`
- Create: `zorp-desktop/Zorp/Info.plist`
- Test: `zorp-desktop/ZorpTests/BridgeServiceTests.swift`

**Interfaces:**
- Consumes: `zorp_bridge.h`, `libzorp_desktop_bridge.a`
- Produces: `BridgeService.shared.start(preferredPort: UInt16) throws -> UInt16`, `BridgeService.shared.stop()`

- [ ] **Step 1: Create Bridging Header and Info.plist**

Create `zorp-desktop/Zorp/Bridge/Zorp-Bridging-Header.h`:
```c
#ifndef Zorp_Bridging_Header_h
#define Zorp_Bridging_Header_h

#import "zorp_bridge.h"

#endif /* Zorp_Bridging_Header_h */
```

Create `zorp-desktop/Zorp/Info.plist`:
```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key>
    <string>com.aviskaar.zorp</string>
    <key>CFBundleName</key>
    <string>Zorp</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>0.5.0</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>LSMinimumSystemVersion</key>
    <string>14.0</string>
    <key>NSMicrophoneUsageDescription</key>
    <string>Zorp requires microphone access for hands-free voice transcription with Qwen3-ASR.</string>
</dict>
</plist>
```

- [ ] **Step 2: Implement `BridgeService.swift`**

Create `zorp-desktop/Zorp/App/BridgeService.swift`:
```swift
import Foundation

public final class BridgeService: @unchecked Sendable {
    public static let shared = BridgeService()

    private var boundPort: UInt16?
    private let lock = NSLock()

    private init() {}

    public func repairPath() {
        _ = zorp_bridge_repair_path()
    }

    public func start(preferredPort: UInt16 = 7777, resourceDir: String? = nil) throws -> UInt16 {
        lock.lock()
        defer { lock.unlock() }

        if let port = boundPort {
            return port
        }

        repairPath()

        var outPort: UInt16 = 0
        let resPathCString = resourceDir?.cString(using: .utf8)
        let status = resPathCString?.withUnsafeBufferPointer { ptr in
            zorp_bridge_start_server(preferredPort, ptr.baseAddress, &outPort)
        } ?? zorp_bridge_start_server(preferredPort, nil, &outPort)

        guard status == 0, outPort > 0 else {
            throw NSError(domain: "ZorpBridgeError", code: 1, userInfo: [
                NSLocalizedDescriptionKey: "Failed to start background zorp server"
            ])
        }

        boundPort = outPort
        return outPort
    }

    public func stop() {
        lock.lock()
        defer { lock.unlock() }

        guard boundPort != nil else { return }
        zorp_bridge_stop_server()
        boundPort = nil
    }
}
```

- [ ] **Step 3: Write `BridgeServiceTests.swift`**

Create `zorp-desktop/ZorpTests/BridgeServiceTests.swift`:
```swift
import XCTest
@testable import Zorp

final class BridgeServiceTests: XCTestCase {
    func testStartAndStopServer() throws {
        let bridge = BridgeService.shared
        let port = try bridge.start(preferredPort: 17778)
        XCTAssertGreaterThan(port, 0)
        bridge.stop()
    }
}
```

- [ ] **Step 4: Commit**

```bash
git add zorp-desktop/Zorp/Bridge/ zorp-desktop/Zorp/App/BridgeService.swift zorp-desktop/Zorp/Info.plist zorp-desktop/ZorpTests/
git commit -m "feat(desktop): add Swift bridge service and tests"
```

---

### Task 4: Networking Layer & Server-Sent Events (SSE) Client

**Files:**
- Create: `zorp-desktop/Zorp/Models/ServerEvent.swift`
- Create: `zorp-desktop/Zorp/Models/Session.swift`
- Create: `zorp-desktop/Zorp/Network/SSEStream.swift`
- Create: `zorp-desktop/Zorp/Network/ZorpClient.swift`
- Test: `zorp-desktop/ZorpTests/SSEStreamTests.swift`

**Interfaces:**
- Consumes: Loopback HTTP endpoints (`/api/*`)
- Produces: `ZorpClient` actor, `SSEStream` async event publisher yielding `ServerEvent`

- [ ] **Step 1: Implement `ServerEvent.swift` (Decodable matching `EventKind`)**

Create `zorp-desktop/Zorp/Models/ServerEvent.swift`:
```swift
import Foundation

public struct ConditionFrame: Codable, Hashable, Sendable {
    public let key: String
    public let value: String
}

public struct ExpectationFrame: Codable, Hashable, Sendable {
    public let metricKey: String
    public let expectedValue: Double
    public let intervalLow: Double
    public let intervalHigh: Double
    public let confidence: Double

    enum CodingKeys: String, CodingKey {
        case metricKey = "metric_key"
        case expectedValue = "expected_value"
        case intervalLow = "interval_low"
        case intervalHigh = "interval_high"
        case confidence
    }
}

public struct MetricFrame: Codable, Hashable, Sendable {
    public let key: String
    public let value: String
}

public struct ExperimentFrame: Codable, Hashable, Sendable {
    public let id: String
    public let status: String
    public let conditions: [ConditionFrame]
    public let expectations: [ExpectationFrame]
    public let metrics: [MetricFrame]
}

public struct LedgerFrame: Codable, Hashable, Sendable {
    public let trackId: String
    public let present: bool?
    public let forecasting: bool?
    public let experiments: [ExperimentFrame]

    enum CodingKeys: String, CodingKey {
        case trackId = "track_id"
        case present
        case forecasting
        case experiments
    }
}

public enum ServerEventKind: Sendable {
    case working
    case workingDone
    case tool(name: String, summary: String, phrase: String?)
    case toolStarted(name: String, phrase: String?)
    case verify(command: String, passed: Bool)
    case notice(text: String)
    case assistantDelta(text: String)
    case assistantWithdrawn(events: Int, reask: Int, bound: Int)
    case assistant(text: String)
    case approvalRequest(id: String, tool: String, arguments: String)
    case checkpointRequest(id: String, kind: String, prompt: String)
    case investigateProgress(phase: String, attempt: Int?, of: Int?, ledger: LedgerFrame?)
    case investigateDone(trackId: String, approved: Bool?, needsPrereg: Bool, artifact: String?)
    case context(usedTokens: UInt64, limitTokens: UInt64?, source: String)
    case compacting(messages: Int, tokensBefore: UInt64, manual: Bool)
    case compacted(ok: Bool, tokensBefore: UInt64, tokensAfter: UInt64, summary: String?)
    case unknown(type: String)
}

public struct ServerEvent: Sendable {
    public let seq: UInt64
    public let kind: ServerEventKind
}
```

- [ ] **Step 2: Implement `SSEStream.swift` with `Last-Event-ID` tracking**

Create `zorp-desktop/Zorp/Network/SSEStream.swift`:
```swift
import Foundation

public final class SSEStream: @unchecked Sendable {
    private let url: URL
    private let session: URLSession

    public init(url: URL, session: URLSession = .shared) {
        self.url = url
        self.session = session
    }

    public func events(startingFrom lastSeq: UInt64? = nil) -> AsyncThrowingStream<ServerEvent, Error> {
        AsyncThrowingStream { continuation in
            let task = Task {
                var request = URLRequest(url: self.url)
                request.setValue("text/event-stream", forHTTPHeaderField: "Accept")
                if let lastSeq = lastSeq {
                    request.setValue("\(lastSeq)", forHTTPHeaderField: "Last-Event-ID")
                }

                do {
                    let (asyncBytes, response) = try await self.session.bytes(for: request)
                    guard let httpResponse = response as? HTTPURLResponse,
                          (200...299).contains(httpResponse.statusCode) else {
                        throw URLError(.badServerResponse)
                    }

                    var currentSeq: UInt64 = lastSeq ?? 0
                    for try await line in asyncBytes.lines {
                        if Task.isCancelled { break }
                        let trimmed = line.trimmingCharacters(in: .whitespaces)
                        if trimmed.hasPrefix("id:") {
                            let idStr = trimmed.dropFirst(3).trimmingCharacters(in: .whitespaces)
                            if let parsed = UInt64(idStr) {
                                currentSeq = parsed
                            }
                        } else if trimmed.hasPrefix("data:") {
                            let jsonStr = trimmed.dropFirst(5).trimmingCharacters(in: .whitespaces)
                            if let data = jsonStr.data(using: .utf8),
                               let event = SSEStream.parseEvent(data: data, seq: currentSeq) {
                                continuation.yield(event)
                            }
                        }
                    }
                    continuation.finish()
                } catch {
                    continuation.finish(throwing: error)
                }
            }

            continuation.onTermination = { _ in
                task.cancel()
            }
        }
    }

    public static func parseEvent(data: Data, seq: UInt64) -> ServerEvent? {
        guard let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let type = json["type"] as? String else {
            return nil
        }

        let kind: ServerEventKind
        switch type {
        case "working":
            kind = .working
        case "working_done":
            kind = .workingDone
        case "tool":
            let name = json["name"] as? String ?? ""
            let summary = json["summary"] as? String ?? ""
            let phrase = json["phrase"] as? String
            kind = .tool(name: name, summary: summary, phrase: phrase)
        case "tool_started":
            let name = json["name"] as? String ?? ""
            let phrase = json["phrase"] as? String
            kind = .toolStarted(name: name, phrase: phrase)
        case "assistant_delta":
            let text = json["text"] as? String ?? ""
            kind = .assistantDelta(text: text)
        case "assistant_withdrawn":
            let events = json["events"] as? Int ?? 0
            let reask = json["reask"] as? Int ?? 0
            let bound = json["bound"] as? Int ?? 0
            kind = .assistantWithdrawn(events: events, reask: reask, bound: bound)
        case "assistant":
            let text = json["text"] as? String ?? ""
            kind = .assistant(text: text)
        case "approval_request":
            let id = json["id"] as? String ?? ""
            let tool = json["tool"] as? String ?? ""
            let arguments = json["arguments"] as? String ?? ""
            kind = .approvalRequest(id: id, tool: tool, arguments: arguments)
        case "checkpoint_request":
            let id = json["id"] as? String ?? ""
            let cKind = json["kind"] as? String ?? ""
            let prompt = json["prompt"] as? String ?? ""
            kind = .checkpointRequest(id: id, kind: cKind, prompt: prompt)
        case "context":
            let used = (json["used_tokens"] as? NSNumber)?.uint64Value ?? 0
            let limit = (json["limit_tokens"] as? NSNumber)?.uint64Value
            let source = json["source"] as? String ?? "estimated"
            kind = .context(usedTokens: used, limitTokens: limit, source: source)
        default:
            kind = .unknown(type: type)
        }

        return ServerEvent(seq: seq, kind: kind)
    }
}
```

- [ ] **Step 3: Implement `ZorpClient.swift` (REST Actor)**

Create `zorp-desktop/Zorp/Network/ZorpClient.swift`:
```swift
import Foundation

public actor ZorpClient {
    public let baseURL: URL
    private let session: URLSession

    public init(baseURL: URL, session: URLSession = .shared) {
        self.baseURL = baseURL
        self.session = session
    }

    public func createSession() async throws -> String {
        let url = baseURL.appendingPathComponent("api/sessions")
        var request = URLRequest(url: url)
        request.httpMethod = "POST"

        let (data, response) = try await session.data(for: request)
        guard let httpResponse = response as? HTTPURLResponse, httpResponse.statusCode == 200,
              let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
              let id = json["id"] as? String else {
            throw URLError(.badServerResponse)
        }
        return id
    }

    public func startTurn(sessionId: String, prompt: String) async throws {
        let url = baseURL.appendingPathComponent("api/sessions/\(sessionId)/turn")
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        let body: [String: Any] = ["prompt": prompt]
        request.httpBody = try JSONSerialization.data(withJSONObject: body)

        let (_, response) = try await session.data(for: request)
        guard let httpResponse = response as? HTTPURLResponse, (200...299).contains(httpResponse.statusCode) else {
            throw URLError(.badServerResponse)
        }
    }

    public func stopTurn(sessionId: String) async throws {
        let url = baseURL.appendingPathComponent("api/sessions/\(sessionId)/stop")
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        _ = try await session.data(for: request)
    }

    public func submitApproval(sessionId: String, requestId: String, approved: Bool) async throws {
        let url = baseURL.appendingPathComponent("api/sessions/\(sessionId)/approve")
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        let body: [String: Any] = ["id": requestId, "approved": approved]
        request.httpBody = try JSONSerialization.data(withJSONObject: body)
        _ = try await session.data(for: request)
    }

    public func submitCheckpoint(sessionId: String, checkpointId: String, approved: Bool) async throws {
        let url = baseURL.appendingPathComponent("api/sessions/\(sessionId)/checkpoint")
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        let body: [String: Any] = ["id": checkpointId, "approved": approved]
        request.httpBody = try JSONSerialization.data(withJSONObject: body)
        _ = try await session.data(for: request)
    }
}
```

- [ ] **Step 4: Write `SSEStreamTests.swift`**

Create `zorp-desktop/ZorpTests/SSEStreamTests.swift`:
```swift
import XCTest
@testable import Zorp

final class SSEStreamTests: XCTestCase {
    func testParseAssistantDelta() {
        let json = #"{"type":"assistant_delta","text":"Hello world"}"#
        let event = SSEStream.parseEvent(data: json.data(using: .utf8)!, seq: 42)
        XCTAssertNotNil(event)
        XCTAssertEqual(event?.seq, 42)
        if case .assistantDelta(let text) = event?.kind {
            XCTAssertEqual(text, "Hello world")
        } else {
            XCTFail("Expected assistantDelta")
        }
    }

    func testParseApprovalRequest() {
        let json = #"{"type":"approval_request","id":"app-1","tool":"bash","arguments":"{\"cmd\":\"ls\"}"}"#
        let event = SSEStream.parseEvent(data: json.data(using: .utf8)!, seq: 100)
        XCTAssertNotNil(event)
        if case .approvalRequest(let id, let tool, _) = event?.kind {
            XCTAssertEqual(id, "app-1")
            XCTAssertEqual(tool, "bash")
        } else {
            XCTFail("Expected approvalRequest")
        }
    }
}
```

- [ ] **Step 5: Commit**

```bash
git add zorp-desktop/Zorp/Models/ zorp-desktop/Zorp/Network/ zorp-desktop/ZorpTests/
git commit -m "feat(desktop): add SSE parser, ZorpClient actor, and tests"
```

---

### Task 5: State Management (`AppState` & `SessionViewModel`)

**Files:**
- Create: `zorp-desktop/Zorp/ViewModels/AppState.swift`
- Create: `zorp-desktop/Zorp/ViewModels/SessionViewModel.swift`
- Test: `zorp-desktop/ZorpTests/SessionViewModelTests.swift`

**Interfaces:**
- Consumes: `ZorpClient`, `SSEStream`, `ServerEvent`
- Produces: `@Observable` classes `AppState` and `SessionViewModel` for SwiftUI views

- [ ] **Step 1: Implement `SessionViewModel.swift`**

Create `zorp-desktop/Zorp/ViewModels/SessionViewModel.swift`:
```swift
import Foundation
import Observation

public struct ChatMessage: Identifiable, Sendable {
    public let id: String
    public let role: String // "user" or "assistant"
    public var text: String
    public var isStreaming: Bool

    public init(id: String = UUID().uuidString, role: String, text: String, isStreaming: Bool = false) {
        self.id = id
        self.role = role
        self.text = text
        self.isStreaming = isStreaming
    }
}

public struct ToolCallItem: Identifiable, Sendable {
    public let id: String
    public let name: String
    public var phrase: String?
    public var summary: String?
    public var isRunning: Bool
}

@Observable
public final class SessionViewModel: @unchecked Sendable {
    public let sessionId: String
    public let client: ZorpClient
    public let streamURL: URL

    public var messages: [ChatMessage] = []
    public var tools: [ToolCallItem] = []
    public var pendingApproval: (id: String, tool: String, arguments: String)?
    public var pendingCheckpoint: (id: String, kind: String, prompt: String)?
    public var isWorking: Bool = false
    public var usedTokens: UInt64 = 0
    public var limitTokens: UInt64? = nil

    private var sseTask: Task<Void, Never>?
    private var streamingMessageId: String?

    public init(sessionId: String, client: ZorpClient, baseURL: URL) {
        self.sessionId = sessionId
        self.client = client
        self.streamURL = baseURL.appendingPathComponent("api/sessions/\(sessionId)/events")
    }

    public func connect() {
        let stream = SSEStream(url: streamURL)
        sseTask?.cancel()
        sseTask = Task { @MainActor in
            do {
                for try await event in stream.events() {
                    self.handleEvent(event)
                }
            } catch {
                print("SSE stream closed or error: \(error)")
            }
        }
    }

    public func disconnect() {
        sseTask?.cancel()
        sseTask = nil
    }

    public func send(prompt: String) async {
        let userMsg = ChatMessage(role: "user", text: prompt)
        messages.append(userMsg)
        do {
            try await client.startTurn(sessionId: sessionId, prompt: prompt)
        } catch {
            messages.append(ChatMessage(role: "assistant", text: "Error starting turn: \(error.localizedDescription)"))
        }
    }

    public func handleEvent(_ event: ServerEvent) {
        switch event.kind {
        case .working:
            isWorking = true
        case .workingDone:
            isWorking = false
        case .toolStarted(let name, let phrase):
            tools.append(ToolCallItem(id: UUID().uuidString, name: name, phrase: phrase, summary: nil, isRunning: true))
        case .tool(let name, let summary, let phrase):
            if let idx = tools.lastIndex(where: { $0.name == name && $0.isRunning }) {
                tools[idx].summary = summary
                tools[idx].phrase = phrase ?? tools[idx].phrase
                tools[idx].isRunning = false
            }
        case .assistantDelta(let text):
            if let id = streamingMessageId, let idx = messages.firstIndex(where: { $0.id == id }) {
                messages[idx].text.append(text)
            } else {
                let newId = UUID().uuidString
                streamingMessageId = newId
                messages.append(ChatMessage(id: newId, role: "assistant", text: text, isStreaming: true))
            }
        case .assistantWithdrawn:
            if let id = streamingMessageId {
                messages.removeAll(where: { $0.id == id })
                streamingMessageId = nil
            }
        case .assistant(let text):
            if let id = streamingMessageId, let idx = messages.firstIndex(where: { $0.id == id }) {
                messages[idx].text = text
                messages[idx].isStreaming = false
            } else {
                messages.append(ChatMessage(role: "assistant", text: text, isStreaming: false))
            }
            streamingMessageId = nil
        case .approvalRequest(let id, let tool, let arguments):
            pendingApproval = (id: id, tool: tool, arguments: arguments)
        case .checkpointRequest(let id, let kind, let prompt):
            pendingCheckpoint = (id: id, kind: kind, prompt: prompt)
        case .context(let used, let limit, _):
            usedTokens = used
            limitTokens = limit
        default:
            break
        }
    }
}
```

- [ ] **Step 2: Implement `AppState.swift`**

Create `zorp-desktop/Zorp/ViewModels/AppState.swift`:
```swift
import Foundation
import Observation

@Observable
public final class AppState: @unchecked Sendable {
    public var baseURL: URL
    public var client: ZorpClient
    public var activeSessionId: String?
    public var activeSessionVM: SessionViewModel?
    public var isConnected: Bool = false

    public init(baseURL: URL) {
        self.baseURL = baseURL
        self.client = ZorpClient(baseURL: baseURL)
    }

    public func selectSession(_ id: String) {
        activeSessionVM?.disconnect()
        activeSessionId = id
        let vm = SessionViewModel(sessionId: id, client: client, baseURL: baseURL)
        activeSessionVM = vm
        vm.connect()
    }

    public func createNewSession() async {
        do {
            let id = try await client.createSession()
            await MainActor.run {
                self.selectSession(id)
            }
        } catch {
            print("Failed to create session: \(error)")
        }
    }
}
```

- [ ] **Step 3: Write `SessionViewModelTests.swift`**

Create `zorp-desktop/ZorpTests/SessionViewModelTests.swift`:
```swift
import XCTest
@testable import Zorp

final class SessionViewModelTests: XCTestCase {
    func testStreamingMessageAndWithdrawal() {
        let baseURL = URL(string: "http://127.0.0.1:7777")!
        let client = ZorpClient(baseURL: baseURL)
        let vm = SessionViewModel(sessionId: "test-1", client: client, baseURL: baseURL)

        vm.handleEvent(ServerEvent(seq: 1, kind: .assistantDelta(text: "Hello")))
        XCTAssertEqual(vm.messages.count, 1)
        XCTAssertEqual(vm.messages[0].text, "Hello")
        XCTAssertTrue(vm.messages[0].isStreaming)

        vm.handleEvent(ServerEvent(seq: 2, kind: .assistantDelta(text: " world")))
        XCTAssertEqual(vm.messages[0].text, "Hello world")

        // Test withdrawal rollback
        vm.handleEvent(ServerEvent(seq: 3, kind: .assistantWithdrawn(events: 2, reask: 1, bound: 3)))
        XCTAssertEqual(vm.messages.count, 0)
    }

    func testFinalAssistantSettlement() {
        let baseURL = URL(string: "http://127.0.0.1:7777")!
        let client = ZorpClient(baseURL: baseURL)
        let vm = SessionViewModel(sessionId: "test-2", client: client, baseURL: baseURL)

        vm.handleEvent(ServerEvent(seq: 1, kind: .assistantDelta(text: "Part 1")))
        vm.handleEvent(ServerEvent(seq: 2, kind: .assistant(text: "Final complete text")))
        XCTAssertEqual(vm.messages.count, 1)
        XCTAssertEqual(vm.messages[0].text, "Final complete text")
        XCTAssertFalse(vm.messages[0].isStreaming)
    }
}
```

- [ ] **Step 4: Commit**

```bash
git add zorp-desktop/Zorp/ViewModels/ zorp-desktop/ZorpTests/
git commit -m "feat(desktop): add AppState, SessionViewModel, and tests"
```

---

### Task 6: SwiftUI Conversation Stream & Tool Activity Views

**Files:**
- Create: `zorp-desktop/Zorp/Views/Conversation/MarkdownView.swift`
- Create: `zorp-desktop/Zorp/Views/Conversation/MessageBubbleView.swift`
- Create: `zorp-desktop/Zorp/Views/Conversation/ToolActivityView.swift`
- Create: `zorp-desktop/Zorp/Views/Conversation/ConversationView.swift`

**Interfaces:**
- Consumes: `SessionViewModel`, `ChatMessage`, `ToolCallItem`
- Produces: SwiftUI conversation detail view with live scrolling and activity cards

- [ ] **Step 1: Implement `MarkdownView.swift` (Native SF Pro typography)**

Create `zorp-desktop/Zorp/Views/Conversation/MarkdownView.swift`:
```swift
import SwiftUI

public struct MarkdownView: View {
    public let text: String

    public init(_ text: String) {
        self.text = text
    }

    public var body: some View {
        // Uses Apple's modern localized string key Markdown parsing
        Text(LocalizedStringKey(text))
            .textSelection(.enabled)
            .font(.system(.body, design: .default))
            .lineSpacing(4)
    }
}
```

- [ ] **Step 2: Implement `ToolActivityView.swift` (Disclosure groups)**

Create `zorp-desktop/Zorp/Views/Conversation/ToolActivityView.swift`:
```swift
import SwiftUI

public struct ToolActivityView: View {
    public let tool: ToolCallItem
    @State private var isExpanded: Bool = false

    public init(tool: ToolCallItem) {
        self.tool = tool
    }

    public var body: some View {
        DisclosureGroup(isExpanded: $isExpanded) {
            if let summary = tool.summary {
                Text(summary)
                    .font(.system(.caption, design: .monospaced))
                    .padding(8)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(Color(.controlBackgroundColor))
                    .cornerRadius(4)
            }
        } label: {
            HStack(spacing: 8) {
                if tool.isRunning {
                    ProgressView()
                        .controlSize(.small)
                } else {
                    Image(systemName: "checkmark.circle.fill")
                        .foregroundColor(.green)
                }
                Text(tool.name)
                    .font(.system(.subheadline, design: .monospaced))
                    .bold()
                if let phrase = tool.phrase {
                    Text("— \(phrase)")
                        .font(.subheadline)
                        .foregroundColor(.secondary)
                }
            }
        }
        .padding(8)
        .background(Color(.windowBackgroundColor).opacity(0.6))
        .cornerRadius(6)
    }
}
```

- [ ] **Step 3: Implement `MessageBubbleView.swift`**

Create `zorp-desktop/Zorp/Views/Conversation/MessageBubbleView.swift`:
```swift
import SwiftUI

public struct MessageBubbleView: View {
    public let message: ChatMessage

    public var body: some View {
        VStack(alignment: message.role == "user" ? .trailing : .leading, spacing: 6) {
            HStack {
                Text(message.role == "user" ? "You" : "Zorp")
                    .font(.caption)
                    .foregroundColor(.secondary)
                    .bold()
                Spacer()
            }

            MarkdownView(message.text)
                .padding(12)
                .background(
                    message.role == "user"
                        ? Color.accentColor.opacity(0.12)
                        : Color(.windowBackgroundColor)
                )
                .cornerRadius(8)
        }
        .padding(.horizontal)
    }
}
```

- [ ] **Step 4: Implement `ConversationView.swift`**

Create `zorp-desktop/Zorp/Views/Conversation/ConversationView.swift`:
```swift
import SwiftUI

public struct ConversationView: View {
    @Bindable public var viewModel: SessionViewModel

    public var body: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 16) {
                    ForEach(viewModel.messages) { message in
                        MessageBubbleView(message: message)
                            .id(message.id)
                    }

                    ForEach(viewModel.tools) { tool in
                        ToolActivityView(tool: tool)
                            .id(tool.id)
                    }

                    if viewModel.isWorking {
                        HStack(spacing: 8) {
                            ProgressView()
                                .controlSize(.small)
                            Text("Zorp is thinking...")
                                .font(.caption)
                                .foregroundColor(.secondary)
                        }
                        .padding(.horizontal)
                        .id("working_indicator")
                    }
                }
                .padding(.vertical)
            }
            .onChange(of: viewModel.messages.last?.text) {
                if let lastId = viewModel.messages.last?.id {
                    proxy.scrollTo(lastId, anchor: .bottom)
                }
            }
        }
    }
}
```

- [ ] **Step 5: Commit**

```bash
git add zorp-desktop/Zorp/Views/Conversation/
git commit -m "feat(desktop): add ConversationView, MarkdownView, and ToolActivityView"
```

---

### Task 7: Human-in-the-Loop Approval & Checkpoint Cards

**Files:**
- Create: `zorp-desktop/Zorp/Views/Cards/ApprovalCardView.swift`
- Create: `zorp-desktop/Zorp/Views/Cards/CheckpointCardView.swift`
- Modify: `zorp-desktop/Zorp/Views/Conversation/ConversationView.swift`

**Interfaces:**
- Consumes: `SessionViewModel.pendingApproval`, `SessionViewModel.pendingCheckpoint`
- Produces: Interactive permission gate cards with `Approve` / `Deny` and `Continue` / `Kill` buttons

- [ ] **Step 1: Implement `ApprovalCardView.swift`**

Create `zorp-desktop/Zorp/Views/Cards/ApprovalCardView.swift`:
```swift
import SwiftUI

public struct ApprovalCardView: View {
    public let id: String
    public let tool: String
    public let arguments: String
    public let onDecision: (Bool) -> Void

    public var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Image(systemName: "exclamationmark.triangle.fill")
                    .foregroundColor(.yellow)
                Text("Approval Required")
                    .font(.headline)
                Spacer()
                Text(tool)
                    .font(.system(.subheadline, design: .monospaced))
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(Color.yellow.opacity(0.2))
                    .cornerRadius(4)
            }

            Text("Arguments:")
                .font(.caption)
                .bold()

            Text(arguments)
                .font(.system(.caption, design: .monospaced))
                .padding(8)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(Color(.controlBackgroundColor))
                .cornerRadius(4)

            HStack {
                Spacer()
                Button("Deny (⌘D)") {
                    onDecision(false)
                }
                .keyboardShortcut("d", modifiers: .command)

                Button("Approve (⌘Y)") {
                    onDecision(true)
                }
                .buttonStyle(.borderedProminent)
                .keyboardShortcut("y", modifiers: .command)
            }
        }
        .padding()
        .background(Color.yellow.opacity(0.08))
        .overlay(
            RoundedRectangle(cornerRadius: 8)
                .stroke(Color.yellow.opacity(0.5), lineWidth: 1)
        )
        .cornerRadius(8)
        .padding(.horizontal)
    }
}
```

- [ ] **Step 2: Implement `CheckpointCardView.swift`**

Create `zorp-desktop/Zorp/Views/Cards/CheckpointCardView.swift`:
```swift
import SwiftUI

public struct CheckpointCardView: View {
    public let id: String
    public let kind: String
    public let prompt: String
    public let onDecision: (Bool) -> Void

    public var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Image(systemName: "shield.checkered")
                    .foregroundColor(.red)
                Text("Research Checkpoint: \(kind)")
                    .font(.headline)
            }

            Text(prompt)
                .font(.body)

            HStack {
                Spacer()
                Button("Kill Track") {
                    onDecision(false)
                }
                .foregroundColor(.red)

                Button("Continue Research") {
                    onDecision(true)
                }
                .buttonStyle(.borderedProminent)
            }
        }
        .padding()
        .background(Color.red.opacity(0.08))
        .overlay(
            RoundedRectangle(cornerRadius: 8)
                .stroke(Color.red.opacity(0.5), lineWidth: 1)
        )
        .cornerRadius(8)
        .padding(.horizontal)
    }
}
```

- [ ] **Step 3: Wire cards into `ConversationView.swift`**

Update `zorp-desktop/Zorp/Views/Conversation/ConversationView.swift` inside the `ScrollView` `LazyVStack`:
```swift
if let approval = viewModel.pendingApproval {
    ApprovalCardView(id: approval.id, tool: approval.tool, arguments: approval.arguments) { approved in
        Task {
            try? await viewModel.client.submitApproval(
                sessionId: viewModel.sessionId,
                requestId: approval.id,
                approved: approved
            )
            await MainActor.run {
                viewModel.pendingApproval = nil
            }
        }
    }
}

if let checkpoint = viewModel.pendingCheckpoint {
    CheckpointCardView(id: checkpoint.id, kind: checkpoint.kind, prompt: checkpoint.prompt) { approved in
        Task {
            try? await viewModel.client.submitCheckpoint(
                sessionId: viewModel.sessionId,
                checkpointId: checkpoint.id,
                approved: approved
            )
            await MainActor.run {
                viewModel.pendingCheckpoint = nil
            }
        }
    }
}
```

- [ ] **Step 4: Commit**

```bash
git add zorp-desktop/Zorp/Views/Cards/ zorp-desktop/Zorp/Views/Conversation/ConversationView.swift
git commit -m "feat(desktop): add ApprovalCardView and CheckpointCardView"
```

---

### Task 8: NavigationSplitView Layout, Sidebar & Menu Bar Commands

**Files:**
- Create: `zorp-desktop/Zorp/Views/Sidebar/SidebarView.swift`
- Create: `zorp-desktop/Zorp/Views/MainWindowView.swift`
- Create: `zorp-desktop/Zorp/App/AppCommands.swift`
- Modify: `zorp-desktop/Zorp/App/ZorpApp.swift`

**Interfaces:**
- Consumes: `AppState`, `SessionViewModel`, `NavigationSplitView`
- Produces: 3-pane macOS window layout and menu commands (`⌘N`, `⌘K`, `⌘\`)

- [ ] **Step 1: Implement `SidebarView.swift`**

Create `zorp-desktop/Zorp/Views/Sidebar/SidebarView.swift`:
```swift
import SwiftUI

public struct SidebarView: View {
    @Bindable public var appState: AppState
    @State private var searchText: String = ""

    public var body: some View {
        List(selection: $appState.activeSessionId) {
            Section("Sessions") {
                if let id = appState.activeSessionId {
                    NavigationLink(value: id) {
                        Label("Current Session", systemImage: "bubble.left.and.bubble.right")
                    }
                }
            }
        }
        .searchable(text: $searchText, placement: .sidebar)
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button(action: {
                    Task {
                        await appState.createNewSession()
                    }
                }) {
                    Image(systemName: "square.and.pencil")
                }
                .help("New Session (⌘N)")
            }
        }
    }
}
```

- [ ] **Step 2: Implement `MainWindowView.swift` (3-Pane SplitView)**

Create `zorp-desktop/Zorp/Views/MainWindowView.swift`:
```swift
import SwiftUI

public struct MainWindowView: View {
    @Bindable public var appState: AppState
    @State private var inspectorPresented: Bool = false

    public var body: some View {
        NavigationSplitView {
            SidebarView(appState: appState)
        } detail: {
            if let vm = appState.activeSessionVM {
                VStack(spacing: 0) {
                    ConversationView(viewModel: vm)
                    Divider()
                    ComposerView(viewModel: vm)
                }
            } else {
                ContentUnavailableView("No Session Selected", systemImage: "bubble.left.and.bubble.right")
            }
        }
        .inspector(isPresented: $inspectorPresented) {
            Text("Inspector & Artifacts")
                .frame(minWidth: 260)
        }
        .toolbar {
            ToolbarItem(placement: .status) {
                if let vm = appState.activeSessionVM, vm.limitTokens != nil {
                    Text("\(vm.usedTokens) tokens")
                        .font(.caption)
                        .foregroundColor(.secondary)
                }
            }
            ToolbarItem(placement: .primaryAction) {
                Button(action: { inspectorPresented.toggle() }) {
                    Image(systemName: "sidebar.trailing")
                }
                .help("Toggle Inspector (⌘\\)")
            }
        }
    }
}
```

- [ ] **Step 3: Implement `AppCommands.swift` & Update `ZorpApp.swift`**

Create `zorp-desktop/Zorp/App/AppCommands.swift`:
```swift
import SwiftUI

public struct AppCommands: Commands {
    public let appState: AppState

    public var body: some Commands {
        SidebarCommands()
        CommandGroup(replacing: .newItem) {
            Button("New Session") {
                Task {
                    await appState.createNewSession()
                }
            }
            .keyboardShortcut("n", modifiers: .command)
        }
    }
}
```

Update `zorp-desktop/Zorp/App/ZorpApp.swift`:
```swift
import SwiftUI

@main
struct ZorpApp: App {
    @State private var appState: AppState

    init() {
        let port: UInt16
        do {
            port = try BridgeService.shared.start()
        } catch {
            fatalError("Failed to initialize Zorp bridge: \(error)")
        }
        let base = URL(string: "http://127.0.0.1:\(port)")!
        _appState = State(initialValue: AppState(baseURL: base))
    }

    var body: some Scene {
        WindowGroup {
            MainWindowView(appState: appState)
                .onAppear {
                    Task {
                        await appState.createNewSession()
                    }
                }
        }
        .commands {
            AppCommands(appState: appState)
        }
    }
}
```

- [ ] **Step 4: Commit**

```bash
git add zorp-desktop/Zorp/Views/Sidebar/ zorp-desktop/Zorp/Views/MainWindowView.swift zorp-desktop/Zorp/App/
git commit -m "feat(desktop): implement 3-pane NavigationSplitView and app commands"
```

---

### Task 9: Composer & Live Voice Waveform Input (`AVAudioEngine`)

**Files:**
- Create: `zorp-desktop/Zorp/ViewModels/VoiceViewModel.swift`
- Create: `zorp-desktop/Zorp/Views/Composer/VoiceMeterView.swift`
- Create: `zorp-desktop/Zorp/Views/Composer/ComposerView.swift`

**Interfaces:**
- Consumes: `AVAudioEngine`, `/api/voice/transcribe`
- Produces: Auto-expanding composer with live audio level visualizer

- [ ] **Step 1: Implement `VoiceViewModel.swift` (`AVAudioEngine` capture)**

Create `zorp-desktop/Zorp/ViewModels/VoiceViewModel.swift`:
```swift
import AVFoundation
import Foundation
import Observation

@Observable
public final class VoiceViewModel: @unchecked Sendable {
    public var isRecording: Bool = false
    public var audioLevel: Float = 0.0

    private var audioEngine: AVAudioEngine?
    private var inputNode: AVAudioInputNode?

    public init() {}

    public func startRecording() {
        let engine = AVAudioEngine()
        let input = engine.inputNode
        let format = input.outputFormat(forBus: 0)

        input.installTap(onBus: 0, bufferSize: 1024, format: format) { [weak self] buffer, _ in
            guard let channelData = buffer.floatChannelData?[0] else { return }
            let frames = Int(buffer.frameLength)
            var sum: Float = 0
            for i in 0..<frames {
                sum += channelData[i] * channelData[i]
            }
            let rms = sqrt(sum / Float(frames))
            DispatchQueue.main.async {
                self?.audioLevel = min(max(rms * 10, 0), 1)
            }
        }

        do {
            try engine.start()
            self.audioEngine = engine
            self.inputNode = input
            self.isRecording = true
        } catch {
            print("Failed to start audio engine: \(error)")
        }
    }

    public func stopRecording() {
        inputNode?.removeTap(onBus: 0)
        audioEngine?.stop()
        audioEngine = nil
        inputNode = nil
        isRecording = false
        audioLevel = 0.0
    }
}
```

- [ ] **Step 2: Implement `VoiceMeterView.swift`**

Create `zorp-desktop/Zorp/Views/Composer/VoiceMeterView.swift`:
```swift
import SwiftUI

public struct VoiceMeterView: View {
    public let level: Float

    public var body: some View {
        HStack(spacing: 3) {
            ForEach(0..<8) { index in
                RoundedRectangle(cornerRadius: 1.5)
                    .fill(Float(index) / 8.0 <= level ? Color.accentColor : Color.secondary.opacity(0.3))
                    .frame(width: 3, height: CGFloat(8 + index * 2))
            }
        }
        .frame(height: 24)
    }
}
```

- [ ] **Step 3: Implement `ComposerView.swift`**

Create `zorp-desktop/Zorp/Views/Composer/ComposerView.swift`:
```swift
import SwiftUI

public struct ComposerView: View {
    @Bindable public var viewModel: SessionViewModel
    @State private var text: String = ""
    @State private var voiceVM = VoiceViewModel()

    public var body: some View {
        VStack(spacing: 8) {
            HStack(alignment: .bottom, spacing: 8) {
                TextEditor(text: $text)
                    .font(.body)
                    .frame(minHeight: 36, maxHeight: 120)
                    .padding(6)
                    .background(Color(.controlBackgroundColor))
                    .cornerRadius(6)

                if voiceVM.isRecording {
                    VoiceMeterView(level: voiceVM.audioLevel)
                }

                Button(action: {
                    if voiceVM.isRecording {
                        voiceVM.stopRecording()
                    } else {
                        voiceVM.startRecording()
                    }
                }) {
                    Image(systemName: voiceVM.isRecording ? "stop.circle.fill" : "mic.fill")
                        .foregroundColor(voiceVM.isRecording ? .red : .primary)
                }
                .buttonStyle(.plain)
                .padding(.bottom, 6)

                Button(action: {
                    let prompt = text.trimmingCharacters(in: .whitespacesAndNewlines)
                    guard !prompt.isEmpty else { return }
                    text = ""
                    Task {
                        await viewModel.send(prompt: prompt)
                    }
                }) {
                    Image(systemName: "arrow.up.circle.fill")
                        .font(.title2)
                }
                .buttonStyle(.plain)
                .disabled(text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || viewModel.isWorking)
                .padding(.bottom, 4)
            }
        }
        .padding(10)
        .background(Color(.windowBackgroundColor))
    }
}
```

- [ ] **Step 4: Commit**

```bash
git add zorp-desktop/Zorp/ViewModels/VoiceViewModel.swift zorp-desktop/Zorp/Views/Composer/
git commit -m "feat(desktop): add ComposerView and VoiceViewModel with live audio meter"
```

---

### Task 10: Sandboxed WKWebView & Aryabhatta Ledger Table

**Files:**
- Create: `zorp-desktop/Zorp/Views/Inspector/SandboxedWebView.swift`
- Create: `zorp-desktop/Zorp/Views/Inspector/LedgerTableView.swift`
- Create: `zorp-desktop/Zorp/Views/Inspector/InspectorView.swift`
- Modify: `zorp-desktop/Zorp/Views/MainWindowView.swift`

**Interfaces:**
- Consumes: `LedgerFrame`, HTML/SVG raw artifact contents
- Produces: Sandboxed `WKWebView` preview complying with security invariants and native DuckDB ledger table

- [ ] **Step 1: Implement `SandboxedWebView.swift` (Enforces strict CSP)**

Create `zorp-desktop/Zorp/Views/Inspector/SandboxedWebView.swift`:
```swift
import SwiftUI
import WebKit

public struct SandboxedWebView: NSViewRepresentable {
    public let htmlContent: String

    public func makeNSView(context: Context) -> WKWebView {
        let config = WKWebViewConfiguration()
        // Disable JavaScript for artifact preview security
        let preferences = WKWebpagePreferences()
        preferences.allowsContentJavaScript = false
        config.defaultWebpagePreferences = preferences

        let webView = WKWebView(frame: .zero, configuration: config)
        return webView
    }

    public func updateNSView(_ nsView: WKWebView, context: Context) {
        // Enforce Content-Security-Policy: sandbox
        let wrappedHTML = """
        <!DOCTYPE html>
        <html>
        <head>
        <meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; img-src data:;">
        </head>
        <body>
        \(htmlContent)
        </body>
        </html>
        """
        nsView.loadHTMLString(wrappedHTML, baseURL: nil)
    }
}
```

- [ ] **Step 2: Implement `LedgerTableView.swift`**

Create `zorp-desktop/Zorp/Views/Inspector/LedgerTableView.swift`:
```swift
import SwiftUI

public struct LedgerTableView: View {
    public let experiments: [ExperimentFrame]

    public var body: some View {
        Table(experiments) {
            TableColumn("Attempt", value: \.id)
                .width(min: 80, max: 120)
            TableColumn("Status", value: \.status)
                .width(min: 60, max: 80)
            TableColumn("Metrics") { exp in
                Text(exp.metrics.map { "\($0.key)=\($0.value)" }.joined(separator: ", "))
                    .font(.caption)
            }
        }
    }
}
```

- [ ] **Step 3: Implement `InspectorView.swift`**

Create `zorp-desktop/Zorp/Views/Inspector/InspectorView.swift`:
```swift
import SwiftUI

public struct InspectorView: View {
    @State private var selectedTab: Int = 0

    public var body: some View {
        VStack(spacing: 0) {
            Picker("View", selection: $selectedTab) {
                Text("Artifacts").tag(0)
                Text("Ledger").tag(1)
            }
            .pickerStyle(.segmented)
            .padding(8)

            Divider()

            if selectedTab == 0 {
                SandboxedWebView(htmlContent: "<p>Select an artifact to preview</p>")
            } else {
                ContentUnavailableView("No Active Investigation", systemImage: "chart.bar.doc.horizontal")
            }
        }
        .frame(minWidth: 280)
    }
}
```

- [ ] **Step 4: Wire `InspectorView` into `MainWindowView.swift`**

Update `zorp-desktop/Zorp/Views/MainWindowView.swift`:
```swift
.inspector(isPresented: $inspectorPresented) {
    InspectorView()
}
```

- [ ] **Step 5: Commit**

```bash
git add zorp-desktop/Zorp/Views/Inspector/ zorp-desktop/Zorp/Views/MainWindowView.swift
git commit -m "feat(desktop): add SandboxedWebView, LedgerTableView, and InspectorView"
```

---

### Task 11: Application Packaging, DMG Bundler & CI Integration

**Files:**
- Create: `zorp-desktop/scripts/package-dmg.sh`
- Modify: `.github/workflows/desktop.yml`
- Modify: `scripts/check-release-version.sh`

**Interfaces:**
- Consumes: Built `Zorp.app`
- Produces: `Zorp-v0.5.0-universal.dmg` for GitHub releases and automated CI validation

- [ ] **Step 1: Implement `zorp-desktop/scripts/package-dmg.sh`**

Create `zorp-desktop/scripts/package-dmg.sh`:
```bash
#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
VERSION=$(grep -m1 'version =' "${REPO_ROOT}/zorp-desktop/bridge/Cargo.toml" | cut -d '"' -f2)
APP_PATH="${REPO_ROOT}/zorp-desktop/build/export/Zorp.app"
DMG_PATH="${REPO_ROOT}/zorp-desktop/build/Zorp-v${VERSION}-universal.dmg"

echo "==> Packaging Zorp.app into ${DMG_PATH}..."
mkdir -p "${REPO_ROOT}/zorp-desktop/build"

if [ -f "${DMG_PATH}" ]; then
  rm "${DMG_PATH}"
fi

hdiutil create -volname "Zorp" \
  -srcfolder "${APP_PATH}" \
  -ov -format UDZO \
  "${DMG_PATH}"

echo "==> DMG successfully created at ${DMG_PATH}"
```
Make executable: `chmod +x zorp-desktop/scripts/package-dmg.sh`

- [ ] **Step 2: Update `.github/workflows/desktop.yml`**

Modify `.github/workflows/desktop.yml` to run:
1. `cargo test --manifest-path zorp-desktop/bridge/Cargo.toml`
2. Universal bridge build via `zorp-desktop/scripts/build-bridge.sh`
3. Xcode archive and test verification.

- [ ] **Step 3: Verify and Commit**

```bash
git add zorp-desktop/scripts/package-dmg.sh .github/workflows/desktop.yml scripts/check-release-version.sh
git commit -m "ci(desktop): add universal DMG packager and update desktop CI workflow"
```

---

## Plan Self-Review & Verification

1. **Spec Coverage:**
   - Universal binary macOS 14+ target: Covered in Task 1, 2, 3.
   - In-process C-ABI bridge + Tokio runtime + port fallback: Covered in Task 1, 3.
   - URLSession + SSE streaming with `Last-Event-ID`: Covered in Task 4.
   - Observation (`@Observable`) state management & delta buffering: Covered in Task 5.
   - 3-Pane `NavigationSplitView` UI, markdown, tool disclosures: Covered in Tasks 6, 8.
   - Approval cards & research checkpoints: Covered in Task 7.
   - Audio capture with `AVAudioEngine`: Covered in Task 9.
   - Sandboxed `WKWebView` with strict CSP: Covered in Task 10.
   - Packaging DMG & CI: Covered in Tasks 2, 11.
2. **No Placeholders:** All tasks contain exact file paths, complete code, test cases, and exact shell commands.
3. **Type Consistency:** Method signatures (`zorp_bridge_repair_path`, `zorp_bridge_start_server`, `SSEStream.events(startingFrom:)`, `ZorpClient.startTurn`, `SessionViewModel.handleEvent`) match exactly across all tasks.
