# zorp-agent: UAT plan for native web search (Tavily)

**Date:** 2026-08-16
**Status:** plan. Nothing in this document has been run. Every verdict
cell is blank on purpose.
**Feature under test:** native `web_search`, per
[`docs/superpowers/specs/2026-08-16-tavily-web-search-design.md`](../superpowers/specs/2026-08-16-tavily-web-search-design.md)
**Build to test:** `zorp-agent 0.2.1`, debug binary, built with
`--features research,search`
**Model backend:** local Ollama, chat model `qwen3.8:27b`, embedding model
`qwen3-embedding:latest`, same as runs 001 and 002
**Search backend:** Tavily, live API, key in `ZORP_TAVILY_API_KEY`
**Previous runs:** [`UAT-report.md`](UAT-report.md) (001),
[`UAT-report-002.md`](UAT-report-002.md) (002)

Why this is a plan and not a report: the Tavily API key arrives later. The
hermetic tests in `zorp-search/tests/` and `zorp-agent` cover the wire
format with a local stub and need no key, and they are a precondition for
running any of this. What they cannot cover is the real endpoint, real
auth, real failure modes, and the thing this feature exists for: `validate`
running with no MCP server configured at all. That is what the scenarios
below are for.

---

## Executive summary

| Metric | Planned |
|---|---|
| Total scenarios | **28** |
| Areas | **6** (config, tool, approval, validate, failure, secrets) |
| Scenarios that reach the network | **9** (A6, B1, B2, B4, C2, D1, D5, E1, E5) |
| Live search requests spent | **~15**, budget 40 with retries |
| Scenarios that must pass for ACCEPT | Areas D, E, F in full |

Three claims are on trial. Everything else is supporting evidence.

1. **`validate` works with no MCP server.** Runs 001 and 002 both had to
   stand up a stub MCP server to test `validate` at all. Area D fails the
   whole feature if that is still true.
2. **A failed search never looks like an empty result set.** Design D7.
   Area E is built around the difference, because conflating the two puts a
   wrong novelty score into an evidence record.
3. **The key never leaves the process.** Design D6. Area F greps every
   output stream, the trace, the state database, and the `.zorp/` tree for
   the literal key.

---

## Preconditions

- `cargo test -p zorp-agent --features research,search` and
  `cargo test -p zorp-search` pass. This UAT is an acceptance pass on top
  of the hermetic tests, not a substitute for them.
- A Tavily key with quota, stored in a file with mode 600. Read it into the
  environment from that file so it never enters shell history, and do not
  run any of this under `set -x`.
- The model backend from runs 001 and 002, or any OpenAI-compatible
  endpoint whose model calls a tool when told to by name. Run 002's
  methodology notes that the local 27B model sometimes answers from its own
  reasoning instead of calling the tool. Every task string below names
  `web_search` explicitly for that reason. A run where the model simply
  never calls the tool is a retry, not a fail.
- `python3` for the fake endpoint in Area E. `duckdb` CLI for D2, optional.
- A scratch sandbox, as in both previous runs.

```bash
cargo build -p zorp-agent --features research,search

# sandbox: nothing here touches the tester's real state
export HOME=$PWD/sandbox-home
export ZORP_STATE_DB=$PWD/uat-state.db
export ZORP_TRUST_FILE=$PWD/uat-trust
export ZORP_TRACE_FILE=$PWD/trace.jsonl

# model backend
export ZORP_BASE_URL=http://localhost:11434/v1
export ZORP_MODEL=qwen3.8:27b
export ZORP_EMBEDDING_MODEL=qwen3-embedding:latest
export ZORP_MAX_STEPS=8

# the key under test
export ZORP_TAVILY_API_KEY=$(cat ~/.tavily-uat-key)

# capture helper used by every scenario
za() { ./target/debug/zorp-agent "$@" >out.txt 2>err.txt; echo "exit=$?"; }
```

Truncate the trace before each scenario (`: > trace.jsonl`) so the
assertions below read one run at a time. Keep `out.txt`, `err.txt`, and
`trace.jsonl` per scenario if you want to grep them again in Area F.

Three facts the assertions rely on. The first two are harness behavior on
`d11719d`. The third is the provider's own error text, read off
`zorp-search/src/lib.rs` while it was still being written, so confirm it
against the built binary before running.

- A tool that returns an error renders as `● <name>  error` on stderr and
  reaches the model as `error: <message>`
  (`zorp-agent/src/tools/mod.rs:280-283`, `zorp-agent/src/agent.rs:793`).
- The trace is JSON lines. A tool call writes
  `{"event_type":"tool.call","tool_name":"web_search",...}` and a result
  writes `{"event_type":"tool.result","tool_name":"web_search","success":<bool>,...}`
  (`zorp-agent/src/agent.rs:85-99`). Tool arguments and tool content are
  not written to the trace.
- `SearchError` has four variants and each one names the provider:

  | Condition | Message |
  |---|---|
  | No key | `tavily search: no API key; set ZORP_TAVILY_API_KEY` |
  | Non-2xx | `tavily search: HTTP status <code>: <body>` |
  | Transport | `tavily search: request failed: <message>` |
  | Bad body | `tavily search: malformed response: <message>` |

  Wrapped as a tool result, each arrives at the model prefixed with
  `error: `.

---

## Area A: Configuration and the key · 6 scenarios

Design D3 puts search behind a feature. Design D6 puts the key in the
environment only, and requires a missing key to name the variable.

| # | Test (command) | Expected | Observed | Verdict |
|---|---|---|---|---|
| A1 | Feature off, tool absent. `cargo build -p zorp-agent --features research` then `printf '/commands\n/exit\n' \| ./target/debug/zorp-agent chat 2>&1 \| grep -cx web_search` | `0`. A research-only build and a default build are unchanged by this work. No `web_search` anywhere. Zero API calls. | | ⬜ |
| A2 | Feature on, tool present. Rebuild with `--features research,search`, same pipe. | `1`. `web_search` appears in the tool list `/commands` prints. Zero API calls. | | ⬜ |
| A3 | Key missing, on a task that would search. `env -u ZORP_TAVILY_API_KEY ./target/debug/zorp-agent --yes "Use the web_search tool to find the release date of DuckDB 1.0.0." >out.txt 2>err.txt; echo "exit=$?"` | `err.txt` contains `tavily search: no API key; set ZORP_TAVILY_API_KEY`, or the same message wrapped as a tool result. The variable name must appear verbatim, which is what D6 requires. No request reaches Tavily. Record the exit code and whether the message arrives before or after the first model turn; see assumption i. | | ⬜ |
| A4 | Key missing, on a task that never searches. `env -u ZORP_TAVILY_API_KEY ./target/debug/zorp-agent "reply with exactly: pong"` | One of two acceptable outcomes, and which one is the point of the scenario. Either stdout is `pong` and exit 0, with the search tool simply absent, or the binary refuses at startup naming the variable. A refusal means a `search` build cannot do anything without a Tavily key, which is a medium finding worth filing. | | ⬜ |
| A5 | Key set but empty. `ZORP_TAVILY_API_KEY= ./target/debug/zorp-agent --yes "Use the web_search tool to find the release date of DuckDB 1.0.0."` | Identical to A3. Repeat once with a whitespace-only key (`ZORP_TAVILY_API_KEY=" "`), which the provider also treats as missing. A silent attempt to search with an empty key is a fail. | | ⬜ |
| A6 | Key present but rejected by the API. `( export ZORP_TAVILY_API_KEY=zorp-uat-invalid-key; za --yes "Use the web_search tool to find the release date of DuckDB 1.0.0." )` | `● web_search  error` on stderr, and a tool result reading `error: tavily search: HTTP status <code>: <body>` with the real rejection code. Trace line has `"success":false`. Not a zero-result summary. The run itself may still exit 0, because a failed tool call is not a failed run. One rejected request. | | ⬜ |

---

## Area B: The tool in isolation · 4 scenarios

A plain agent task, no research subcommand. The question is whether the
tool runs, renders, and hands its results back to the model.

| # | Test (command) | Expected | Observed | Verdict |
|---|---|---|---|---|
| B1 | A search runs and renders. `za --yes "Use the web_search tool to find the release date of DuckDB 1.0.0, then answer in one sentence with the source URL."` | At least one stderr line starting `● web_search`, with a summary that is neither `denied` nor `error`. `exit=0`. Trace has a `tool.call` and a `tool.result` with `"tool_name":"web_search"` and `"success":true`. One request. | | ⬜ |
| B2 | Results reach the model. Same run as B1: `grep -o 'https\?://[^ )]*' out.txt`, then re-run the query by hand with the same request the provider sends: `curl -s https://api.tavily.com/search -H "Authorization: Bearer $ZORP_TAVILY_API_KEY" -H 'Content-Type: application/json' -d '{"query":"DuckDB 1.0.0 release date","search_depth":"basic"}'` | At least one URL in the model's answer also appears in the provider's own result set for that query. If none does, the model invented the citation, which is a model finding rather than a harness one, but record it: `validate` depends on this path being real. Check the curl against `zorp-search/src/tavily.rs` first, in case the request shape moved. One request. | | ⬜ |
| B3 | Streams stay separated. After B1: `grep -c '●' out.txt` and `grep -c '● web_search' err.txt` | `0` on stdout and `1` or more on stderr. Piped stdout carries no ANSI bytes either. Same contract as run 001, Area A tests 9 and 10. Zero extra API calls. | | ⬜ |
| B4 | Two searches in one run. `za --yes --max-steps 12 "Use the web_search tool twice: once for DuckDB 1.0.0 release date, once for DuckDB ADBC support. Then summarise both in two sentences."` | Two `● web_search` lines, two `tool.call` events with `"tool_name":"web_search"`, both results `"success":true`, and an answer that touches both topics. Two requests. | | ⬜ |

---

## Area C: Approval · 4 scenarios

Design D4 puts `web_search` at `Decision::Ask`, the same place MCP tools
sit, because a search sends the user's question to a third party. All four
scenarios here except C2 spend zero API calls, since a denied tool never
runs.

| # | Test (command) | Expected | Observed | Verdict |
|---|---|---|---|---|
| C1 | Denied without `--yes`, non-interactive. `za "Use the web_search tool to find the release date of DuckDB 1.0.0."` with stdin not a TTY | `● web_search  denied` on stderr, immediately. The model sees `denied: approval required`. Three consecutive denials end the run: `zorp-agent: stopped: several actions were denied. Re-run with --yes to auto-approve edits and commands, or set an approval preset in a flavor.` and exit 1. No request reaches Tavily. | | ⬜ |
| C2 | Allowed with `--yes`. The B1 run, recorded here as the paired positive. | `● web_search` with a non-error summary, exit 0. Shared with B1, no extra cost. | | ⬜ |
| C3 | A preset does not grant it. `za --approval full "Use the web_search tool to find the release date of DuckDB 1.0.0."` | Still `● web_search  denied`. Presets set what is asked about, not what runs without an answer, which `--help` states since PR #13. D4 puts `web_search` at Ask under every preset. Zero API calls. | | ⬜ |
| C4 | Withheld by a project flavor. Write `.zorp/flavor.toml` with `[tools]` / `enabled = ["read_file", "search_text", "list_files"]`, then `za --yes "Use the web_search tool to find the release date of DuckDB 1.0.0."` | No `● web_search` line at all: the tool is never registered, so there is nothing to approve. `/commands` under the same flavor does not list it. The model either says it cannot search or reaches for `search_text`. Zero API calls. Project-scope `[tools] enabled` applies without trust, verified in run 001 Area D test 7. | | ⬜ |

---

## Area D: Integration with `validate` · 5 scenarios

This is the point of the feature. Runs 001 and 002 could only test
`validate` against a stub MCP server, because standing up a real one is a
separate project. D1 is the scenario that was impossible in both.

Each scenario runs in its own empty directory so track ids and the DuckDB
store do not collide. Do not run two of them at once: concurrent research
commands hit the track-store lock, which is finding G3 from run 002 and has
nothing to do with search.

| # | Test (command) | Expected | Observed | Verdict |
|---|---|---|---|---|
| D1 | **`validate` with no MCP server anywhere.** In a fresh empty directory with no `.zorp/mcp.toml` and no `--mcp` flag: `za --yes --max-steps 12 validate "does connection pooling reduce p99 latency in our API"` | At least one `● web_search` line on stderr with a non-error summary. stdout is `validate: approved, track <id> ready for investigate` or `validate: rejected, track <id> killed`. `exit=0`. The string `no search-capable tool is available` must **not** appear. `.zorp/zorp.duckdb` is created. One to four requests, model dependent. | | ⬜ |
| D2 | The evidence landed in the record. After D1: `duckdb .zorp/zorp.duckdb "select redundancy_score, feasibility_score, redundancy_citations, feasibility_citations, verdict from validations order by created_at desc limit 1"` | Every citation behind a nonzero score has a `source` that is a URL the search actually returned. Cross-check at least one against `err.txt` or against a hand-run query. A score above 0 whose citations are all model assertions is a finding: `validate`'s contract is a cited score. Zero extra API calls. | | ⬜ |
| D3 | No native search, no MCP: the gate still fails closed. In a fresh directory: `( unset ZORP_TAVILY_API_KEY; za --yes validate "<a different question>" )` | Exit 1, and stderr tells the user how to fix it. Today's text is `no search-capable tool is available; configure an MCP search server (--mcp or .zorp/mcp.toml)`. In a build with native search compiled in, a message naming only MCP is incomplete: it should also name `ZORP_TAVILY_API_KEY`. Record what is printed; see assumption vi. Zero API calls. | | ⬜ |
| D4 | Native tool withheld by a flavor, no MCP. Fresh directory, key set, `.zorp/flavor.toml` with a `[tools] enabled` list that omits `web_search`: `za --yes validate "<a different question>"` | Exit 1 with the gate message. The allow-list still governs the research path, and a natively registered tool cannot slip past a flavor that withheld it. Zero API calls. | | ⬜ |
| D5 | The MCP path is not regressed. Fresh directory: `za --yes --mcp "stdio:stub:$PWD/target/debug/stub_search_mcp_server" validate "<a different question>"` | Exit 0 with an approved or rejected line, exactly as in run 001 Area E test 3 and run 002. MCP tools still appear as `mcp__stub__*`. Design D5 widens the predicate by exact name and leaves MCP verb matching untouched; this checks that. Zero to two requests, depending on which tool the model reaches for. | | ⬜ |

---

## Area E: Failure handling · 5 scenarios

Design D7: a provider error becomes a `ToolError` naming the provider and
the condition, and zorp never substitutes empty results for a failed
search. This area exists to prove a tester can tell the two apart at a
glance.

**How to tell a failed search from an empty one.**

| Signal | Failed search | Empty result set |
|---|---|---|
| Activity line on stderr | `● web_search  error` | `● web_search` with a zero-count summary |
| What the model is handed | text starting `error: `, naming the provider and the condition | a normal result payload with no items |
| Trace `tool.result` | `"success":false` | `"success":true` |
| Consequence for `validate` | nothing is cited, so nothing may be scored | a legitimate signal of no prior work |

E2 to E4 need the request pointed somewhere other than Tavily. Assumption
iii covers the mechanism. A small fake endpoint is enough:

```bash
cat > fake_tavily.py <<'PY'
import http.server, sys
code, body = int(sys.argv[1]), sys.argv[2].encode()
class H(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def log_message(self, *a): pass
http.server.HTTPServer(("127.0.0.1", 8899), H).serve_forever()
PY
python3 fake_tavily.py 500 '{"error":"boom"}' &
```

| # | Test (command) | Expected | Observed | Verdict |
|---|---|---|---|---|
| E1 | Empty result set, the contrast case. `za --yes "Use the web_search tool with the query zqxjkvwpflmrbn7742nonsense and report exactly how many results came back."` | `● web_search` with a non-error summary reporting zero results. Trace `"success":true`. The model is told there were no results and says so. If the live provider returns something anyway, re-run against `python3 fake_tavily.py 200 '{"results":[]}'` and record that instead. One request, or zero if the fake is used. | | ⬜ |
| E2 | Unreachable endpoint. Point the base URL at a closed port (`http://127.0.0.1:9`), then `za --yes "Use the web_search tool to find the release date of DuckDB 1.0.0."` | `● web_search  error`, trace `"success":false`, and a tool result reading `error: tavily search: request failed: <connection refused>`. Anything that reads as an empty result set here is a blocking fail. Zero API quota spent. | | ⬜ |
| E3 | Non-200 from the provider. Base URL at the fake endpoint started with `500`. | `● web_search  error` and `error: tavily search: HTTP status 500: {"error":"boom"}`. The status and the provider are both named, per D7. Trace `"success":false`. Zero quota. | | ⬜ |
| E4 | Malformed response body. `python3 fake_tavily.py 200 '{"nope":1}'`, same command as E3. | `● web_search  error` and `error: tavily search: malformed response: ...`, not a zero-result summary. This is the exact case the design's testing section calls out: a response missing `results` errors rather than returning empty. Trace `"success":false`. Zero quota. | | ⬜ |
| E5 | A failed search must not become a `validate` score. Fresh directory, no MCP: `( export ZORP_TAVILY_API_KEY=zorp-uat-invalid-key; za --yes --max-steps 12 validate "<a different question>" )` | No approval that rests on nothing. Acceptable: exit 1 with `could not score the search results: ...`, exit 1 with `agent did not complete: ...`, or `validate: rejected`. A run that prints `validate: approved` after every search failed is a blocking fail, and if it does approve, run the D2 query: citations sourced from the model's memory rather than a search result are the failure mode being hunted here. One rejected request. | | ⬜ |

---

## Area F: Secret hygiene · 4 scenarios

Design D6: the key is never logged, never written to the trace, and never
included in a tool result. These four scenarios run over the artifacts the
earlier areas already produced, so they cost nothing.

Two backstops worth knowing about, and not confusing with the guarantee.
`secret_values()` in `zorp-agent/src/sandbox/mod.rs:268` redacts the value
of any environment variable whose name contains `KEY`, `TOKEN`, `SECRET`,
or `PASSWORD`, and `ZORP_TAVILY_API_KEY` matches on `KEY`. The provider
also strips its own key out of any text that came back over the wire,
because Tavily echoes a rejected key in some error bodies. Both are nets
under the trapeze. The requirement is that the key never gets into those
strings in the first place, and F3 is the scenario that actually exercises
the echo path.

| # | Test (command) | Expected | Observed | Verdict |
|---|---|---|---|---|
| F1 | Not in the output of a good run. After B1: `grep -F -- "$ZORP_TAVILY_API_KEY" out.txt err.txt; echo "grep exit=$?"` | No matches, `grep exit=1`. Repeat with the last 8 characters of the key (`grep -F -- "${ZORP_TAVILY_API_KEY: -8}"`) to catch a truncated echo. | | ⬜ |
| F2 | Not in the trace. Same grep over `trace.jsonl`, for the B1 run and for the D1 run. | No matches. The trace carries no tool arguments and no tool content by construction, so the realistic risk is an `infrastructure.error` message that quotes a request. | | ⬜ |
| F3 | Not in any error message, including one that echoes it back. Same grep over A6, E2, E3, and E4. Then run the echo case with a throwaway key: `python3 fake_tavily.py 401 '{"error":"invalid key: zorp-uat-echo-me"}'`, `( export ZORP_TAVILY_API_KEY=zorp-uat-echo-me; za --yes "Use the web_search tool to find anything." )`, then `grep -F zorp-uat-echo-me out.txt err.txt trace.jsonl` | No matches anywhere, and the error body shows `<redacted>` where the key was. Failure paths are where keys leak: an error that prints what the provider said takes the echoed key with it. Zero quota. | | ⬜ |
| F4 | Not in anything zorp persisted. After D1: `grep -rF -- "$ZORP_TAVILY_API_KEY" .zorp "$ZORP_STATE_DB" "$ZORP_TRUST_FILE" "$HOME/.config/zorp" 2>/dev/null; echo "grep exit=$?"` | No matches. The session recorder stores every message, so this catches a key that reached the model's context through a tool result, which is the leak that matters most for a product that writes evidence records to disk. | | ⬜ |

Any hit in Area F is a blocking fail. Rotate the key afterwards if one is
found.

---

## Cost and rate limits

Every live scenario spends real Tavily quota. Rough budget:

| Area | Live requests | Note |
|---|---|---|
| A | 1 | Only A6, and that one is rejected. A1 to A5 reach nothing. |
| B | ~4 | B1 is one, B2 adds a hand-run curl, B4 is two, B3 reuses B1's run. |
| C | 0 | C2 reuses B1. Denials never reach the network. |
| D | ~6 to 12 | `validate` decides how many times to search. `--max-steps 12` caps it. D3 and D4 spend none. |
| E | ~2 | E1 and E5. E2 to E4 never reach Tavily. |
| F | 0 | Greps over files the other areas produced. |
| **Total** | **~15** | Budget 40 to allow for retries and one re-run of D1. |

The single most expensive thing here is re-running D1, because a local
model that wanders can search four or five times per `validate`. Cap it
with `--max-steps` and check the count in the trace before assuming the
harness is at fault.

Rate limits are not tested on purpose. A deliberate burst costs money and
time and produces the same signal E3 already produces synthetically. If a
429 happens by accident, treat it as free evidence for Area E: it must
render as `● web_search  error` naming the condition, never as an empty
result set. Record it if it happens.

---

## What this plan deliberately does not cover

- **Result quality and relevance.** Whether Tavily returns good results,
  and whether the model picks the right ones out of them, is a provider and
  model concern. The harness's job is to carry the query out and the
  results back without mangling either. A well-formed answer that is wrong
  on the facts is not a harness failure and will not be filed as one.
- **Whether a novelty score is the right number.** B2 and D2 check that a
  score is backed by a citation to something the search actually returned.
  Whether 40 was the correct redundancy score for a given question is not a
  UAT question and has no ground truth to test against.
- **Provider portability.** Design D2 claims a second provider needs no
  changes above the trait. There is no second provider yet, so the claim
  cannot be exercised. It becomes testable the first time Brave or Exa is
  added.
- **Caching.** Ruled out by the design, deliberately, because it interacts
  with pre-registration. Nothing to test.
- **Deliberate rate-limit exhaustion.** See the cost section.
- **Concurrency.** Two research commands at once collide on the DuckDB
  lock. That is run 002's finding G3, in the track store, and this feature
  does not touch it.
- **Corporate TLS interception and proxies.** Out of scope for a first
  acceptance pass. If the tester's network has one, say so in the report,
  because it changes what E2 and E3 mean.
- **Windows.** The workspace is Unix only (libc, POSIX process groups in
  the sandbox), so there is no Windows path to accept.
- **Credit accounting inside zorp.** zorp does not count or report Tavily
  credits, so there is no behavior to test. The budget above is the
  tester's own bookkeeping.

---

## Assumptions where the design is silent

The design pins some strings exactly and leaves others open. Where it is
open, this plan states what it assumed so a later reader can tell a wrong
assumption from a real defect.

i. **Missing-key behavior: resolved, no longer an assumption.** An earlier
   draft of D6 said "a clear startup error". The shipped behavior warns and
   skips registering the tool, and the design was amended to say so, for
   exactly the reason this plan anticipated: a hard exit would make a
   `search` build useless for non-search work. Verified against the built
   binary before this plan was finalized:

   ```
   $ zorp-agent "reply with exactly: pong"        # search build, no key
   zorp-agent: web_search unavailable: tavily search: no API key; set ZORP_TAVILY_API_KEY
   pong                                            # exit 0
   ```

   A3 and A4 now assert that shape rather than accepting either.

ii. **An empty key is a missing key.** The design does not say so. The
    implementation agrees: the provider trims the key and treats an empty
    result as missing, which also covers a whitespace-only value. A5 tests
    both. This assumption is the safe one and is now backed by code.

iii. **Base URL override: resolved, no longer an assumption.** This plan
     flagged that without an override, E2 to E4 could not be run at all and
     a failure path nobody can exercise outside a unit test is a failure
     path nobody has checked in the field. `ZORP_TAVILY_BASE_URL` was added
     for that reason, with the name this plan predicted. Set it to a local
     stub for E2 to E4 and leave it unset for every live scenario. The whole
     chain was already exercised this way against a stub before any real key
     existed: approval gating, a rendered result set, and a `validate` run
     with no MCP server configured.

iv. **The activity line summary is not pinned.** The design says the user
    can see a search in the activity line and nothing more. The assertions
    require only `● web_search` plus a summary that is neither `denied` nor
    `error`. Any summary that names the query, the result count, or both is
    acceptable.

v. **The zero-result tool payload is not pinned.** The plan requires only
   that it is distinguishable from an error by the activity summary and by
   the trace's `success` field, which is the property D7 actually cares
   about.

vi. **The gate message may not have been updated.** D5 widens
    `name_can_search`, but the design never says the `NoSearchTool` text
    changes. In a build with native search compiled in, the current text
    names only MCP. D3 records what is printed and treats an MCP-only
    message as a low finding rather than a fail, because the design does
    not require otherwise.

vii. **The query on the wire is model-authored.** D4's rationale is that a
     search sends the user's question to a third party. No scenario asserts
     an exact query string, because the model composes it. A tester with an
     intercepting proxy can check what actually left the machine, and that
     would strengthen Area F, but it is not required for a pass.

---

## Exit criteria

**ACCEPT** requires every scenario in Areas D, E, and F to pass. Those are
the three claims: `validate` without MCP, failure that never reads as
empty, and a key that stays inside the process.

**Blocking fails**, any one of which stops the feature:

- Any Area F hit. The key appears in an output stream, the trace, or
  anything on disk.
- Any case where a failed search reaches the model, the activity line, or
  the trace as an empty result set.
- `validate` approving a track on the back of searches that all failed
  (E5).
- D1 still printing `no search-capable tool is available` with a working
  key and the feature built in.

Failures in Areas A, B, and C that come down to message wording, exit
codes, or an unpinned summary string are findings, filed with a severity,
not blockers. That matches how runs 001 and 002 treated F2 through F6.

---

## Methodology notes

- This plan is written from the design. The implementation was still being
  written when it was drafted, so the provider error strings quoted above
  were read off `zorp-search/` mid-flight and the tool adapter
  (`zorp-agent/src/search_tool.rs`) did not exist at all. Every string that
  the design itself does not pin is an expectation, not a quotation. Check
  them against the built binary before running, and say in the report which
  ones moved.
- One tester, sequential, one machine, as in runs 001 and 002. Run 002's
  own note applies again: the weakest position from which to confirm a fix
  is the position of the person who wrote it. If the author of the search
  feature runs this plan, say so in the report.
- Every scenario is black box against the real binary. No source edits, no
  `cargo` invocations except the two builds in A1 and A2.
- Live search counts, per run, should be read out of the trace
  (`grep -c '"event_type":"tool.call".*web_search' trace.jsonl`) rather
  than guessed, both for the cost budget and to catch a tool that searches
  more often than the activity lines suggest.
- The model is the slowest and least predictable part of this. Run 002 had
  a `deliver` run exceed a ten minute command budget. Expect the same from
  `validate` in Area D and run it detached if needed.
- Nothing here tests `investigate`, `co-write`, or `deliver`. None of them
  gained a search path in this design, and `deliver`'s huiban gate is
  explicitly untouched by D5.
