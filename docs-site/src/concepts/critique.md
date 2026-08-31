# critique, the evidence gate

`critique` audits co-write's draft against the track's own evidence
record and revises what the record does not support. It is a gate on the
artifact, not a fifth capability.

How it works:

- The audit is code. The model's only job is to inventory the claims in
  the draft; deciding whether the record supports each one is not left
  to a model's opinion of its own writing.
- Revision happens within a bound you set: `--critique-rounds`, or
  `ZORP_CRITIQUE_ROUNDS`, default 2.
- The pass refuses to run if the evidence record moved under it. That
  is what keeps it from touching the Kill Threshold or anything else
  that was pre-registered.
- What it found is written into the record.

The design stance is the same one the whole system takes: detection is
code, and the model only interprets. A model asked whether it likes its
own draft will like its own draft.
