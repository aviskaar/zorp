# Tracks and pre-registration

A track is zorp's unit of investigation: one question, one evidence
record, one history. The `zorp-track` crate is the research foundation
everything else builds on.

## What a track holds

- **The registration.** The hypothesis, the metric, and the Kill
  Threshold, written before any evidence is gathered.
- **The evidence record.** Every attempt, not just the one that worked.
  Each attempt records what was tried, what came back, and the
  conditions it ran under.
- **Checkpoints.** Points where a human decides whether the
  investigation continues.

## Why git

The registration is written to a file, hashed, and committed to git.
That makes pre-registration tamper-evident: a run cannot quietly rewrite
what it set out to test, because the record of what it set out to test
is in history that would show the edit.

## Storage

Track data lives in DuckDB. A LanceDB vector library sits behind a
non-default `library` feature for retrieval work, and the `research`
feature does not pull it in.

## Where to read more

The design specs live in
[`docs/superpowers/specs/`](https://github.com/aviskaar/zorp/tree/main/docs/superpowers/specs)
in the repository, one per capability.
