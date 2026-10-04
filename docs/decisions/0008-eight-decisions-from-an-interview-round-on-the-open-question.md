---
status: accepted
date: 2026-08-09
---

# eight decisions from an interview round on the open questions

A short interview to work through the open questions left in the
architecture proposal. Each is small enough to log together; none of
them has a full writeup beyond this entry.

**One binary, not two.** The four capabilities (validate, experiment,
co-write, find a venue) ship as new subcommands on `zorp-agent`, not a
separate `zorp-research` binary. Parallel experiment workers still run
as isolated subprocesses, but by having `zorp-agent` spawn more copies of
itself, not a second program. Keeps one thing to install and learn.

**Pre-registration is always required, not optional.** Every experiment
writes its hypothesis, metric, and a numeric kill threshold as its own
commit before any experiment code runs, the same discipline
lab-engine/Catalyst's idea triage already uses and for the same reason:
it stops the threshold from being quietly moved after seeing results.

**No hard experiment budget.** Catalyst caps experiments at 150 lines of
code, 10 minutes, no GPU. zorp ships sane default guidance but doesn't
hard-enforce it, since zorp is general-purpose and a cap tuned for
Catalyst's small validation experiments could be wrong for what zorp's
users actually run.

**Checkpoints are interactive by default.** The three research-loop
checkpoints (after validate, after experiment, before co-write finalizes)
default to asking a human, the same default `zorp-agent`'s existing
per-tool-call approval gate already uses. An explicit flag allows
unattended full-loop runs.

**Run record metrics are typed key-value pairs, not narrative logs.**
Every experiment attempt records explicit, named, typed values (for
example `accuracy: 0.87`) in DuckDB columns, alongside the free-form
logs. This is what lets the co-write claim-check compare a number the
draft cites against something structured, not an LLM's read of raw
stdout.

**Venue matching calls a live venue API, not a shipped catalog.** Confirmed
using huiban (the conference/journal database used to research zorp's own
venues earlier this session) as the model: query for current deadlines
and rankings rather than shipping and maintaining a dataset that goes
stale between releases.

**Multi-track from day one.** zorp supports multiple concurrent research
investigations from the start, closer to ORR's track model, rather than
one-at-a-time with multi-track added later. Chosen over the YAGNI
default specifically to avoid a data-model migration once real usage
exists.

**Venue matching runs on an abstract and contribution summary, not the
full paper.** Enough signal to match scope and contribution type, and it
can run as soon as co-write has a draft abstract, before the full paper
is finalized.
