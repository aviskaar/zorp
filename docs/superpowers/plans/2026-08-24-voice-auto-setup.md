# Voice Auto Setup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make one microphone click request permission, prepare a pinned local Qwen3-ASR runtime when needed, record audio, and insert its transcript into the composer.

**Architecture:** `zorp-voice` owns setup in a versioned virtual environment below the platform data directory. It first tries the pinned vLLM extra, recreates the environment with the pinned plain package when resolution fails, and starts only a loopback process. The plain-package path runs an embedded Flask server that reports model download and load state through its health response. `zorp-web` starts setup only from the human-triggered readiness POST and carries progress through the existing SSE. The browser requests microphone permission and readiness concurrently, records immediately, then waits for readiness before uploading.

**Tech Stack:** Rust 1.95, std process management, `dirs`, `rustix`, Flask, Qwen3-ASR 0.0.6, PyAV, soundfile, librosa, TypeScript, jsdom, Axum SSE.

**Spec:** `docs/superpowers/specs/2026-08-23-qwen3-asr-voice-input-design.md`, superseded for setup and browser ordering by the 2026-08-24 voice auto-setup request.

## Global Constraints

- Recorded audio goes only through the existing checked loopback client. Do not change its URL validation, pinned resolver, redirect policy, or proxy policy.
- Install exactly `qwen-asr==0.0.6` or `qwen-asr[vllm]==0.0.6` in a zorp-owned virtual environment. Never install as root or into system or user site-packages.
- Select vLLM by successful package resolution, not by an operating-system branch.
- Bind spawned runtimes only to the already validated loopback host and port.
- `ZORP_VOICE_AUTOSTART=0` prevents every setup and spawn operation.
- A human browser action is the only trigger for setup or recording.
- Render only fixed, short browser messages through `textContent`. Send raw failure details only to `console.error`.
- Do not use em dashes or en dashes as punctuation in repository prose.

---

### Task 1: Pinned Runtime Bootstrap and Embedded Server

**Files:**
- Create: `zorp-voice/src/bootstrap.rs`
- Create: `zorp-voice/src/transformers_server.py`
- Modify: `zorp-voice/src/lib.rs`
- Modify: `zorp-voice/src/client.rs`
- Modify: `zorp-voice/Cargo.toml`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`

**Interfaces:**
- Consumes: `LoopbackUrl`, `QwenAsr::status`, `QwenAsr::start_command`, `DEFAULT_VOICE_MODEL`, and `DEFAULT_VOICE_URL`.
- Produces: `QwenAsr::ensure_runtime`, `BootstrapProgress`, `BootstrapStage`, `BootstrapOutcome`, and `VOICE_AUTOSTART_VAR`.

- [ ] **Step 1: Write failing bootstrap tests**

  Add tests that drive a fake command runner through these real bootstrap decisions: an explicit `0` disables setup without invoking a command, a failed vLLM install recreates the virtual environment and installs `qwen-asr==0.0.6`, a successful vLLM install selects the fast backend, and each generated spawn command carries the checked loopback host and port as separate arguments. Add a Python syntax test that sends the embedded asset to `python3 -c "import ast,sys; ast.parse(sys.stdin.read())"`.

- [ ] **Step 2: Run the focused tests and verify RED**

  Run: `cargo test -p zorp-voice bootstrap`

  Expected: compilation fails because the bootstrap module and public interfaces do not exist.

- [ ] **Step 3: Implement the setup state machine**

  Resolve `dirs::data_local_dir()/zorp/voice/qwen-asr-0.0.6`, reject effective UID 0 through `rustix`, find `python3` or `python`, create a virtual environment with `python -m venv`, and invoke only that environment's `python -m pip --disable-pip-version-check --no-input install`. Try `qwen-asr[vllm]==0.0.6`. On failure, remove only the resolved versioned environment, recreate it, and install `qwen-asr==0.0.6`. Persist a backend marker and rewrite the embedded server asset at setup time. Use `std::process::Command` with separate arguments and no shell.

- [ ] **Step 4: Implement loopback runtime spawning**

  For vLLM, use the virtual environment's Python to run `huggingface_hub.snapshot_download` while reporting `downloading_model`, then spawn `qwen-asr-serve MODEL --host HOST --port PORT` while reporting `loading`. For the fallback, spawn the virtual environment's Python with the embedded server path and the same checked model, host, and port. Send stdout and stderr to the versioned runtime log, retain no shell command, and watch the child so an early exit becomes a bootstrap error.

- [ ] **Step 5: Implement the embedded Flask server**

  Parse only `--model`, `--host`, and `--port`; translate `localhost` to `127.0.0.1` and reject every non-loopback literal. Start Flask immediately and load on a background thread. Report `downloading_model`, `loading`, `ready`, or `error` from `GET /health`; keep `GET /v1/models` empty before ready. Download with `huggingface_hub.snapshot_download`, load with `Qwen3ASRModel.from_pretrained`, accept the exact chat audio data URL, decode soundfile-compatible bytes first and PyAV containers second, normalize integer arrays by their dtype maximum, mix to mono, resample with librosa to 16 kHz float32, call `transcribe`, and return exactly `language {language}<asr_text>{text}` in `choices[0].message.content`.

- [ ] **Step 6: Run focused and crate tests and verify GREEN**

  Run: `cargo test -p zorp-voice`

  Expected: all bootstrap, contract, and loopback tests pass. If the sandbox denies loopback binds, record the exact skipped or failed tests for the PR instead of generalizing the result.

---

### Task 2: Human-Triggered Setup and Staged Readiness SSE

**Files:**
- Modify: `zorp-web/src/voice.rs`
- Modify: `zorp-web/tests/voice.rs`

**Interfaces:**
- Consumes: `QwenAsr::ensure_runtime`, `BootstrapProgress`, `BootstrapStage`, and the current `voice_model` event.
- Produces: `voice_model` events with unchanged terminal `status` semantics and an added `stage` field.

- [ ] **Step 1: Write failing route tests**

  Add tests for ordered `creating_environment`, `installing`, `downloading_model`, `loading`, and `ready` events using a test bootstrap hook that performs no installation. Add a disabled-autostart test proving the readiness POST emits one terminal error and executes no setup command. Keep the existing JSON content-type and observed-ready tests.

- [ ] **Step 2: Run the focused tests and verify RED**

  Run: `cargo test -p zorp-web --features voice --test voice`

  Expected: assertions fail because readiness events have no stage and the route never starts setup.

- [ ] **Step 3: Connect setup to the readiness POST**

  Start `ensure_runtime` only inside `POST /api/voice/wait`. Forward setup progress over a channel into the existing SSE, then poll observed health every two seconds. Map embedded-server health stages into the same events and terminate on ready or error. Keep `GET /api/voice/status` read-only so capability loading cannot download or execute code, and keep the route absent from model tools.

- [ ] **Step 4: Keep status compatibility without rendering commands**

  Preserve the operator command in the status API only for the explicit `ZORP_VOICE_AUTOSTART=0` compatibility path. Do not send it through any readiness event. Keep endpoint validation errors server-side and let the browser use fixed copy.

- [ ] **Step 5: Run the focused route tests and verify GREEN**

  Run: `cargo test -p zorp-web --features voice --test voice`

  Expected: all voice route tests pass with ordered staged events.

---

### Task 3: Permission-First Browser Recording

**Files:**
- Modify: `web/src/voice-input.ts`
- Modify: `web/src/voice-readiness.ts`
- Modify: `web/src/api.ts`
- Modify: `web/src/main.ts`
- Modify: `web/test/voice-input.test.ts`
- Modify: `web/test/voice-api.test.ts`
- Modify: `web/index.html`
- Modify: `web/styles.css`

**Interfaces:**
- Consumes: staged `VoiceWaitEvent`, `getUserMedia`, `MediaRecorder`, and `insertTranscript` behavior.
- Produces: a permission-first recorder that waits for local readiness only before upload.

- [ ] **Step 1: Write failing browser tests**

  Replace the operator-command test with one that holds readiness pending and proves `getUserMedia` was called first. Prove recording begins while readiness is pending, stopping waits before upload, all five stage values produce fixed short lines, setup and transcription failures render one fixed human sentence while their details reach `console.error`, and hostile transcripts remain textarea value with a bubbled `input` event and caret-aware insertion. Extend stream parsing tests to require a valid stage.

- [ ] **Step 2: Run focused browser tests and verify RED**

  Run: `npm test -- --test-name-pattern='voice'`

  Expected: the permission ordering, stage, fixed-error, and removed-command assertions fail.

- [ ] **Step 3: Implement concurrent permission and setup**

  Check secure-context and browser API availability, then create the readiness promise before immediately calling `getUserMedia` in the same click turn. Start recording as soon as permission resolves. Let setup continue concurrently. On stop, release the stream, await readiness, upload, and call the unchanged caret-aware `insertTranscript`. Keep cancellation from uploading and consume readiness rejection so it never becomes unhandled.

- [ ] **Step 4: Render only fixed stage and error copy**

  Map `creating_environment`, `installing`, `downloading_model`, `loading`, and `ready` to one short sentence each through `textContent`. On setup, recording, or transcription failure, write one fixed sentence to the status node and pass the raw value to `console.error`. Keep the dedicated permission-denied sentence.

- [ ] **Step 5: Remove manual controls and stale types**

  Delete the command code block and wait button from `web/index.html`, remove their element fields and listeners from `voice-input.ts` and `main.ts`, remove stale command styling, and stop exposing `command` in the browser `VoiceStatus` type.

- [ ] **Step 6: Run browser tests and verify GREEN**

  Run: `npm test`

  Expected: all jsdom tests pass, including injection cases.

---

### Task 4: Record the Reversal and Synchronize Repository Guidance

**Files:**
- Modify: `docs/DECISIONS.md`
- Modify: `CLAUDE.md`
- Modify: `AGENTS.md`

**Interfaces:**
- Consumes: the completed runtime and browser behavior.
- Produces: an append-only 2026-08-24 decision and matching voice guidance in both instruction files.

- [ ] **Step 1: Add the append-only decision entry**

  Insert a new newest 2026-08-24 entry without editing the earlier voice decision. State that the old operator-only runtime is reversed because its vLLM dependency cannot resolve on macOS arm64, backend choice follows package resolution rather than platform names, the new download and execution power is bounded by a pinned package, owned virtual environment, non-root check, loopback bind, human POST, and `ZORP_VOICE_AUTOSTART=0`, and permission is requested before setup completes so browser consent is not delayed by the model download.

- [ ] **Step 2: Update both voice bullets identically**

  Replace only the stale runtime sentences in `CLAUDE.md` and `AGENTS.md`. Describe automatic pinned setup, the resolution-selected backend, embedded transformers server, staged readiness, permission-first ordering, and the unchanged loopback and untrusted-transcript rules.

- [ ] **Step 3: Check prose and synchronization**

  Run: `diff -u <(sed -n '/^- `zorp-voice/,/^- `memory/p' CLAUDE.md) <(sed -n '/^- `zorp-voice/,/^- `memory/p' AGENTS.md)`

  Expected: no difference in the voice block. Search changed prose for Unicode dash punctuation and remove any occurrence.

---

### Task 5: Full Verification, Review, and Delivery

**Files:**
- Inspect: every changed file and generated lockfile entry.

**Interfaces:**
- Consumes: all prior tasks.
- Produces: one reviewed commit and a pull request against `main`.

- [ ] **Step 1: Run Rust verification**

  Run in order: `cargo build --workspace`, `cargo test --workspace`, `cargo fmt --all`, `cargo test -p zorp-voice`, and `cargo test -p zorp-web --features voice`.

  Expected: every command exits zero. Record socket-test skips or sandbox failures by exact test name.

- [ ] **Step 2: Run web verification**

  From `web/`, run in order: `npm run check`, `npm test`, and `npm run build`.

  Expected: every command exits zero.

- [ ] **Step 3: Inspect the final diff and requirement coverage**

  Run: `git diff --check`, `git status --short`, and `git diff --stat origin/main...HEAD` after committing. Confirm each numbered user requirement maps to code or documentation and no loopback protection changed.

- [ ] **Step 4: Commit and review**

  Commit with `feat(voice): set up local transcription automatically`. Request a read-only code review over `origin/main..HEAD`, fix every critical or important finding test-first, rerun affected checks, and amend or add a fix commit.

- [ ] **Step 5: Push and create the PR**

  Run: `git push -u origin feat/voice-auto-setup`, then `gh pr create --base main --head feat/voice-auto-setup` with a short direct body. Include the backend-selection choice, the status-API compatibility choice, exact verification results, and every sandbox-limited or unexercised install step.
