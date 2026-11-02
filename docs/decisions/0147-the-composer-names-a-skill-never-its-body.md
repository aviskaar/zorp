---
status: accepted
date: 2026-11-02
---

# the composer names a skill, never its body

**Decision:** typing `/` in the browser composer opens a picker of installed
skills; picking one writes `/name ` into the composer as plain text and the
turn request carries a `skill` name. The server rechecks the name against
the same registry the listing is built from (`is_valid_skill_name`, 400 for
an unknown one, never joined onto a path) and puts one zorp-authored
sentence on the seed, in the position and by the mechanism the memory block
uses, asking the model to load the skill with the `skill` tool. The body
never travels through the page, the request, a `user` message, or the store.
A turn that named a skill and made no `skill` call gets a "did not load"
line under the answer, derived in code from the turn's tool events.

**Why:** the browser could list installed skills but not choose one for a
message, while the terminal already had `zorp --skill`. The gap was closed
with a name rather than a body on purpose. ADR 0119's surrounding work and
the 2026-09-10 decision had left a composer slash command out because a
control that pastes a skill's body into the composer makes untrusted text
look like something the person wrote. That objection is to the body reaching
the composer; it does not reach a name. The three places a body could have
travelled were all refused: the composer (2026-09-10), a stored `user`
message (where `recall` embeds it, `memory` quotes it, and `title` reads it),
and a second fenced path on the seed with its own nonce and test. Naming the
skill and letting the model's own `skill` tool load it adds none of them, at
the price that the model has to comply, which the page reports.

**What this rules out:** loading a skill from a route (there is none and
`zorp-web/tests/skills.rs` says so), moving a skill body to the page or the
request, and keeping a skill chosen for the rest of the conversation (it is
per message; a sticky choice is a separate issue if people retype it). The
terminal REPL gaining the same `/skill <name> <prompt>` is parity work
tracked separately; this decision is the browser.

See issue #252.
