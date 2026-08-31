# The Kill Threshold

The Kill Threshold is zorp's core primitive: a number a human supplies
that says, in advance, what would prove the investigation wrong.

Before zorp gathers anything, the hypothesis, the metric, and the
threshold are written to a file, hashed, and committed to git. After
that:

- **The agent never proposes the threshold.** Only a human can set it,
  and only a human can move it.
- **A run cannot rewrite what it set out to test.** The registration is
  in git history, so an edit would show.
- **Crossing the line ends the run.** When an investigation crosses its
  threshold it is killed, and the record says why.
- **Every attempt is recorded**, not just the one that worked.

Nothing downstream is allowed to touch it either. The
[critique](critique.md) pass revises a draft against the evidence record
but refuses to run if the record moved under it, so it cannot move the
threshold or anything else that was pre-registered. Browser-launched
investigations auto-approve their checkpoints, but the pre-registered
kill threshold is still enforced in code regardless.

The point is falsifiability you cannot walk back. An investigation that
can quietly redefine success when the evidence turns against it is not
an investigation, it is a press release with extra steps.
