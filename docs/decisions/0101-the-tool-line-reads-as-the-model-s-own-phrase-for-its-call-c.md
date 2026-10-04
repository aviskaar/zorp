---
status: accepted
date: 2026-09-04
---

# the tool line reads as the model's own phrase for its call, clamped in code

**Finding:** the transcript's tool line showed a shell brief, the program
and its first few arguments, as in `run_command ls web/src …`, and that
was noise to a person reading a turn. They want to know what the agent
was doing: listing files, running the tests, converting a document. The
first fix was a table of verbs keyed by program and subcommand, computed
in code, and a table cannot cover what a model can type: every script,
every program the table does not know and every pipeline read as
`Running x`. A second model call per tool call, asked afterwards to
describe what just ran, was considered and rejected. It costs a call per
tool use, and on a local model it contends with the turn itself for the
same hardware.

**Decision:** `run_command` and `start_background_process` take an
optional `description` argument next to `command`, and the model writes
it in the same call it is already making. The agent hands it to the
renderer through `Renderer::tool_described`, which defaults to `tool`;
the terminal renderer keeps that default, so the CLI's output is byte
for byte what it was. zorp-web puts it on the `tool` event as `phrase`,
absent when there is none, and the browser draws it clamped in code,
one line with control and bidirectional characters stripped and capped
at `BRIEF_MAX`, in italics and labelled as the model's own description
of the call. A call with no description gets the code-derived phrase
from the table in `web/src/activity-line.ts`, which stays as the
fallback. `RepeatGuard` fingerprints a call without its `description`,
so a fresh sentence over the same command cannot hide a repeated call.

A reopened session draws the same lines from the store. The call and its
`description` are already in `messages.tool_calls`, and the result's
status is derived back from the stored content in code by
`summary_from_content`, so `get_session` replays each stored call as a
`tool` entry in its stored order and the browser hands it to the same
`toolLine`. No new column and nothing new written. The recall index and
the memory block read message text and never `tool_calls`, which is what
keeps the phrase out of the evidence.

**Why:** no extra call. The phrase travels with the call it describes
and sits in the transcript as part of that call's arguments, which are
already the model's own words, so the record gains no new model
authored line. Beyond that it is display only: nothing reads it, the
tool ignores it when it runs, and no other tool sees it. And it can be
wrong, which is why the verbatim command stays one click under the line
and the approval card still shows the arguments whole.
