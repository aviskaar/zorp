---
status: accepted
date: 2026-09-10
---

# three surfaces report what skills are installed, and none of them loads one

**This extends 2026-08-18 ("skills are read, and a skill body grants
nothing").** Nothing in that entry is withdrawn.

**Decision:** `zorp` core, `zorp-agent`'s chat REPL and `zorp-web` can each
say what skills are installed. `zorp --skills` and `zorp --skill <name>
<prompt>` list and apply one, `/skills` in the REPL prints the same index the
model is shown, and `GET /api/skills` answers with names, descriptions, paths
and scopes while `GET /api/capabilities` reports a count. All three read the
scopes through `zorp_skill::scope_dirs_from_env` rather than re-deriving
them, so the list a person is shown is the list the agent would register.

**None of them loads a skill, and there must never be a route that does.**
Loading is the `skill` tool, chosen by the model when the task matches a
description and gated exactly as every other tool call is. A control that
pastes a body into the composer would make untrusted text look like something
the person wrote, which is the thing 2026-08-18 arranged against. That is why
the issue's suggested composer slash command is deliberately absent, and why
`zorp-web/tests/skills.rs` asserts `/api/skills/:name` and
`/api/skills/:name/load` are 404 and that the listing carries no body.

**In `zorp` core the body goes in front of the user's own words and never
into the system prompt.** The system slot is the one channel the harness
speaks in, and a `SKILL.md` is a file this binary did not write.

**A skill's description is text somebody else wrote, so the page treats it
like model output.** `skills-view.ts` builds no HTML strings, and a skill
declaring `allowed-tools` says on the page that zorp does not grant them, so
the gap between what a skill asks for and what it gets is visible rather than
discovered.
