# zorp: optimizations and suggestions

A code-level review of the whole workspace, pinned to commit `0c0305e`. It
covers the core crate, `zorp-agent`, the research stack (`zorp-track` plus the
validate/investigate/co-write/deliver capabilities), `zorp-mcp`, and
`zorp-eval`.

Findings are grouped by theme and ranked so the highest-impact items sit at the
top of each section. Every entry names a file and line so it can be acted on
directly.

**Status (2026-08-14): acted on.** The findings in this document were
implemented in the optimization branch that followed the review. The document
is kept as the review record; line references are pinned to `0c0305e` and no
longer match the fixed code.

## How to read this

- **Correctness and safety** first, because a few of these quietly defeat
  features the project advertises (tamper evidence, the command denylist, the
  kill threshold).
- **Performance** second: real allocations and rescans in hot loops, not
  micro-tuning.
- **Build, CI, and dependencies** third: the fastest wins in wall-clock terms.
- **Duplication and structure** last, plus a short list of what is already done
  well so none of it gets "fixed" by mistake.

Severity tags: **[critical]** defeats a security or integrity guarantee,
**[high]** wrong results or a broken user path, **[medium]** meaningful but
bounded, **[low]** cleanup.

---

## 1. Correctness and integrity

These are the ones to look at first. Several undermine a property the paper and
README claim zorp has.

### 1.1 The pre-registered kill threshold is never enforced [critical]

`zorp-agent/src/investigate/mod.rs:149-156`. `kill_threshold` is only ever
formatted into a prompt string; it is never compared to `attempt.metric_value`.
The whole point of pre-registration is that a committed threshold kills the
track when the evidence misses it. Today a track that badly misses its
threshold, under `--yes`/`AutoApprove`, is auto-approved and stays `Active`.

Fix: after `parse_attempt_result`, compute the breach and act on it. Store a
direction (`lower_is_better` / `higher_is_better`) in `prereg.md` and the
`preregistrations` table so the comparison has defined semantics: today the
threshold is a bare number with no comparison operator, so it cannot be
enforced even in principle. A breach must be exempt from `AutoApprove`, which is
the one decision in the whole flow where auto-approving defeats the product's
stated purpose.

### 1.2 The corruption-rebuild path launders tampering [critical]

`zorp-track/src/track.rs:190-222`. `rebuild_from_prereg_files` recomputes the
SHA-256 from whatever `prereg.md` is on disk *now*, then stores that as the
authoritative hash. So any path that loses the DuckDB row (delete it, or corrupt
one byte of `zorp.duckdb` to trigger `open_store_recovering_from_corruption` at
`project.rs:55-69`) causes a tampered file to be silently re-blessed, after
which `verify_prereg_integrity` passes. The tamper-evidence guarantee is
defeated by the recovery path that is supposed to protect it.

Fix: make git the root of trust on rebuild. The code already runs
`git log -1 --format=%H -- prereg.md`; extend it to `git show <commit>:prereg.md`,
hash that blob, and compare to the working tree. A mismatch must be
`IntegrityMismatch`, not a fresh row. With no git commit, mark the rebuilt row
`unverified` rather than presenting it as equivalent to a committed one.

### 1.3 `git_commit_hash` is recorded but never verified [high]

`zorp-track/src/prereg.rs:243-275`. `verify_prereg_integrity` reads only
`file_path` and `file_hash`; the `git_commit_hash` column is write-only. The
git half of the tamper-evidence story enforces nothing, so a
`git commit --amend`/`reset` that rewrites the pre-registration commit is
undetectable.

Fix: when `git_commit_hash` is set, also run `git cat-file -e {h}` and
`git show {h}:prereg.md | sha256`, and compare. Add a test that amends the commit
and asserts `IntegrityMismatch`.

### 1.4 `$(...)` command substitution bypasses the run_command denylist [critical]

`zorp-agent/src/policy.rs:124-134`. `deny_reason` blocks backticks explicitly,
but `tokenize_command` treats `$` as an ordinary word character, so `$(...)`
never becomes its own token. `echo $(sudo rm -rf /)` tokenizes with `echo` as
the executable, matches nothing, and resolves to `Ask`, which is `Allow` under
`Preset::Full` or `--yes`. `/bin/sh -c` in `sandbox.rs:87` then expands and runs
the substitution. `<(...)`, `>(...)`, and `${VAR}` indirection have the same
hole.

Fix: treat `$(`, `<(`, `>(` like the backtick case, or better, recurse into the
balanced substitution body and run `deny_reason` on it, exactly as is already
done for `sh -c` payloads at `policy.rs:155`.

### 1.5 `rm -rf /*` and friends pass the root-rm guard [high]

`zorp-agent/src/policy.rs:158-164`. The guard only matches an argument equal to
`/` or starting with `/../`. `rm -rf /*`, `rm -rf /bin`, `rm -rf ~`, and
`rm -rf ../..` all miss it and become `Allow` under `Preset::Full`.

Fix: deny when `rm` carries a recursive+force pair and any argument that, after
`..`/`~` expansion, resolves outside the canonicalized `repo_root`
(`sandbox.rs:57` already canonicalizes it).

### 1.6 Redirect denylist only catches one spelling [high]

`zorp-agent/src/policy.rs:130-132`. The four substring patterns
(`> /`, `>/`, `>> /`, `>>/`) miss `> ~/.ssh/authorized_keys`, `>|/etc/hosts`,
and `>${HOME}/x`.

Fix: tokenize redirect operators as distinct tokens in `tokenize_command` and
evaluate the target path against `repo_root`, rather than substring-matching the
raw string.

### 1.7 zorp-mcp protocol gaps that break real servers [high]

Concentrated in `zorp-mcp`, these make the client fail against compliant
servers:

- **No `notifications/initialized` after `initialize`** (`server.rs:59-62`).
  SDK-based servers gate `tools/list` on it and will hang or error. Add a
  `send_notification` to the `Transport` trait and call it at the end of
  `initialize()`.
- **`Mcp-Session-Id` is dropped** (`transport/streamable_http.rs:50-65`). The
  header from the initialize response is never captured or echoed, so every
  session-oriented server (GitHub's MCP endpoint, the reference TS server)
  rejects request #2. Capture it and set it plus `MCP-Protocol-Version` on
  later requests.
- **JSON-RPC ids typed `u64`** (`protocol.rs:21`). The spec allows string ids;
  a server echoing a string id fails deserialization with no recovery. Use
  `Option<Value>` with an `id_matches` helper, and `#[serde(default)]` on
  `jsonrpc`.
- **`tools/list` ignores `nextCursor`** (`server.rs:65-88`). Paginated servers
  expose only their first page of tools; the agent then reports real tools as
  missing. Loop on the cursor.

### 1.8 Anthropic plus reasoning mode always sends an invalid request [high]

`zorp-agent/src/reasoning.rs:101-110` vs `provider.rs:42`. `max_tokens` defaults
to 4096 while `High`/`XHigh` set `thinking.budget_tokens` to 24000/32000.
Anthropic requires `max_tokens > budget_tokens`, so
`zorp-agent --provider anthropic --reasoning high` (without an explicit
`--max-tokens`) fails every request with a 400. A test at `model.rs:1030-1051`
currently asserts the invalid combination, locking in the bug.

Fix: clamp `max_tokens = max_tokens.max(budget + 1024)` after computing the
budget, or reject the combination at config resolution with a clear message.
Change the test to assert `max_tokens > budget_tokens`.

### 1.9 HTTP error bodies are discarded across the workspace [high]

`src/lib.rs:67-75`. `ureq` returns `Err(Status(code, resp))` on non-2xx and its
`Display` prints only the status code; the response body, which is where
providers put "invalid api key", "model not found", "context length exceeded",
is dropped. `src/main.rs:50` then prints a bare `zorp: status code 400`. Every
downstream crate that calls `zorp_raw` inherits the unactionable message.

Fix: match the error and rebuild it with the body included. This is the
single highest-value ergonomics fix in the core, and it fixes four call sites at
once. Add the missing test asserting the server's error body reaches the caller.

### 1.10 `zorp-eval` silently fabricates experiment results [high]

For a harness whose entire purpose is trustworthy measurement, three separate
paths turn "could not evaluate" into a recorded pass or fail:

- **A malformed trace line becomes "all contracts failed"**
  (`runner.rs:209` + `contracts.rs:45-51`). A killed agent's truncated final
  line errors the whole file, `unwrap_or_default()` makes it an empty event
  list, and every required predicate then reads false and is written as a
  genuine `fail`. Skip-and-count bad lines; record `trace_unavailable` instead
  of evaluating contracts.
- **Unknown predicate ids evaluate to `false`** (`contracts.rs:403`). A typo in
  a contract YAML becomes a permanent required-predicate violation, or a silent
  pass in a `forbidden` list. Validate every id against the known set at load
  time and fail loudly.
- **Missing `seq` collapses to 0** (`contracts.rs:53-55`). Every ordering
  predicate is built on `seq_of`; a trace with no `seq` makes all comparisons
  false, so a never-evaluated run reports "pass". Treat a seq-less event as
  unevaluable.

Also: `LlmRubricGrader` unconditionally returns pass (`grader.rs:99-103`, with a
test asserting it), and the `Eval` command is a no-op that initializes the DB and
exits without running any grader (`main.rs:9-14`). Either wire the grader
subsystem in or delete it until it exists.

### 1.11 Primary-key collisions from millisecond-only ids [medium]

`zorp-track/src/checkpoint.rs:80`, `experiment.rs:77`, `experiment.rs:119`. Ids
are `format!("...-{}", now_millis())`; two operations in the same millisecond
violate the `PRIMARY KEY` and hard-error. `validation.rs:14-17` already solved
this with a `VALIDATION_SEQ: AtomicU64`; the other three never got the fix.
`record_checkpoint` under `AutoApprove` does no I/O between calls, so back-to-back
checkpoints collide easily.

Fix: hoist the atomic into a shared `crate::id::next_seq()` and append it in all
four id constructors, or use DuckDB sequences.

### 1.12 Terminal left unusable on any REPL panic [medium]

`zorp-agent/src/main.rs:1308,1412`. Raw mode is enabled with `unwrap()` and
`disable_raw_mode` runs only at the natural loop exit, so any panic inside the
chat loop drops the user into a shell with echo off.

Fix: wrap raw mode in an RAII guard whose `Drop` restores the terminal, and fall
back to the existing non-TTY path on the initial failure.

### 1.13 Smaller correctness gaps [medium/low]

- **`has_search_tool` accepts any MCP tool** (`validate/mod.rs:28-30`): checks
  the bare `mcp__` prefix, so a non-search server passes the "search-capable"
  gate. `deliver/mod.rs:10-12` does the specific-prefix check correctly. Mirror
  it.
- **Malformed tool-call args become `null`** (`model.rs:289`):
  `from_str(s).unwrap_or(Value::Null)` hides the real error ("your arguments
  were not valid JSON") behind a downstream "missing field". Keep the raw string
  or feed the parse error back as the tool result.
- **MCP tool failures reported as successes** (`mcp_adapter.rs:27`): the summary
  `"mcp error"` isn't recognized by the trace/denial-streak logic, so
  `agent.rs:728` records `success: true`. Return `Err(ToolError)` and use the
  uniform error path.
- **Trust-store and TOFU writes swallow I/O errors** (`trust.rs:62-73`,
  `tofu.rs:42-44`): `let _ = fs::write(...)` reports a failed write as success,
  so a user who answers "trust" is re-prompted forever. Write to a temp file,
  set mode `0o600`, rename, and surface the error.
- **`--flavor` path has no component validation** (`flavor.rs:253-269`):
  `--flavor ../../../../tmp/evil` loads an arbitrary manifest that can carry
  shell commands and permissive approval settings. `/capsule-create` already
  rejects traversal; make flavor loading consistent.

---

## 2. Performance

Real allocations and rescans on hot paths, ordered by how often the path runs.

### 2.1 A fresh HTTP agent (and TLS config) is built on every request [high]

`src/lib.rs:57-62`. `agent()` runs per `zorp_raw` and per `zorp_stream`, and
each `AgentBuilder::build()` constructs a new connection pool and a fresh rustls
config with a new root store. The agent's tool loop pays a full TLS handshake
every turn and never reuses keep-alive.

Fix: `static AGENT: OnceLock<ureq::Agent>` and clone it (a cheap `Arc` clone).

### 2.2 Conversation history is unbounded and fully re-serialized every turn [high]

`zorp-agent/src/agent.rs:554-618`, `model.rs:317-374`. `self.messages` only
grows, and `messages_to_body` rebuilds the entire JSON body from scratch each
iteration. With 32 KB tool-result caps, a 20-step run holds ~640 KB of `String`
and re-serializes all of it 20 times.

Fix: cache the serialized `Value` per message (they are append-only and
immutable after push) and rebuild only the tail; add a history budget that drops
or summarizes the oldest tool-result bodies past a byte threshold. The crate has
no such mechanism today.

### 2.3 Images are base64 re-encoded on every turn [high]

`src/model.rs:335`, `src/provider.rs:96`. Every image part is
`STANDARD.encode(data)`-ed on every request. A session with three screenshots
re-encodes several MB per model call for the rest of the session.

Fix: memoize the base64 string on the `ContentPart::Image` (a `OnceCell<String>`)
so encoding happens once per image.

### 2.4 Missing indexes on every SQLite and DuckDB table [high]

- **Session DB** (`zorp-agent/src/session.rs:11-52`): `messages`,
  `file_changes`, and `message_images` are all queried by `session_id` with no
  index, against a single file that accumulates every session ever. `/resume`
  and `--continue` degrade with total historical usage, not session size. Add
  `idx_messages_session(session_id, seq)` and the two siblings.
- **zorp-track** (`zorp-track/src/schema.rs:1-58`): every table has only its
  primary key. `metrics(experiment_id)` is the biggest win because
  `record_metric` scans it per insert (see 2.5). Add indexes on the
  foreign-key columns.

### 2.5 `record_metric` does `SELECT COUNT(*)` per insert [high]

`zorp-track/src/experiment.rs:114-118`. Every metric insert full-scans the
`metrics` table (no index) to compute `seq`, so it is O(n²) over a run, and
read-then-write with no transaction means concurrent inserts produce duplicate
`seq` values, silently breaking the documented ordering contract.

Fix: `INSERT ... SELECT COALESCE(MAX(seq), -1) + 1 FROM metrics WHERE
experiment_id = ?` as one statement, or a DuckDB sequence.

### 2.6 N+1 queries in the research stack [medium]

- **`verify_all_prereg_integrity` runs one query per track, plus a full re-read
  and re-hash of every `prereg.md`, on every `Project::open`**
  (`track.rs:238-249`, `project.rs:99-100`), i.e. on every research subcommand.
  Replace the loop with one `SELECT`, and gate the disk re-hash on an
  `(mtime, len)` check or move full verification behind an explicit
  `track verify` subcommand.
- **`co_write::all_metrics` queries metrics per experiment**
  (`co_write/mod.rs:15-23`). Add a single `metrics_for_track` join.

### 2.7 Streaming redaction is O(bytes x secrets), byte at a time [medium]

`zorp-agent/src/sandbox.rs:388-402`. For every output byte, `redact_pending`
scans all secrets with `starts_with` and pushes one byte. A `cargo build`
emitting 5 MB with 15 secret env vars is ~75M `starts_with` calls plus 5M
single-byte pushes.

Fix: a `[bool; 256]` first-byte table to skip the secret loop for non-matching
bytes, and batch-copy runs of non-matching bytes in one push. Related:
`secret_values()` re-scans and re-sorts the whole environment on every `run()`
(`sandbox.rs:86,252-269`); snapshot it into a process-level `OnceLock`.

### 2.8 Cheaper hot-path allocations [medium/low]

- **`Message::text()` allocates a Vec plus a String every call** (`model.rs:80-89`),
  the hottest accessor in the crate. Return `Cow<str>`, borrowing for the
  single-`Text`-part case that covers ~all messages.
- **`TraceIdentity` cloned ~10x per step even when tracing is off**
  (`agent.rs`, many lines). Gate the construction behind
  `self.trace_file.is_some()` or store it as `Arc`.
- **Per-token stdout lock in the streaming closure** (`src/main.rs:27-32`):
  hoist `io::stdout().lock()` above the closure.
- **SSE lines allocate a `String` per frame plus a deep `Value` clone**
  (`src/lib.rs:141,146,188,54`): reuse one buffer with `read_line` + `clear()`.
- **Regex and skin rebuilt repeatedly**: `IMAGE_REF` regex per keypress
  (`main.rs:250`) and `MadSkin::default()` per assistant message
  (`render.rs:101`) both belong in a `OnceLock`.
- **`content_hash` allocates a String per byte** (`flavor.rs:8-16`): use one
  `String::with_capacity(64)` and `write!`.

### 2.9 LanceDB: ~390 crates for a write-only, never-read sink [high, build-time]

`zorp-track` pulls the full arrow/datafusion/lancedb tree (measured: 397 of the
crate's 404 dependencies). It is written by `validate::run` but never read:
`library.rs` exposes only `table_names()` and `insert_source()`, there is no
query or nearest-neighbor API anywhere, and the eager `library` table
(`library.rs:41-54`) is never written to. The citations `co_write` actually
reads come from the DuckDB `validations` JSON columns, so the vector copy is
redundant today.

Fix: put LanceDB behind a non-default `library` cargo feature and make
`Project::library` lazy (`OnceCell`), so `co_write`/`deliver` never build or link
it, or cut it until there is a retrieval story and keep citations in the DuckDB
columns already in use. Also drop `rt-multi-thread` from `zorp-track`'s tokio
features (`Cargo.toml:10`): the code builds a current-thread runtime only.

---

## 3. Build, CI, and dependencies

The fastest wall-clock wins in the review.

### 3.1 `panic = "abort"` defeats a `catch_unwind` in the agent [high]

`Cargo.toml:21` sets `panic = "abort"` workspace-wide, but
`zorp-agent/src/tools/subagent.rs:525` wraps subagent execution in
`catch_unwind`. That guard is inert in every release build, so a subagent panic
kills the whole process in production while passing in debug/test. Decide which
you want: drop `panic = "abort"` (a few KB of unwind tables) so the guard works,
or set the release profile to `panic = "unwind"` and keep the guard. Note the
same setting makes several `expect`s abort-without-unwind, e.g.
`zorp-mcp/src/transport/stdio.rs:31` on model-supplied tool args.

### 3.2 `Cargo.lock` is untracked [high]

`.gitignore:2` ignores it, so CI resolves fresh every run: builds are
non-reproducible, an upstream semver-compatible release can break `main` with no
local repro, and `Swatinem/rust-cache` keys partly on the lockfile so cache hits
degrade. Commit it and add `--locked` to CI cargo invocations.

### 3.3 CI never compiles the research feature or `zorp-track` [high]

`.github/workflows/ci.yml:28` excludes `zorp-track`, which also drops
`zorp-agent --features research`, so an entire crate plus a feature-gated agent
surface can stop compiling while `main` stays green. Add a second job (nightly
schedule plus a paths-filtered `pull_request` trigger) running
`cargo test -p zorp-agent --features research --locked`; even a
`cargo check -p zorp-track` on the cheap path catches most of the rot. While
there: add `cargo fmt --check` and `cargo clippy -- -D warnings`, a
`macos-latest` matrix leg (the crate uses `crossterm`/`libc`/`OwnedFd` and
`install.sh` targets macOS), and drop the `protobuf-compiler` apt step, whose
only consumer is the excluded `zorp-track`.

### 3.4 `install.sh` throws away the whole build cache [high]

`install.sh:6` runs `cargo clean` before every install, then `install.sh:8`
builds `--workspace` (including `zorp-track` and `zorp-eval`, neither installed).
Every install becomes a 10-plus-minute from-scratch build including DuckDB's
bundled C++ and the lance/arrow tree.

Fix: delete the `cargo clean`, build only `-p zorp -p zorp-agent --release
--locked`, and use `install -m 755` rather than `cp` to avoid `ETXTBSY` when
upgrading over a running binary.

### 3.5 Profile and dependency-weight tuning [medium]

- **Size profile applied to the DuckDB/arrow crates too** (`Cargo.toml:17-22`):
  `opt-level = "z"` plus full `lto` plus `codegen-units = 1` is defensible for
  the 333-line core but is the most expensive knob in the whole build when
  applied to `zorp-track`, and `"z"` pessimizes the analytic hot paths the
  research feature exists for. Add a `[profile.release.package.zorp-track]` with
  `opt-level = 2`, and consider `lto = "thin"`.
- **Unoptimized dependencies in test runs**: add `[profile.dev.package."*"]
  opt-level = 2` so bundled `rusqlite`/`duckdb` compile optimized once and cache.
- **`zorp-eval` drags in a second HTTP and TLS stack**: `reqwest 0.11` with
  default features pulls native-tls/OpenSSL plus hyper 0.14, while `lancedb`
  pulls reqwest 0.12 + rustls, so the workspace links two of each. And `reqwest`
  is actually dead in `zorp-eval` (only referenced in a comment). Remove it; if
  the LLM grader is implemented later, use the workspace's `ureq`.
- **`zorp-eval` uses `tokio` `full` for a synchronous program** (`Cargo.toml:8`,
  `main.rs:5`): nothing in the binary path awaits. Drop `#[tokio::main]` and
  narrow the features.
- **`ureq` in `zorp-agent` exists for one Ollama tags call** (`main.rs:562`):
  route it through `zorp::zorp_raw` and drop the second HTTP client.
- **`tokio` in `zorp-mcp` is declared but entirely unused** (`Cargo.toml:9`):
  every transport is blocking `std::process`/`ureq`. Delete it (or use it to
  implement the missing stdio read timeout) and fix the stale `lib.rs:3` doc.
- **`serde_yaml` is unmaintained** (`zorp-eval/Cargo.toml:10`, archived
  upstream): migrate the two call sites to `serde_yaml_ng`.

### 3.6 Workspace hygiene [medium/low]

- **No `[workspace.dependencies]` or `[workspace.package]`** (`Cargo.toml`):
  `serde_json`, `sha2`, `ureq`, `serde`, `toml`, `tempfile`, and `tokio` are
  re-declared across manifests with divergent feature lists. Hoist the shared
  set; hoist `edition`/`license`/`repository` policy.
- **No `rust-version` (MSRV) anywhere**, despite `zorp-eval` using
  `Option::is_none_or` (stable 1.82). Declare a floor and add an MSRV check job.
- **No `[workspace.lints]`**: a shared `unsafe_code`/`unwrap_used` policy is
  nearly free for a minimalism-focused project.
- **Stale comment** at `Cargo.toml:14` lists `zorp-mcp` as a future member; it is
  already a member on line 13.
- **Add `cargo-deny`** (advisories plus a `bans.multiple-versions` check with a
  `skip-tree` for the lance subtree you can't control): the lockfile currently
  carries 71 duplicated package names.

---

## 4. Duplication and structure

Maintainability items with clear, safe boundaries. None is urgent; all reduce
the surface where a fix has to be applied in more than one place.

- **`main.rs` repeats a ~55-line agent-construction block seven times**
  (`zorp-agent/src/main.rs:623,731,841,948,1043,1186,1863`), which is most of
  why the file is 2866 lines. Extract one `build_agent(...)`; the callers differ
  only in the system preamble and what they do with the `Outcome`.
- **The four research subcommands duplicate ~90 lines of setup verbatim**
  (`main.rs` validate/investigate/co_write/deliver). Extract one
  `research_setup(...)`. This is the single largest maintainability win in the
  research code: a bug fixed in one copy today is fixed in one of four.
- **The 7-arm `Outcome` match, `all_fenced_blocks`, and the four capability
  error enums are each duplicated across the capabilities**
  (`*/mod.rs`, `validate/result.rs:51-65` = `investigate/result.rs:41-55`,
  `*/error.rs`). Move them to a shared `crate::research` module. The fence
  scanner also has a real bug in both copies (a single-line fence yields the
  whole text as content), so sharing it fixes the bug once.
- **`Flavor` fields are declared three times plus four near-identical
  resolvers** (`flavor.rs:22-39,48-68,188-210,293-339`). Collapse
  `ConfiguredFlavorDocument` into `Flavor` and the four resolvers into one plus
  wrappers.
- **`sandbox.rs` and `session.rs` do not need splitting** (their size is mostly
  tests), but `sandbox.rs:291-482` (`BoundedCapture` + capture + redaction +
  truncation) is a clean, self-contained unit worth moving to `sandbox/capture.rs`.
- **Delete the committed AI-scratchpad comment** at `agent.rs:415-421` (seven
  lines of first-person deliberation). It documents a real dead entry:
  `policy.rs:99` still lists `invoke_subagent` in the always-`Allow` arm, but no
  tool by that name is registered.
- **`CapsuleRegistry::get` linear-scans a map already keyed for lookup**
  (`capsule.rs:156-158`): use `self.capsules.get(&name.to_lowercase())`.

---

## 5. Tests worth adding

The reviews turned up several tests that currently assert a bug as correct, plus
high-consequence gaps:

- **`model.rs:1030-1051` locks in the Anthropic `max_tokens < budget_tokens`
  bug** (1.8): change it to assert the clamped invariant.
- **`track.rs:392` and `integration.rs:149` assert that a rebuilt prereg passes
  integrity** (1.2): they encode the vulnerable behavior as correct. Add a test
  that tampers `prereg.md`, deletes the DB, rebuilds, and asserts
  `IntegrityMismatch`.
- **No test breaches the kill threshold** (1.1): add one that breaches under
  `AutoApprove` and asserts the track is killed.
- **No test amends the prereg git commit** (1.3): add one asserting
  verification fails.
- **No coverage for the policy bypasses** (1.4-1.6): add table cases for
  `$(...)`, `<(...)`, `rm -rf /*`, `>|/etc/x`, `> ~/.bashrc`.
- **No `zorp-eval` test for an unknown predicate id, a seq-less event, or a
  malformed trace line** (1.10): the three silent-corruption paths.
- **`instrumentation_validation.rs:16-52` is a permanently-green no-op**: three
  `return`s turn every failure into a pass. Mark it `#[ignore]` and run it
  explicitly so a skip reads as a skip.
- **Nothing asserts LanceDB contents**: the whole vector path runs unverified.

---

## Already good (do not "fix" these)

The reviews consistently flagged careful work worth preserving:

- **Core lib**: zero `unwrap`/`expect` outside tests; correct SSE comment
  skipping and empty-200 handling; safe shell-injection escaping in `--init`,
  tested.
- **`sandbox.rs` process supervision**: `setpgid` + `waitid(WNOWAIT)` +
  `POLLHUP` probe on duplicated fds is a genuinely careful solution to
  background-descendant reaping, with tests for both the kill and no-false-timeout
  properties. Streaming redaction correctly handles secrets straddling read
  boundaries, and UTF-8-safe truncation never emits replacement characters.
- **`policy.rs` wrapper unwrapping**: `exec -a`, `command --`, `env FOO=bar`,
  `git -c`, path-qualified executables, and recursive `sh -c` payloads are all
  handled and fail closed on unbalanced quotes. The substitution and redirect
  gaps above are holes in an otherwise serious design.
- **zorp-track**: one long-lived connection (not per-call opens); 100% parameter
  binding, zero string-built SQL; the corruption-vs-lock discrimination in
  `project.rs` is exactly right and covered by a real cross-process lock test;
  `write_prereg` is deliberately non-idempotent; `seq`-based metric ordering
  correctly recognizes that millisecond timestamps give no order; the
  no-`NonInteractive`-escape-hatch checkpoint design is the right call.
- **zorp-eval**: path canonicalization before chdir, manifest-relative suite
  resolution, and post-run restore are each subtle and each locked in by a
  targeted regression test; contract predicate tests cover both directions for
  all six contracts.
- **zorp-mcp**: `McpError` is `#[non_exhaustive]` with structured variants;
  hash inputs are sorted before hashing; the trust model is honestly documented
  as un-enforced rather than implying a guarantee.
- **Feature-flag hygiene in `zorp-agent`**: `otel`/`mcp`/`research`/`clipboard`
  are all off by default with correct `dep:` syntax, so the default build pays
  for none of them. `image`, `chrono`, and the `zorp-mcp`/`zorp-track` tokio
  feature lists are already tight.

---

*Generated from a four-part parallel review of the workspace at `0c0305e`.
File and line references are pinned to that commit and will drift as the code
moves.*
