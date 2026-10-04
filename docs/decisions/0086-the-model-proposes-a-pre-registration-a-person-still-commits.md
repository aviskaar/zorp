---
status: accepted
date: 2026-09-01
---

# the model proposes a pre-registration, a person still commits it, and an unsure model asks

**Decision:** Zorp mode is a lightning bolt in the composer. The question
is whatever a person typed there, and the metric, the kill threshold and
the direction that kills are read out of it by a model rather than typed
into a form first. The toolbar button and the question textarea are gone.
The form stays, and now opens only as an escalation.

**Why:** the form was the reason the feature went unused. A person with a
question has not yet decided what would count as answering it, and being
made to commit to a number before starting is the hard part of the work,
not the easy part. What was being asked for up front is exactly what the
investigation is for.

**Why this does not break the record.** The pre-registration is a
commitment made before the work, and it still is. What
`prereg_infer::infer` returns goes into the same `PreregParams` a typed
form produced, on the same path, and `investigate::run` records it before
the first step runs. Three things bound it and none of them is the
prompt:

- **It only ever proposes where there is nothing.** The person's typed
  fields win, then the track's own recorded pre-registration, and the
  model is asked only when there is neither. So nothing here can revise a
  commitment that exists: a second attempt uses the record and a mismatch
  is refused exactly as before. An inferred threshold is a prediction and
  never a postdiction, which is the property the whole record is built to
  keep.
- **An unsure model hands back to the person.** `MIN_CONFIDENCE` is 0.7
  and it is a floor, not a tiebreak. Below it, and on an unparseable
  answer, a missing field, a non-finite threshold, an unknown direction, a
  metric name nothing survives from, and an unreachable or unconfigured
  model, `clamp` returns `None`. That becomes `PreregRequired`, which
  becomes the form. The asymmetry is the point: a person sent to the form
  is where they already were, and a committed threshold the model had to
  invent is worse than no attempt because it looks like evidence
  afterwards. The system prompt says so too, in as many words, so a model
  is not pushed toward confidence it does not have.
- **The question is untrusted.** It is composer text and can be pasted
  from anywhere, so it is fenced under a boundary sentence with a per-call
  marker, the shape `zorp-skill`, `memory` and `title` all use, and
  everything coming back is clamped in code. A metric name in particular
  is scrubbed to `[a-z0-9_]` and capped, because it is written into a
  pre-registration and compared for equality on every later attempt: one
  carrying a space or a bidirectional override would lock the track out of
  every run that tried to match it by hand.

**Ruled out:** letting the page recognise the escalation from the wording
of the error. `InvestigateDone` carries a `needs_prereg` flag instead. The
escalation is the one error the page acts on rather than only displays, so
two copies of a sentence drifting apart would fail by silently never
opening the form.

Also ruled out: a model starting an attempt. The bolt is a click and there
is still no tool that reaches it, for the reason 2026-08-21 gives. Letting
a model propose the metric does not loosen that; the thing a model must
not do is decide that an attempt happens and feed the record it is later
read against.

**Still ahead, and this feature is thinner without it.** The 2026-09-01
entry below found the anomaly ledger has no producer. Iteration that never
admits an anomaly is a loop, so the "systematic" half of this is waiting on
a caller for `rerun_gate`, and the artifact half on `deliver`. See #140.
