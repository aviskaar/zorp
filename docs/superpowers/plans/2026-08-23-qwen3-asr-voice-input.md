# Qwen3-ASR Voice Input Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Record browser audio, transcribe it with a loopback-only Qwen3-ASR runtime, and place editable text in the composer.

**Architecture:** A standalone `zorp-voice` crate owns a pinned loopback HTTP client for `qwen-asr` 0.0.6 and vLLM 0.14.0. `zorp-web` exposes always-registered status, readiness, and transcription routes behind an opt-in feature. A focused TypeScript module owns recording, readiness, and composer insertion.

**Tech Stack:** Rust 1.95, ureq 2, Axum 0.7, TypeScript, MediaRecorder, jsdom, qwen-asr 0.0.6, vLLM 0.14.0

**Spec:** `docs/superpowers/specs/2026-08-23-qwen3-asr-voice-input-design.md`

## Global Constraints

- Audio goes to a checked loopback endpoint or nowhere.
- The HTTP client uses a pinned resolver, no redirects, and no environment proxy.
- `ZORP_VOICE_URL` and `ZORP_VOICE_MODEL` are the only overrides.
- Voice input is a non-default `zorp-web` feature.
- Browser text uses `textContent` or textarea `value`, never HTML strings.
- The transcript remains editable and is never sent automatically.
- Shared dependency versions stay in the root manifest.

---

### Task 1: Standalone voice client

**Files:**
- Create: `zorp-voice/Cargo.toml`
- Create: `zorp-voice/src/lib.rs`
- Create: `zorp-voice/src/loopback.rs`
- Create: `zorp-voice/src/client.rs`
- Create: `zorp-voice/tests/common/mod.rs`
- Create: `zorp-voice/tests/no_remote.rs`
- Create: `zorp-voice/tests/no_proxy.rs`
- Create: `zorp-voice/tests/runtime.rs`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`

**Interfaces:**
- Consumes: qwen-asr and vLLM `health`, `v1/models`, and `v1/chat/completions` routes.
- Produces: `QwenAsr::from_env`, `status`, and `transcribe`; `VoiceStatus` and `Transcription`.

- [ ] **Step 1: Write failing boundary and wire tests**

Add canary tests that assert zero connections for off-device URLs, redirects,
proxies, and fallback environment variables. Add mock runtime tests for the
three endpoint paths, the chat audio data URL, and the Qwen language envelope.

- [ ] **Step 2: Run tests to verify failure**

Run: `cargo test -p zorp-voice`

Expected: failure because the new crate and its types do not exist.

- [ ] **Step 3: Implement the guarded client**

Implement the loopback URL and resolver types, one hardened ureq agent, chat
audio upload, loaded-model matching, and
`language <tag><asr_text><text>` parsing.

- [ ] **Step 4: Run tests to verify success**

Run: `cargo test -p zorp-voice`

Expected: all client and connection-counting tests pass.

### Task 2: zorp-web voice API

**Files:**
- Create: `zorp-web/src/voice.rs`
- Create: `zorp-web/tests/voice.rs`
- Modify: `zorp-web/Cargo.toml`
- Modify: `zorp-web/src/lib.rs`
- Modify: `zorp-web/src/api.rs`
- Modify: `zorp-web/tests/capabilities.rs`

**Interfaces:**
- Consumes: the `zorp-voice` client from Task 1.
- Produces: `GET /api/voice/status`, `POST /api/voice/wait`, `POST /api/voice/transcribe`, and `capabilities.voice`.

- [ ] **Step 1: Write failing route tests**

Cover the feature-off 501 answers, the explanatory status response, a bounded
accepted audio upload, the readiness SSE stream, and exact equality between
`capabilities.voice` and `/api/voice/status`.

- [ ] **Step 2: Run tests to verify failure**

Run: `cargo test -p zorp-web --features voice`

Expected: failure because the feature and routes do not exist.

- [ ] **Step 3: Implement routes from one observed status function**

Register every route unconditionally. Use `spawn_blocking` for ureq work and
an Axum SSE response backed by observed readiness polls. Require a JSON
content type on the readiness POST. Keep
the upload body limit at 25 MiB and return clear 400, 413, 415, 501, and 503
responses.

- [ ] **Step 4: Run tests to verify success**

Run: `cargo test -p zorp-web --features voice`

Expected: all voice route and capability tests pass.

### Task 3: Browser voice controller

**Files:**
- Create: `web/src/voice-input.ts`
- Create: `web/test/voice-input.test.ts`
- Modify: `web/src/api.ts`
- Modify: `web/src/main.ts`
- Modify: `web/index.html`
- Modify: `web/styles.css`
- Modify: `web/test/send-control.test.ts`

**Interfaces:**
- Consumes: the three voice API routes from Task 2 and browser MediaRecorder APIs.
- Produces: `createVoiceInput`, microphone state changes, readiness messages, and editable composer insertion.

- [ ] **Step 1: Write failing DOM and controller tests**

Test insecure context, denied permission, cancel without upload, missing model
command, readiness stream termination, and hostile transcript text remaining
textarea text.

- [ ] **Step 2: Run tests to verify failure**

Run: `npm test` from `web/`.

Expected: failure because the module and markup do not exist.

- [ ] **Step 3: Implement recording and insertion**

Add a microphone button, cancel button, status region, and command element.
Keep send and stop as one primary control. Make the controller own media
tracks and always stop them after stop or cancel. Insert at the textarea
selection, dispatch `input`, and focus without submitting the form.

- [ ] **Step 4: Run web checks**

Run from `web/`: `npm run check`, `npm test`, `npm run build`.

Expected: type checking, tests, and bundling pass.

### Task 4: Documentation and policy record

**Files:**
- Modify: `README.md`
- Modify: `CLAUDE.md`
- Modify: `AGENTS.md`
- Modify: `docs/DECISIONS.md`

**Interfaces:**
- Consumes: the final feature names, environment variables, and runtime contract.
- Produces: operator setup and a permanent architecture decision.

- [ ] **Step 1: Document exact setup and defaults**

Document `pip install "qwen-asr[vllm]==0.0.6"`, `qwen-asr-serve`, the `voice`
Cargo feature, both environment variables, and the three endpoint contract
confirmed in official docs and source.

- [ ] **Step 2: Add the decision and synchronized agent guidance**

Record the loopback-only rule, no remote fallback, command plus observed
readiness fallback, and untrusted editable transcript. Add the matching
`zorp-voice` bullet to both agent instruction files.

### Task 5: Full verification and delivery

**Files:**
- Modify: only files required by failures found during verification.

**Interfaces:**
- Consumes: Tasks 1 through 4.
- Produces: a reviewed commit and pull request against `main`.

- [ ] **Step 1: Format and run every requested check**

Run all Rust workspace checks, both explicit voice feature test commands, and
all three web commands from the spec.

- [ ] **Step 2: Review the diff**

Check for accidental runtime dependencies, any `innerHTML`, any remote
provider or fallback, and any prose punctuation forbidden by repository
style.

- [ ] **Step 3: Commit and publish**

Commit the complete change, push `web/voice-input-qwen3-asr` to origin, and
create a pull request against `main`. The PR body names qwen-asr 0.0.6,
the confirmed endpoints, the versioned documentation used, and every check
run.
