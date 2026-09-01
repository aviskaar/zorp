# Code mode: a file tree, real diffs, and a scratch directory to work in

Date: 2026-08-31. Status: proposed design.

zorp's web UI already runs a full coding agent: `zorp-agent` reads,
writes, patches, and shell-executes, gated by approval, and `zorp-web`
streams every turn over SSE with approval cards the browser already
renders. What it does not do is look like a coding tool. An edit shows
up as raw JSON in an approval card, not a diff. There is no file tree,
only a flat "Files" popover of whatever a turn happened to produce. And
every session works in whatever directory the server happened to start
in: there is no way to start a fresh, contained coding session from the
browser.

This design covers both: making edits legible as diffs, and letting a
session start in a directory made for it. Nothing here is a new
capability the agent does not already have. It is exposing what
`zorp-agent` already computes and `zorp-web` already stores, plus one
narrow new write path (create a directory) with the same care every
other write in this codebase gets.

## What ships

Five pieces, in delivery order, each shippable and useful alone:

1. **Tree-ify the Files popover.** `web/src/main.ts`'s existing
   `renderArtifactList` becomes a nested, collapsible tree instead of a
   flat list, over the same `/api/artifacts` data it already fetches.
   Frontend only, no backend change.
2. **`GET /api/sessions/:id/changes`.** Exposes the `FileChange`
   before/after text that `zorp-agent`'s `apply_patch` and `write_file`
   tools already persist via `Store::record_change` (`zorp-agent/src/
   session.rs`) and that nothing currently reads back over HTTP.
   Read-only, scoped to a session id the caller already holds.
3. **A diff viewer component.** Takes a `FileChange`'s before/after and
   renders a line-level diff, built as DOM nodes and read out through
   `textContent` only, the same rule `markdown.ts` and
   `streamed-message.ts` already follow, because both sides of the diff
   are model-influenced text. `patch.rs`'s `line_delta()` is a multiset
   count, not a diff; this needs a small pure-JS line-diff, not a new
   Rust dependency.
4. **Diff-aware approval cards.** `appendApproval` renders the pending
   diff, via the same component, for `apply_patch`/`write_file` calls
   instead of raw tool-call JSON. This is the moment the feature is
   actually for: seeing what a write will do before approving it.
5. **Code mode: create a directory, start a session there.**
   `POST /api/sessions` gains an optional new-directory parameter.
   `SessionState` gains a `workspace` field. `turn.rs`, `panel.rs`, and
   `investigate.rs`, which each independently call
   `std::env::current_dir()` today, read the session's workspace
   instead.

## What stays out

No live diff streaming mid-turn: the diff viewer is pull-based, fetched
after a turn completes, the same way `checkForProducedArtifacts`
already fetches the Files popover's contents. If per-tool-call live
diffs in the approval card turn out to matter later, that is a small,
additive addition to the `approval_request` event payload, not a new
mechanism, and is out of scope here.

No new approval preset. A freshly created code-mode directory gets the
same `ReadOnly` default and the same per-chat `auto_approve` toggle as
any other session. Nothing about a directory being empty changes what
it takes to approve an edit or a shell command in it. This was an open
question raised during scoping; it is resolved, not deferred: no
special case.

No separate CORS policy. The new endpoints (`changes`, the tree
variant of `artifacts`, and directory creation) ride the same
`--allow-origin` allowlist `/turn` already uses. This was the other
open question raised during scoping; also resolved, not deferred: same
policy as every other session-scoped route.

No new top-level UI surface. Code mode is the existing chat interface.
Creating a directory is a variant of starting a session, not a
different page or mode switch.

## Architecture

**Backend, new or changed:**

- `GET /api/sessions/:id/changes` — reads `Store::load_changes`,
  returns the ordered `FileChange` list for that session. No new
  filesystem reach: the data is already persisted server-side by the
  existing write tools.
- A tree-shaped listing alongside `/api/artifacts`. The traversal,
  depth cap, and file classification in `artifacts.rs` (`list()`/
  `resolve()`) are reused as-is; this is a response-shape change, not
  new logic.
- `POST /api/sessions` accepts an optional directory name. When
  present, it creates the directory under the configured workspace
  root and scopes the new session to it. Path resolution reuses
  `Context::resolve_for_create`'s traversal check, the same one that
  already guards the agent's own file writes, so a name that would
  escape the root is refused the same way an escaping write already
  is. This is the one genuinely new write surface in this design: a
  browser-reachable `mkdir`, reachable before any approval exists for
  the directory it creates. It gets exactly this one check and no
  more; it does not get its own approval prompt, consistent with "no
  new approval preset" above.
- `SessionState` gains a `workspace: PathBuf` (or equivalent). `turn.rs`,
  `panel.rs`, and `investigate.rs` read it instead of calling
  `std::env::current_dir()` independently, which is what currently
  makes every session on one process share one directory with no way
  to differ.

**Frontend, new or changed:**

- File tree component, replacing the flat popover list, reusing
  `listArtifacts`/`readArtifact` from `web/src/api.ts` unchanged.
- Diff viewer component: a small line-diff over `FileChange.before`/
  `after`, DOM-built, no `innerHTML`.
- `appendApproval` updated to render a diff for patch/write calls.
- Session creation UI gains an optional "new directory" input, calling
  the extended `POST /api/sessions`.

**Data flow:** a turn runs and writes files exactly as it does today;
`zorp-agent` already records each `FileChange`. After the turn ends,
the browser fetches `/api/sessions/:id/changes` the same way it
already fetches produced artifacts, and renders the tree and diffs
from that response. Nothing about the turn's own streamed event
protocol changes.

## Security boundary

Three surfaces, three boundaries:

- **Reading the tree / reading a file's content:** unchanged from
  today's `/api/artifacts` — read-only, traversal-safe, sandboxed
  content-type handling for anything a browser could execute, gated by
  the standard `--allow-origin` allowlist. No new exposure; a second
  door onto data the agent could already read.
- **Reading a session's recorded changes:** same allowlist, read-only,
  scoped to a session id. No new filesystem reach: it serves data
  `zorp-agent` already wrote to the store.
- **Creating a directory:** the one real new write path. Constrained to
  the configured workspace root via the existing traversal-refusal
  logic; no absolute path from the browser is ever honored; no new
  approval gate, because approval already governs what happens inside
  the directory once a session starts there, and an empty directory has
  nothing an approval would protect.

## Testing

Rust: the directory-creation endpoint's traversal refusal (a name that
tries to escape the workspace root must fail the same way an escaping
write already does), and `SessionState.workspace` actually reaching
`turn.rs`/`panel.rs`/`investigate.rs` instead of `current_dir()`. TDD,
per repo convention: write the failing test for each before the
implementation.

TypeScript: `node:test` coverage for the diff renderer following the
injection-safety pattern `markdown.ts` and `streamed-message.ts`
already use, since a `FileChange`'s before/after text is
model-influenced and must go through `textContent`, never
`innerHTML`. `npm run check`, `npm test`, and `npm run build` from
`web/`, per repo convention.

## Delivery order

1. Tree-ify the Files popover — frontend only, smallest, ships first.
2. `GET /api/sessions/:id/changes` — backend, unblocks the rest.
3. Diff viewer component — usable standalone once (2) exists.
4. Diff-aware approval cards — depends on (2) and (3); this is the
   feature's actual payoff.
5. Code-mode directory creation and `SessionState.workspace` threading
   — last, because it is the one piece that changes an existing
   architectural assumption (one process, one directory) rather than
   adding a new read over existing data.

Each step leaves the tree working and is a candidate for its own
implementation plan and PR.
