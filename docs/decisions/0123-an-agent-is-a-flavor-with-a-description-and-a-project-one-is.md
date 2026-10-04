---
status: accepted
date: 2026-09-13
---

# an agent is a flavor with a description, and a project one is untrusted until somebody clicks

**Decision:** `zorp-agent/src/agents.rs` reads the two `flavors/`
directories the CLI already reads and presents them as agents. `Flavor`
gains `description` and nothing else. `GET /api/agents`,
`GET /api/agents/:scope/:name`, `POST /api/agents/:scope/:name/trust` and
`PUT /api/sessions/:id/agent` are the browser's surface;
`zorp-agent agents` is the terminal's.

**Not a new format, and that is the decision.** zorp has four things
somebody could call an agent. A capsule is instructions a person loads mid
session; a skill is instructions the model loads mid turn; a subagent is a
child run the model spawns. An agent is chosen by a person, before the
conversation starts, and carries the model, the prompt, the tool allow-list
and the approval preset for the whole of it. That is exactly a flavor. A
file in `flavors/` is a CLI flavor and a browser agent at the same time,
resolved by the same `layer_paths` and merged by the same `Flavor::merge`.
If a second file format ever appears here, this was implemented wrong.

**The name is stored and the scope is not.** `sessions.agent` follows
`sessions.project_id` exactly: nullable, added in `migrate_session_columns`
and never in `SCHEMA`, copied verbatim by `branch_session`. Only the name
goes in, because the scope is resolved at turn time through `layer_paths`
the way the CLI does, so a user agent and a workspace agent of the same name
merge the way they do on the CLI. A stored scope would have frozen that.

**Locked once the conversation has answered.** `Store::set_session_agent`
refuses with `SetAgent::Locked`, so the CLI gets the same rule rather than
the browser getting it alone. A transcript whose first half ran under one
system prompt and tool set and whose second half ran under another is one
nobody can read back honestly: the record would show a model refusing to
write a file on turn two and writing one on turn five with nothing to
explain it. The alternative, a marker frame saying "now running as X", is
more code for a worse record. Branching already copies the agent with the
row, which is what makes "branch it" a real answer.

**Project scope is gated by content hash, and that gate is load bearing.**
The model can write a file into `<workspace>/.zorp/flavors/` with
`write_file`. So a workspace agent that carries shell commands or loosens
approval applies those fields only once a person has trusted its current
hash, and editing the file produces a different hash and revokes the trust
without anybody remembering to. An agent that only narrows is not gated at
all: discarding it would throw away a restriction somebody asked for, which
is the opposite of what the gate is for. User scope is trusted because the
person put the file there, which is the rule the CLI already applies.

`no_tool_picks_an_agent_or_trusts_one` in `agent.rs` is the other half. The
hash gate is worth nothing if a tool can also click the button: a model that
could write a flavor granting itself `full` approval and then trust it has
granted itself full approval.

**A browser turn cannot stop and ask, so it says so on the stream.**
`gated` returns the flavor and whether anything was withheld, and `turn.rs`
renders a notice naming the agent. Refusing the turn outright would be
worse, since the restriction an agent usually carries is the reason somebody
picked it.

**An agent replaces what zorp is. It does not get to drop where files go.**
`scoped_prompt` appends the workspace sentence to whichever prompt won,
because that is the one thing only the server knows and an agent whose
prompt omitted it would write into the top of somebody's repository. The
prompt still reaches the model through `plan_seed` and never enters
`messages`, which matters more now that it can be untrusted text out of a
cloned file.

**An agent can only narrow.** `register_builtins_filtered` picks from what
the build has and cannot add a tool it lacks. Where an agent's approval
preset and the toolbar toggle disagree the stricter wins, and
`with_own_server` is applied after the preset either way: one approved
`run_command` that curls this server is otherwise enough to stand the
approval gate down, and an agent is a file that may have arrived by
`git clone`.

**A file that does not parse is listed as broken, never dropped.** An agent
exists mostly to restrict what a run may do, so one that silently vanishes
is a run that silently loses its restrictions.
