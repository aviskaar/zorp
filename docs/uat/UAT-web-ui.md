# UAT: the web UI's model settings and artifact pane

Hand-run acceptance test of `zorp-web` against a live local model and a
real browser. Black-box: the running server and the shipped bundle, driven
over HTTP and through Chrome. No source edits during the run.

**Build:** `zorp-web 0.3.2`, debug, at `feat/artifact-pane`.
**Model:** `ollama serve` on `127.0.0.1:11434`, six models installed,
`qwen3:4b` for the turns.
**Date:** 2026-08-18.
**Scope:** the settings panel added in #48 and the markdown renderer and
artifact pane added in #50. Not a re-run of the 001/002 series, which
covers the CLI.

**Result: ACCEPT.** 24 scenarios, 24 pass. Four defects were found and
fixed during the run, listed at the bottom; every one of them is a real
bug that the test suite as it stood did not catch.

## A. Model settings

| # | Scenario | Expected | Observed | Verdict |
|---|---|---|---|---|
| A1 | `GET /api/settings` with no `ZORP_*` set and no config file | `configured: false`, every source `default` | as expected | pass |
| A2 | `GET /api/settings/models` against real Ollama | the installed model ids | all six, `error: null` | pass |
| A3 | `PUT /api/settings` with Ollama's base URL and `qwen3:4b` | 200, `configured: true`, sources `ui` | as expected | pass |
| A4 | The file written to disk | provider, base_url, model. No key. | exactly those three keys | pass |
| A5 | `PUT` with `base_url: file:///etc/passwd` | 400 | 400 | pass |
| A6 | `POST /api/settings/test` naming an unreachable candidate | `ok: false`, and nothing saved | `ok: false`; no config file created at all | pass |
| A7 | `POST /api/settings/test` with no body after a good save | `ok: true` against the saved endpoint | `ok: true` | pass |
| A8 | A turn with nothing configured | a readable error naming settings, not a provider 401 | error event says "no model configured" | pass |
| A9 | A real turn on `qwen3:4b` | the model's answer streams back | `assistant: "pineapple"`, then `done` | pass |

## B. The settings panel in a browser

| # | Scenario | Expected | Observed | Verdict |
|---|---|---|---|---|
| B1 | Open the panel from the top bar | provider, base URL, model, key fields | all present, Ollama preselected | pass |
| B2 | The model dropdown | populated from the live endpoint | all six real model ids, `qwen3:4b` selected | pass |
| B3 | Provenance labels | say where each value came from | "saved" under base URL and model | pass |
| B4 | The API key field's explanation | says memory-only and names the env var | as expected | pass |
| B5 | Type a dead URL, press Test | reports the failure | error shown, single URL, not doubled | pass |
| B6 | The saved config after that failed test | unchanged | still Ollama's URL, on disk and in `GET /api/settings` | pass |
| B7 | The panel at a short viewport | scrolls, buttons reachable | `max-height: 82vh; overflow-y: auto` | pass |

## C. The artifact pane

| # | Scenario | Expected | Observed | Verdict |
|---|---|---|---|---|
| C1 | `GET /api/artifacts` in a workspace with a track dir | the md, txt and pdf; not the noise | `draft.md`, `notes.txt`, `paper.pdf`; `target/` and `node_modules/` absent | pass |
| C2 | Open `draft.md` in the pane | rendered, not raw | heading, bold, italic, ordered list, table, blockquote, fenced code with lang tag, inline code, link | pass |
| C3 | Open `paper.pdf` | inline in the pane | browser's own viewer, page visible | pass |
| C4 | Open a plain `.txt` | monospace, unrendered | as expected | pass |
| C5 | Refresh after writing a new file | the new file appears | appeared, and the open file stayed open | pass |

## D. The parts that had to be attacked

| # | Scenario | Expected | Observed | Verdict |
|---|---|---|---|---|
| D1 | `path=../uat-web.toml` | refused | 403 | pass |
| D2 | `path=%2E%2E%2F%2E%2E%2Fetc%2Fpasswd` | refused | 404 | pass |
| D3 | `path=%2Fetc%2Fpasswd` (absolute) | refused | 403 | pass |
| D4 | `path=.zorp%2F..%2F..%2Fuat-web.toml` | refused | 403 | pass |
| D5 | A symlink inside the workspace pointing out of it | refused, target not leaked | refused; contents never appear | pass |
| D6 | `path=secret.env` | refused on type, contents not served | 415, no contents | pass |
| D7 | Headers on a served PDF | `nosniff` and a sandbox CSP | both present, `application/pdf` | pass |
| D8 | A markdown file full of injection attempts, opened in the pane through the real bundle | nothing executes, nothing fetched, only safe links clickable | `xssRan: false`; 0 scripts, 0 images, 0 iframes, 0 svgs, 0 event handlers; the only anchor is the one `https://` link; the beacon URL is visible as text | pass |

D8's input contained `<script>`, `<img onerror>`, `<iframe src="javascript:">`,
`<svg onload>`, a raw `<a href="javascript:">`, a `javascript:` markdown
link, a `data:text/html` link, and a markdown image pointing at a tracker.

## E. Mutation checks

A test that has never failed has not been shown to test anything. Several
tests here passed the first time they ran, so the load-bearing ones were
checked by breaking the code under them.

| # | Mutation | Expected | Observed | Verdict |
|---|---|---|---|---|
| E1 | Delete all three settings routes from the router | `settings_endpoints_are_gated_too` should fail | it **passed** | see F1 |
| E2 | Same, against the replacement test | should fail | failed, with a readable message | pass |
| E3 | Remove the not-configured guard from `turn.rs` | the turn test should fail | failed, for the right reason | pass |
| E4 | Remove the containment check from `artifacts::resolve` | the traversal tests should fail | exactly the two traversal tests failed, nothing else | pass |

## Findings

All four were fixed during the run and each has a regression test.

**F1. `settings_endpoints_are_gated_too` proved nothing.** Severity:
medium. The token layer wraps the router and answers before routing, so an
unmatched path is refused exactly like a matched one. With all three
settings routes deleted, every assertion in the test still passed (E1).
The test could not tell "gated" from "absent". Replaced with one that
requires a 200 and a settings document from a good token, which fails when
the routes go away (E2).

**F2. Test wrote to disk before testing.** Severity: medium. `POST
/api/settings/test` only ever looked at stored state, so the panel saved
the form before testing it. A button that reads like a question overwrote
`~/.config/zorp/web.toml` to ask it, and an address that turned out to be
wrong destroyed the working one already there. The endpoint now takes an
optional candidate and stores nothing.

**F3. The scheme check guarded the read path, not the write path.**
Severity: medium. `validate_scheme` ran in `fetch_models`, the read-only
probe, and not in `apply`. `file:///etc/passwd` could be persisted and
handed to the model call on every later turn. Now checked where the value
is stored.

**F4. Every markdown image was a clickable link.** Severity: low. The link
pattern matched the `[alt](url)` inside `![alt](url)` and left the `!` as
text. Nothing was fetched, since no `img` element was ever created, so
this is not the beacon the design was guarding against, but it is not the
plain text the design promised either. Caught in D8, not by review, and
not by the spec, which says images render as text.

## Notes

- A fifth defect, unrelated to these features, showed up as a CI failure
  during the run: the gate test raced a request body the server never
  reads, which macOS turns into an RST that discards the response. Fixed
  separately; it is a test bug, not a product one.
- `web/` had no test harness before this run. It now has jsdom and
  `node:test` wired into CI, 19 tests, most of them injection cases.
- Nothing here generates a PDF. C3 renders one that already exists.
