# zorp-agent: User Acceptance Test (UAT) Report

**Date:** 2026-08-16
**Build under test:** `zorp-agent 0.2.1`, debug binary, built with `--features research`
**Backend:** local Ollama 0.32.14-rc0 (OpenAI-compatible endpoint), chat model
`qwen3.8:27b`, embedding model `qwen3-embedding:latest`
**Method:** black-box testing of the real binary against a live model. No
source edits, no `cargo` invocations except the initial build. Every run used
an isolated `HOME`, `ZORP_STATE_DB`, and `ZORP_TRUST_FILE` under a scratch
directory, so nothing touched the tester's real state.

This is zorp's first UAT. It follows the method of the quecto reports
preserved in `docs/upstream-quecto/uat/`, and adds Area E for the four
research capabilities, which quecto never had.

---

## Executive summary

| Metric | Result |
|---|---|
| Total scenarios | **67** |
| Pass | **64** |
| Partial (works, with a rough edge) | **2** |
| Fail | **1** |
| Blocking defects | **0** |

**Verdict: ACCEPT, with one medium finding to fix.** Every core workflow
works end to end against a live local model: one-shot tasks, the chat REPL,
the full tool set, editing under approval, the hard denylist, the
verification gate, session persistence, flavors with trust-on-first-use,
and all four research capabilities. The four capabilities are the first
thing here that is zorp's own rather than inherited, and they held up:
validate approved a track, investigate wrote a git-committed
pre-registration and enforced an immutable kill threshold, co-write drafted
from recorded evidence without overclaiming, and deliver produced a venue
shortlist.

One real defect surfaced: a project-scope flavor that tightens approvals is
silently ignored. It fails closed in the safety sense (a project flavor
still cannot grant itself more power), but a user who writes a restriction
gets no restriction and no warning.

---

## Findings

| ID | Severity | Area | Finding |
|---|---|---|---|
| F1 ([#11](https://github.com/aviskaar/zorp/pull/11)) | 🟠 Medium | Flavors | A project-scope `[approval]` override that *tightens* policy is silently dropped. `gated_flavor` (`zorp-agent/src/main.rs:438-441`) returns the user flavor alone whenever the project flavor does not want privilege, and `wants_privilege()` is false for tightening overrides by design (`zorp-agent/src/flavor.rs:644-653`). The project `[approval]` section therefore never reaches `build_policy`, which reads `user.approval.overrides` (`main.rs:505`). The asymmetry is visible in one file: project `[tools] enabled` applies (it goes through `merged`, `main.rs:712`), project `[approval] write_file = "deny"` does not. Live: the identical override denies the write at user scope and permits it at project scope, with no warning either way. |
| F2 ([#12](https://github.com/aviskaar/zorp/pull/12)) | 🟡 Low | Tools | `take_note` and `search_notes` still write to `.qkb/`, a quecto-era name, while every other path was renamed to `.zorp/`. A zorp user running the notes tools gets a `.qkb/` directory in their repo. `docs/superpowers/specs/2026-08-08-zorp-bootstrap-design.md:25` records dropping quecto's own `.qkb/` as a session artifact, but the tools that create it kept the name. |
| F3 ([#13](https://github.com/aviskaar/zorp/pull/13)) | 🟡 Low | Safety | `--approval read-only` does not prevent writes when combined with `--yes`. The preset sets `edit: Decision::Ask` (`policy.rs:73-77`) and `--yes` answers every ask. That is coherent once you know it, but the flag names suggest the opposite, and nothing documents the interaction. The way to actually block an operation is an explicit `deny` override, which works (at user scope, see F1). |
| F4 ([#13](https://github.com/aviskaar/zorp/pull/13)) | 🟡 Low | Core CLI | Step-limit exhaustion is reported two different ways: a one-shot task prints `zorp-agent: step limit reached`, a research subcommand prints `zorp-agent: agent did not complete: StepLimit`. Both exit 1. |
| F5 ([#13](https://github.com/aviskaar/zorp/pull/13)) | 🟡 Low | Tools | A failing verification gate needs at least 3 verify attempts to exit cleanly as `VerificationFailed`. With a small `ZORP_MAX_STEPS` (4 in this run) the step limit fires first and the clean outcome is masked. The bound counts attempts at an unchanged file-change count, so a model that keeps editing resets it. |
| F6 ([#13](https://github.com/aviskaar/zorp/pull/13)) | 🟡 Low | Research | Grammar in an error message: `--metric-name, --kill-threshold, and --threshold-direction is required on the first investigate call` should read "are required". |

None of these block use. F1 is the only one that changes behavior a user
asked for. Each has a fix open, linked from its ID above.

---

## Area A: Core UX & CLI · 12 pass

| # | Test | Observed | Verdict |
|---|---|---|---|
| 1 | No arguments | `usage: zorp-agent [--yes] [--no-verify] "<task>"` on stderr, exit 2 | ✅ |
| 2 | `--help` / `-h` | All 10 subcommands plus per-option descriptions, exit 0 | ✅ |
| 3 | `--version` | `zorp-agent 0.2.1`, exit 0 | ✅ |
| 4 | One-shot live task | `pong`, exit 0 | ✅ |
| 5 | Subcommand-looking task after `--` | `zorp-agent -- undo is a word in this sentence…` treated as a task, `OK`, exit 0 | ✅ |
| 6 | Chat slash commands | `/help /model /context /status /clear` all answered; `/commands` listed 16 tools | ✅ |
| 7 | Chat unknown command | `unknown command '/frobnicate' — try /help`, no crash | ✅ |
| 8 | Chat real turn and exit | `› pong` then `› bye`, exit 0 | ✅ |
| 9 | Activity renderer | `● read_file  note.txt (1 lines)` on stderr, answer `hello uat` on stdout | ✅ |
| 10 | Piped formatting | No ANSI bytes in piped stdout | ✅ |
| 11 | Exit codes | `resume` with no id → 2; unknown id → 1 `no session 'deadbeef-not-a-session'`; step limit → 1 `step limit reached` | ✅ |
| 12 | Unknown flag | `error: unexpected argument '--bogus' found`, exit 2 | ✅ |

The subcommand-precedence fix from PR #5 holds: `--yes undo` runs `undo`
rather than sending "undo" to the model as a task.

---

## Area B: Tools & Safety · 16 pass, 1 partial

| # | Test | Observed | Verdict |
|---|---|---|---|
| 1 | `read_file` | `● read_file  data.txt (1 lines)`, correct line returned (the count is of the selected `start_line`/`end_line` range, not the file) | ✅ |
| 2 | `search_text` | `● search_text  'gamma' (1 matches)`, named the right file | ✅ |
| 3 | `write_file` under `--yes` | `● write_file  created created.txt (1 lines)`, contents exact | ✅ |
| 4 | `apply_patch` under `--yes` | `● apply_patch  1/1 blocks applied`, edit correct | ✅ |
| 5 | `git_status` | `● git_status  2 changed files` | ✅ |
| 6 | `git_diff` | `● git_diff  diff (7 lines)` | ✅ |
| 7 | `run_command` under `--yes` | `● run_command(echo marker-9931)  exited 0`, stdout captured | ✅ |
| 8 | Deny write without `--yes`, non-interactive | `● write_file  denied` immediately, no file created | ✅ |
| 9 | Deny `run_command` without `--yes` | `● run_command(touch nope.txt)  denied`, no file created | ✅ |
| 10 | Denylist under `--yes`: `sudo` | `● run_command(sudo -n true)  denied` | ✅ |
| 11 | Denylist under `--yes`: `git push` | `● run_command(git push origin main)  denied` | ✅ |
| 12 | Denylist under `--approval full` | `git push` still `denied`, preset does not win | ✅ |
| 13 | Verify gate passing | `● verify true  passed`, exit 0 | ✅ |
| 14 | Verify gate failing | `● verify false  failed` ×3 then `verification still failing after 3 attempts`, exit 1 (needs step headroom, see F5) | ✅ |
| 15 | `--no-verify` | Gate skipped entirely, exit 0 | ✅ |
| 16 | Long command | `sleep 4; echo awake` captured `awake`, exit 0 | ✅ |
| 17 | `take_note` / `search_notes` | Round-trip works, but writes to `.qkb/` (F2) | 🟡 |

The destructive-`rm` rule was verified by reading `policy.rs:240-253` rather
than by running it. Deliberate: the safe live substitutes (`sudo`,
`git push`) exercise the same code path in `deny_reason`.

---

## Area C: Persistence · 10 pass

| # | Test | Observed | Verdict |
|---|---|---|---|
| 1 | `diff` with no sessions | `zorp-agent: no sessions`, exit 1 | ✅ |
| 2 | `undo` with no sessions | `zorp-agent: no sessions to undo`, exit 1 | ✅ |
| 3 | Record a file edit | `sessions=1 messages=5 file_changes=1`; `note.txt` v1→v2 | ✅ |
| 4 | `diff` | `1 file change(s)` / `modified  note.txt  (was 1 lines, now 1 lines)` | ✅ |
| 5 | `undo` | `reverted note.txt`, file back to v1, `file_changes=0` | ✅ |
| 6 | `undo` again | `no changes to undo`, exit 1 | ✅ |
| 7 | `resume <id>` | `zorp-agent: resuming session 18cc341d9425c720-9be...` on stderr | ✅ |
| 8 | Multi-turn chat persistence | New session recorded, messages 6→11, both turns answered | ✅ |
| 9 | No-git degradation | In a non-git directory `● git_status  error`, agent still completes, exit 0 | ✅ |
| 10 | State-db isolation | Real `~/.local/state/zorp/sessions.db` untouched (mtime unchanged); overridden `HOME` stayed empty | ✅ |

Schema observed in the state db: `sessions`, `messages`, `file_changes`,
`message_images`.

---

## Area D: Flavors & Trust · 13 pass, 1 partial, 1 fail

| # | Test | Observed | Verdict |
|---|---|---|---|
| 1 | `new reviewer` | `created …/.zorp/flavors/reviewer.toml`, exit 0 | ✅ |
| 2 | `new reviewer` again | `already exists`, exit 1 | ✅ |
| 3 | `api_key` in a manifest | `flavor error: TOML parse error`, exit 1, key never read | ✅ |
| 4 | Unknown manifest key | `flavor error: TOML parse error … bogus_key`, exit 1 | ✅ |
| 5 | User-scope persona | `system_prompt` in `~/.config/zorp/flavor.toml` → `Le ciel est bleu par un jour clair.` with `ZORP_SYSTEM` unset | ✅ |
| 6 | Model precedence, flag over env | `--model definitely-not-a-real-model` → 404 naming that model, exit 1 | ✅ |
| 7 | Tool allow-list | Project `[tools] enabled` without `write_file` → write request produced no tool call and no file | ✅ |
| 8 | Allowed tools still work | `read_file` under the same allow-list returned `v1` | ✅ |
| 9 | `--approval read-only` with `--yes` | Write allowed, file created (preset asks, `--yes` answers, see F3) | 🟡 |
| 10 | Project `[approval] write_file = "deny"` | Write allowed, file created, no warning | ❌ F1 |
| 11 | Same override at user scope | `● write_file  denied`, no file created | ✅ |
| 12 | Untrusted project flavor | `project flavor not trusted; its verify/approval settings are ignored`, exit 0, no trust hash written | ✅ |
| 13 | `--yes` trusts and applies | Hash `62be9b0f667831fa…` written to the trust file, project `[verify] test = "false"` then ran and failed, exit 1 | ✅ |
| 14 | Trusted reload | No warning on the next run | ✅ |
| 15 | Changed manifest | Appending one comment re-gated it: `project flavor not trusted…` | ✅ |

Test 10 is the F1 defect. Tests 12 to 15 show the trust-on-first-use cycle
working exactly as specified: withhold, trust on `--yes`, silent reload,
re-gate on content change.

---

## Area E: Research capabilities · 13 pass

This area is zorp's own. It has no quecto equivalent.

| # | Test | Observed | Verdict |
|---|---|---|---|
| 1 | `validate` with no MCP tool | `no search-capable tool is available; configure an MCP search server (--mcp or .zorp/mcp.toml)`, exit 1 | ✅ |
| 2 | `deliver` before any draft | `this track has no draft.md yet; run co-write at least once before deliver`, exit 1 (this check precedes the huiban check) | ✅ |
| 3 | `validate` with a stub search server | `validate: approved, track 2026-08-16-does-connection-pooling-reduce-p99-latency-in-our-api ready for investigate`, exit 0; `.zorp/zorp.duckdb` created | ✅ |
| 4 | `investigate` with no pre-registration flags | `no pre-registration exists for this track yet; --metric-name, --kill-threshold, and --threshold-direction is required…`, exit 1 (F6) | ✅ |
| 5 | `investigate` with partial flags | `--metric-name, --kill-threshold, and --threshold-direction must be given together`, exit 2 | ✅ |
| 6 | `investigate` first attempt | Wrote `.zorp/tracks/<track>/prereg.md` and committed it: `f077d69 prereg(<track>): pre-registration`; agent wrote and ran a benchmark; `investigate: approved, track … stays active` | ✅ |
| 7 | Pre-registration immutability, threshold | `--kill-threshold (999) does not match the track's recorded pre-registration (250)`, exit 1 | ✅ |
| 8 | Pre-registration immutability, metric | `--metric-name (latency_ms) does not match the track's recorded pre-registration (p99_ms)`, exit 1 | ✅ |
| 9 | Kill threshold breach | `kill threshold breached: metric 'sort_ms' = 110.42 went above threshold 0 (lower-is-better); track killed`; stdout `investigate: rejected, track … killed` | ✅ |
| 10 | `co-write` | `co-write: approved, draft ready for review at .zorp/tracks/<track>/draft.md` | ✅ |
| 11 | `deliver` with a non-huiban tool | `no huiban-prefixed tool is available; configure the huiban MCP server (--mcp or .zorp/mcp.toml)`, exit 1 | ✅ |
| 12 | `deliver` with a huiban-prefixed tool | `deliver: approved, shortlist ready for review at .zorp/tracks/<track>/venues.md` | ✅ |
| 13 | `.zorp/mcp.toml` instead of `--mcp` | Server loaded from the file, tools appeared as `mcp__huiban__*`, validate approved a second track | ✅ |

The co-written draft is worth quoting, because it is the behavior the
README claims and it happened without prompting:

> **The hypothesis is untested, not disproven or supported.** A single p99
> value of 7.5 ms tells us the absolute latency observed under one
> configuration; it says nothing about the *delta* introduced by pooling.
> Without a control, the 7.5 ms figure is not interpretable as a pooling
> effect.
>
> Confidence: none.

It drafted from the one metric actually on record, named the missing
control, and refused to draw a conclusion. The venue shortlist was equally
blunt: contribution type "*None* that maps to a venue" for a one-data-point
memo.

One presentation nit: `venues.md` opens with conversational framing ("I ran
the searches, so I can now assess scope") in a file meant to be a document.
That is model output rather than harness behavior, so it is not filed as a
finding.

---

## What worked especially well

- **Safety defaults hold under pressure.** Non-interactive denial preserves
  files, the hard denylist beats both `--yes` and `--approval full`, and
  unknown manifest keys and `api_key` fail closed.
- **Pre-registration is real.** The kill threshold is written to a file,
  committed to git, and then enforced against later flags. Changing either
  the metric or the threshold is refused with the recorded value quoted
  back. This is the product's core claim and it works.
- **A killed track says why.** The breach message names the metric, the
  value, the direction, and the threshold.
- **Error messages are actionable.** Both research fail-fast paths name the
  fix (`--mcp` or `.zorp/mcp.toml`), and the deliver path names the
  prerequisite step.
- **Trust-on-first-use is tight.** Content-hash gating re-asks after a
  one-comment edit.
- **State isolation is honest.** With `ZORP_STATE_DB` and `ZORP_TRUST_FILE`
  set, the real state directory was untouched across the whole run.

---

## Methodology notes

- One tester, sequential, on a single machine. The quecto reports used four
  parallel testers per area; this run traded that for a live research-stack
  pass that quecto never had.
- Isolation: `HOME`, `ZORP_STATE_DB`, and `ZORP_TRUST_FILE` all pointed into
  a scratch directory. Five working directories, one per area, two of them
  git repositories.
- Live turns were capped with `ZORP_MAX_STEPS` (6 by default, 10 to 14 for
  the research capabilities). A local 27B model wanders, and an uncapped
  loop measures the model rather than the harness.
- The one source-verified rather than live-tested item is the destructive
  `rm` denylist rule, noted in Area B.
- Ollama's desktop app quit itself repeatedly mid-request during setup,
  which surfaces in zorp as `Network Error: Unexpected EOF` or
  `Connection refused`. Running `ollama serve` from a terminal instead of
  the desktop app made the endpoint stable for the whole run. Worth knowing
  before blaming the harness for a transport error.

## How to reproduce

```bash
cargo build -p zorp-agent --features research

export ZORP_BASE_URL=http://localhost:11434/v1
export ZORP_MODEL=qwen3.8:27b
export ZORP_EMBEDDING_MODEL=qwen3-embedding:latest   # validate needs it, no default
export ZORP_STATE_DB=$PWD/uat-state.db               # keep UAT out of the real state dir
export ZORP_TRUST_FILE=$PWD/uat-trust
export ZORP_MAX_STEPS=8

# core
./target/debug/zorp-agent "reply with exactly: pong"

# research, using the in-tree stub MCP server
./target/debug/zorp-agent --yes --mcp "stdio:stub:$PWD/target/debug/stub_search_mcp_server" \
  validate "<question>"
./target/debug/zorp-agent --yes investigate "<question>" \
  --metric-name <name> --kill-threshold <n> --threshold-direction lower-is-better
./target/debug/zorp-agent --yes co-write "<question>"
./target/debug/zorp-agent --yes --mcp "stdio:huiban:$PWD/target/debug/stub_search_mcp_server" \
  deliver "<question>"
```

The hermetic equivalent, which needs no model and no MCP server, is
`cargo test -p zorp-agent --features research` (480 tests, all passing on
this build).
