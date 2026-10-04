---
status: accepted
date: 2026-09-05
---

# the tool line carries its result as colour, and the browser hears a call start

**Decision:** the browser's tool line no longer writes its status word
(`exited 0`, `timed out`, `created a.md (3 lines)`) beside the phrase. The
result is the bullet's colour, from exactly one of three classes on the
`.activity-line`: `activity-ok`, `activity-fail`, `activity-running`. The
word is the line's `title`, and for a shell call it is also under the
verbatim command in the details, so anyone who wants it has it one hover or
one click away. `stateForStatus` in `web/src/activity-line.ts` is the one
mapping. `exited 0` and `passed` are ok. A non-zero exit, `timed out`,
`cancelled`, `failed`, `error`, `denied`, `unknown tool`, a withheld
result, a patch of which nothing applied, and the subagent tool's stopped
states are failures. Everything else is ok, because the other tools
summarise a success in their own words (`a.txt (12 lines)`, `'q' (4
matches)`, `started PID 512`) and a rule that read an unknown word as a
failure would paint a working session red. That is the split the agent
loop already uses for its own `succeeded`. The `verify` line keeps its
word, since it is not a tool line, and takes the same classes.

The in-progress state is real rather than implied.
`Renderer::tool_starting(name, description)` is a new trait method with an
empty default, so the CLI and every test renderer are untouched.
`WebRenderer` turns it into a
`tool_started` frame carrying the same `name` and `phrase` the `tool` frame
for that call will carry, and the browser appends a running line on the
first and settles it in place on the second, matching on the name; tools
run one at a time, so one pending reference is enough. A reopened session
has every result, so a replayed line is never in progress. A turn that
ends with a line still pending fails it, with the title saying the turn
ended before the call reported.

**Why it is sent after approval:** `tool_starting` fires from
`Agent::run_tool`, which only the `Allow` branch and the approved `Ask`
branch reach, immediately before `Registry::dispatch`. Sent before
approval, the line would pulse under the approval card while the person
was still deciding, which reads as a call that ran without permission, and
a denied call would get a running line that nothing ever settles. A denied
call never started, so it gets only its result, as before. `agent.rs` has
a test for the order and a test for the denial.

**Ruled out:** keeping the word on the line in a smaller face (the line
was already three things wide and wrapped); marking any unknown status as
a failure (above); a second class scheme for the activity group's summary
(the group reads the three line classes and nothing else).
