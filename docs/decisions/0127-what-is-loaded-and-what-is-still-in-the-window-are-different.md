---
status: accepted
date: 2026-09-13
---

# what is loaded and what is still in the window are different questions

**Decision:** `GET /api/sessions/:id/skills/active` reports which skills'
instructions are in the transcript a turn would send, next to `/api/skills`
which reports what is on disk. `zorp-agent/src/active_skills.rs` computes
it and the route is an adapter over `seed_transcript`.

**Why the activity line was not already the answer.** A skill body is a
tool result body. `plan_seed` drops the oldest exchanges whole and then
elides the oldest tool result bodies, and the activity line is rebuilt from
the store rather than from the window. So a skill loaded twenty turns ago
shows in the line as loaded while its instructions left the request long
ago. On a short conversation the line is right; on a long one it is
confidently wrong, which is worse than silent.

**The answer is the plan, not a model of the plan.** The route calls the
same `seed_transcript` with the same system prompt and the same
`ContextBudget::from_env()` that `start_turn` does, then reads what came
back. Nothing is asked of a model and nothing is inferred from anything a
model wrote. If the two ever disagree, the plan is right and the report is
wrong, which is the correct direction for a report to fail in.

**Four states, not two.** The issue asked for present against elided.
Building it found that elision is not even the common fate: over budget,
`plan_seed` drops whole exchanges before it elides anything, so an old
skill load usually leaves the request entirely. `dropped` and `elided` are
therefore both real and separately observable, and `unrecorded` covers a
turn killed between writing a call and writing its result, which
`repair_tool_calls` already exists to paper over. Each has a test that goes
through the real planner rather than a hand-built transcript.

**The name comes from zorp's text, never the model's.** A `skill` call's
argument is a string the model chose, and a call naming a skill that does
not exist returns an error rather than instructions. So a load is
recognised by the `# Skill: ` header `Skill::instructions` writes onto every
successful body, read out of the durable record where it is never elided. A
hallucinated name produces no row, and a test pins it. Reading the name off
the call arguments would have put whatever the model typed into the list a
person reads to find out what is influencing the model.

**Still read-only, and the body still never crosses.** Nothing here loads,
unloads or reorders a skill; loading stays the agent's `skill` tool. The
report is assembled from state that already exists and never re-injects a
body in order to describe it. The body itself is not in the response at
all, which has its own test, because a `SKILL.md` is a file zorp did not
write. A skill loaded and since uninstalled keeps its row with a null
scope, since instructions from a file that is no longer on disk are exactly
what somebody needs to be able to see.
