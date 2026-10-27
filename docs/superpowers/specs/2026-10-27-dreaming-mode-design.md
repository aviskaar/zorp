# Dreaming mode

While zorp is idle, it goes back over how a person works and comes back with
suggestions that improve their experience, each one a proposal backed by the
exact conversations it came from and doing nothing until the person accepts
it. Examples:

- "you ask for this report every Monday, want a flavor for it?"
- "these three conversations are about one project, file them together?"
- "this command fails every time with X, here is the fix you used last time."

This is the design, pending approval (ADR 0144, status `proposed`). It is
written first because storing anything a model concluded about a person is the
one move zorp has refused from the start (ADR 0063), so the shape has to be
agreed before any code.

## The rule it is designed around

zorp has no stored sentence a model composed about the person: no claim table,
no "learned facts". The agent's guesses must never become its own evidence.
Dreaming mode keeps that four ways:

1. **Detection is code; the model only words it.** Detectors run over
   code-derived columns and return evidence rows, never prose. A model phrases
   a suggestion from the pattern and the verbatim messages, inside the fenced,
   nonce-marked `user` message `memory` uses. No detector reads a
   model-authored column.
2. **A suggestion is a proposal.** It is shown with its evidence and does
   nothing until accepted. Accepting creates an ordinary object a person could
   have made by hand. A rejected one is recorded rejected and not raised
   again.
3. **Suggestions never reach recall, memory or `messages`.** They live in a
   `suggestions` table, labelled model-authored, the way `display_title` is
   kept from `task` (ADR 0069).
4. **It is local, off by default, and a person starts it.** No model tool
   turns it on or acts on a suggestion. Listed in `state.rs`.

## Detectors (code, over code-derived columns)

- **Recurring requests:** near-identical first messages on a schedule, by
  embedding similarity over the recall index plus message timestamps.
- **Repeated failures:** the same command failing across sessions, by
  code-derived tool status.
- **Unfiled clusters:** conversations near each other in the index with no
  project.
- **Repeated manual approvals:** the same command approved by hand over and
  over.

Each returns the evidence rows (ids, timestamps, statuses), not text.

## The dream pass

A background worker beside the recall sweep (ADR 0079), gated on `ZORP_DREAM=1`
and an interval setting, off by default. It runs the detectors, and for the
patterns they return asks the person's configured model to phrase at most N
suggestions, each from the pattern and the verbatim messages it cites, in the
fenced nonce-marked block. It writes the results to the `suggestions` table.

## Storage

`suggestions`: `kind`, `evidence_ids` (references, not copied text),
`wording` (model-authored, labelled), `state` (`new` | `accepted` |
`dismissed`), timestamps. Nothing that feeds a model reads this table.

## Surfaces

- A "While you were away" card in the browser and the Mac app, each
  suggestion with its evidence links and Accept / Dismiss.
- CLI: `zorp-agent dreams`.
- Accept creates the real object through the same code path a person uses:
  project create, flavor file write (untrusted until trusted, ADR 0123),
  scheduled task.

## Tests (when built)

- Detectors on fixture stores.
- The suggestion text never appears in `messages`, the recall index, or a
  memory block (a test like `there_is_one_chunker_and_one_fingerprint`).
- No tool starts a dream pass or acts on a suggestion.

## Out of scope

- Any suggestion kind whose evidence would be model-authored text.
- Acting on a suggestion without the person's accept.
