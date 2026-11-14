---
status: accepted
date: 2026-11-02
---

# the speed dial is nine saved slots, and a press is a settings save

**Decision:** the model speed dial is up to nine numbered slots in the one
`zorp.toml`, each carrying exactly the non-secret fields a settings save
carries (provider, base URL, model, max tokens) and an optional label. A
slot type has no field for an API key, so a slot cannot hold a secret even
if a caller tried. Pressing a slot is `PUT /api/settings` with the slot's
fields and nothing else: the same save the model form does, under the same
rules (the base URL's scheme is checked, an unknown provider is a 400), so
the next turn reads the new model the way it already reads a saved one. The
turn code is not touched. The chord is Control+Alt+digit (Control+Option on
a Mac) for 1 to 9, read from `event.code` so Option+digit on a Mac still
counts, and it fires from inside the composer and calls `preventDefault`. A
digit with no slot behind it does nothing and says nothing. Labels are a
person's own text off the settings file and land through `textContent`, the
same path a model name off a provider listing uses.

The terminal has no chords: `/dial` in the chat REPL lists the slots and
`/dial <n>` switches to one, and `zorp-agent config dial <n>` does the same
from the CLI. Both read the same slots out of `zorp.toml` and write the
slot's fields to the saved file. In the terminal the switch lands on the
next session, because the chat REPL resolves its model once at start and
holds a built client for the conversation; it does not re-resolve per turn
the way the browser's turn does. That is the terminal's honest version of
the browser's rule that a press during a turn changes the next turn, not the
running one.

**Why:** switching models in the browser was a four step trip (open the
form, pick a preset, wait for the listing, pick the model, save), long
enough that people stopped switching and ran everything on one model. A
dial is nine choices a person already made, one chord each. Making a press
an ordinary settings save, rather than a new path into the turn, means it
inherits every rule the save already has and adds no way for a slot to carry
a key or for a running turn to change model under itself. Control+Alt+digit
is the one chord free in every browser this runs in and in the desktop web
view: Command+digit and Control+digit switch browser tabs everywhere, and
Alt+digit switches tabs in Firefox on Linux.

**Constraints that are not negotiable:**

- A slot never holds a key. `DialSlot` has no `api_key` field, `to_persisted`
  cannot be handed one, and a round trip through `zorp.toml` is grepped for
  the key the way `to_persisted_never_carries_the_api_key` greps the core
  fields.
- A press is a save and nothing more. No new path into the turn; the same
  `validate_scheme` and the same 400 an unknown provider gets.
- An agent's model wins. A conversation under an agent that sets `model`
  (#241) is not moved by the dial, because a flavor sits above the saved file
  in the chain. The pill shows the agent's model and the dial does nothing
  there rather than writing a file that will not be read.
- No model can press a slot. There is no tool for it and
  `no_tool_presses_a_speed_dial_slot` in `agent.rs` says so.

See issue #242. Builds on the settings pane (#240) and the shared settings
file (2026-09-10). The per-slot chord, a second API key, a per-conversation
model, and a recent-models list are all out of scope; a conversation that
needs its own model is an agent (#241).
