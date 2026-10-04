---
status: accepted
date: 2026-09-12
---

# a Zorp mode run reports where it is, and a person can answer its checkpoints

**Decision:** three changes to the bolt, one display and two about who
decides.

A run now puts `investigate_progress` frames on the event stream as it
goes: `prereg`, `attempt-started`, `attempt-finished`, `write-up`,
`critique`, with the attempt number and the count the run will make. The
frames carry a phase name from a closed set and no prose. The words a
reader sees are chosen in `web/src/zorp-mode.ts`, because a phase is a
fact about the run and a sentence is a thing somebody reads, and a server
that sent prose would put a second copy of the page's voice somewhere no
test can see it.

Research checkpoints can be answered in the browser.
`interactive_checkpoints` on the start request builds a
`CheckpointMode::Interactive` over a `WebDecider`, which asks on the
stream the person is already watching. It is off by default and it is per
run: ticking the box is a promise to be watching for the next few
minutes, not a preference, so there is nowhere to save it.

And a run can be wound down rather than cancelled.
`POST /api/sessions/:id/investigate/stop-after` lets the attempt that is
running finish and be recorded, skips the ones that would have followed,
and still writes the track up. One way only: there is no route that clears
it, because a run told to wind down and then told to carry on is a run
whose attempt count nobody can state afterwards, and the attempt count is
part of what the evidence record means.

**What this reverses.** The 2026-08-21 entry says checkpoints are
auto-approved from the browser because there is no terminal to ask. The
premise was wrong rather than the reasoning: there is no terminal, but
there is a page, and the approval card had been asking about tool calls
on it for months. Auto-approve is still the default and still chosen
explicitly rather than fallen back to, so nothing that used to happen
stops happening. What is new is that a person can opt in, and the choice
is recorded either way, because `checkpoint_mode` is one of the conditions
every attempt writes and it now reads `interactive` when somebody was in
the loop.

**Nobody answering is not somebody saying no.** This is the part that
needed a change in `zorp-track`. A rejected checkpoint kills the track and
the rejection goes into the evidence record, so a browser that was closed,
or a run somebody stopped, must not come back as a no. `Decider` gains
`answered()`, defaulting to true so `TerminalDecider` behaves exactly as
it did, and `record_checkpoint` refuses with `CheckpointBlocked` before
writing anything when it is false. `Gate::abandon` is what a stop uses:
it releases the parked thread and leaves `answered` down. Without this,
closing a tab mid-run would kill a track and attach to it a decision
nobody made, which is the worst kind of row to find in an evidence record
because it is indistinguishable from a real one.

**Why the ledger rides on the frame instead of being fetched.**
`read_ledger` opens the project, a running attempt is holding that
DuckDB lock, and a second open deadlocks. So the run thread reads it
through the handle it already has and sends it with `attempt-finished`,
and `GET /api/investigate/ledger` answers 409 while any session is
running, with a sentence saying where the numbers are instead. The page's
Refresh button is hidden until the run ends for the same reason.

**What it rules out:** a model answering any of it. There is no tool that
resolves a checkpoint, none that ends a run, and none that starts one;
`agent.rs` carries the test. The reason is sharper here than for the
launch rule it extends: a model that could approve its own attempts would
be feeding the record it is later read against. It also rules out a
saved "always ask" setting, and a way to un-wind-down a run.

**What it does not change:** the pre-registered kill threshold is still
enforced in code, without consulting the checkpoint mode at all, so
neither mode can skip it. A killed track still gets no write-up. And the
ledger reader still names no model-authored text column.
