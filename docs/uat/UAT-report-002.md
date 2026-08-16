# zorp-agent: UAT 002, retest on merged main

**Date:** 2026-08-16
**Build under test:** `zorp-agent 0.2.1` at `f3ea7b1`, debug binary, built
with `--features research`
**Backend:** local Ollama 0.32.14-rc0 (OpenAI-compatible endpoint), chat model
`qwen3.8:27b`, embedding model `qwen3-embedding:latest`
**Previous run:** [`UAT-report.md`](UAT-report.md) (67 scenarios, 1 fail,
2 partial, six findings)

A full re-run of all 67 scenarios against main after the four fixes
(#10, #11, #12, #13) landed. Purpose: confirm the six findings are closed
and that nothing regressed. Fresh sandbox, nothing carried over from the
first run.

---

## Executive summary

| Metric | Run 001 | Run 002 |
|---|---|---|
| Total scenarios | 67 | **67** |
| Pass | 64 | **67** |
| Partial | 2 | **0** |
| Fail | 1 | **0** |
| Blocking defects | 0 | **0** |

**Verdict: ACCEPT.** All 67 scenarios pass. Every finding from run 001 is
closed and verified against observed output, not just against a test
suite.

The retest found **three new low-severity issues** (G1, G2, G3). None
block use and none are regressions. Two of them are in areas run 001
reached but did not push on, and the third is a consequence of testing
the approval policy harder than run 001 did.

---

## Verification of run 001's findings

| ID | Fix | Retest evidence | Closed |
|---|---|---|---|
| F1 🟠 | [#11](https://github.com/aviskaar/zorp/pull/11) | Project `.zorp/flavor.toml` with `[approval] write_file = "deny"` under `--yes` now prints `● write_file  denied` twice. Run 001 created the file silently. | ✅ |
| F2 🟡 | [#12](https://github.com/aviskaar/zorp/pull/12) | `● take_note  appended to .zorp/notes/retest-marker-8802.md`; `.qkb` is not created (`ls: .qkb: No such file or directory`). | ✅ |
| F3 🟡 | [#13](https://github.com/aviskaar/zorp/pull/13) | `--help` now states that `--yes` answers the asks a preset produces, and that presets set what is asked about rather than what is refused. | ✅ |
| F4 🟡 | [#13](https://github.com/aviskaar/zorp/pull/13) | One-shot: `zorp-agent: step limit reached`. Research path: `zorp-agent: agent did not complete: step limit reached`. Same phrase, both paths. | ✅ |
| F5 🟡 | [#13](https://github.com/aviskaar/zorp/pull/13) | `--max-steps` documents the headroom a failing gate needs. With 10 steps the gate reports itself: `verification still failing after 3 attempts`. | ✅ |
| F6 🟡 | [#13](https://github.com/aviskaar/zorp/pull/13) | `no pre-registration exists for this track yet; pass --metric-name, --kill-threshold, and --threshold-direction on the first investigate call`. | ✅ |

Run 001's two partials are also resolved: the notes path (F2) is fixed
outright, and the `--approval` / `--yes` interaction (F3) is now
documented at the point of use, so it stops reading as a surprise.

---

## New findings

| ID | Severity | Area | Finding |
|---|---|---|---|
| G1 | 🟡 Low | Tools | `search_notes` matches note bodies only, never titles or filenames. A note taken as title `retest-marker-8802` with body `is the build id` is not findable by `retest-marker` or by `8802`: both return `0 matches`, while `build id` returns `2 matches`. Since `take_note` puts the title in the filename and only the body in the file, the most natural way to look a note up is the one way that cannot work. |
| G2 | 🟡 Low | Safety | `write_file = "deny"` alone is not a write barrier. With `run_command` still approved under `--yes`, the model was denied twice and then wrote the file anyway: `● run_command(printf 'hi\n' > project-deny2.txt && cat project-deny2.txt)  exited 0`. Redirects inside the repo are allowed by design (`policy.rs` only denies redirects that escape the repo root), so this is expectation rather than a hole, but a user who denies one write tool reasonably expects no writes. |
| G3 | 🟡 Low | Research CLI | A track-store open failure exits **2**, the code this same binary uses for CLI usage errors (no arguments, unknown flag). Observed when a second research command hit the DuckDB lock held by a concurrent run: `zorp-track db error: IO Error: Could not set lock on file ... Conflicting lock is held ... (PID 81916)`, exit 2. Other runtime failures exit 1 (`no session '<id>'`, model transport errors). Four call sites: `zorp-agent/src/main.rs:837, 949, 1077, 1177`. |

G2 has a working answer, verified: denying the write tools together
closes every route.

```toml
# .zorp/flavor.toml
[approval]
write_file = "deny"
run_command = "deny"
apply_patch = "deny"
```

```
● write_file  denied
● run_command(cp project-deny2.txt paired.txt && cat paired.txt)  denied
● run_command(echo hi > paired.txt)  denied
● spawn_subagent  denied
zorp-agent: stopped: several actions were denied.
```

Worth noting that `spawn_subagent` is covered by the same policy, so the
obvious escape hatch is not one.

---

## Area results

| Area | Run 001 | Run 002 |
|---|---|---|
| A: Core UX & CLI | 12 pass | **12 pass** |
| B: Tools & Safety | 16 pass, 1 partial | **17 pass** |
| C: Persistence | 10 pass | **10 pass** |
| D: Flavors & Trust | 13 pass, 1 partial, 1 fail | **15 pass** |
| E: Research capabilities | 13 pass | **13 pass** |

Spot evidence from each area, beyond the finding verification above.

**A.** No arguments still exits 2 with usage on stderr. `--version` is
`zorp-agent 0.2.1`. The chat REPL answered every slash command, hinted on
`/frobnicate`, answered a real turn, and exited cleanly. Piped stdout
carries no ANSI.

**B.** Every tool round-tripped: `● read_file  data.txt (3 lines)`,
`● search_text  'gamma' (1 matches)`, `● apply_patch  1/1 blocks applied`,
`● git_diff  diff (7 lines)`, `● run_command(echo marker-7712)  exited 0`.
Denials hold without `--yes`, and the hard denylist still beats both
`--yes` and `--approval full` on `git push`.

**C.** Empty-state messages, record, `diff`, `undo`, `undo` again,
`resume`, and multi-turn chat persistence (messages 6 → 11, sessions
1 → 2) all behaved as in run 001. The real state database at
`~/.local/state/zorp/sessions.db` is still dated Aug 14, untouched across
both runs.

**D.** Scaffold, overwrite refusal, `api_key` rejection, unknown-key
rejection, user persona, model precedence, and the full trust-on-first-use
cycle (withhold, trust on `--yes`, silent reload, re-gate after a
one-comment edit) all pass. The trust hash recorded this run was
`cd1b0b6e8c7ba0ec…`.

**E.** All four capabilities ran end to end again on a fresh project:
validate approved a track, investigate wrote and git-committed
`prereg(<track>): pre-registration`, both immutability checks refused a
changed metric and a changed threshold, a breach killed a track
(`metric 'hash_ms' = 0.314 went above threshold 0 (lower-is-better)`),
co-write produced a draft, and deliver produced a venue shortlist.
`.zorp/mcp.toml` works in place of `--mcp`.

---

## Methodology notes

- Same method as run 001: one tester, sequential, isolated `HOME`,
  `ZORP_STATE_DB`, and `ZORP_TRUST_FILE` in a scratch sandbox. Five
  working directories, two of them git repositories.
- Run 002 is not an independent replication. It was run by the same
  tester who wrote the fixes, which is the weakest position from which to
  confirm them. That is why each verification above cites observed output
  rather than a passing test.
- G3 was observed once, when the tester's own harness ran two research
  commands concurrently in one project. The exit code and the four call
  sites were then confirmed by reading the source. It was not reproduced
  a second time on purpose; a deliberate reproduction attempt was
  abandoned after it cost more model time than the finding is worth.
- Two scenarios needed a second attempt because the local 27B model
  answered from its own reasoning instead of calling the tool under test
  (`git_status` in Area B, both runs). That is a model behavior, not a
  harness one, and the retry with an explicit instruction succeeded both
  times.
- Run time is dominated by the model. `deliver` took long enough to
  exceed a ten minute command budget once and had to be re-run detached.

## Suggested next steps

1. G1 and G3 are small and self-contained. G2 is a documentation change
   in the same family as F3.
2. Run 003 should be somebody, or something, other than the author of the
   fixes. Run 001 used four parallel testers per area for the inherited
   harness; that structure would catch what a single sequential pass
   misses.
