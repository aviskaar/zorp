---
status: accepted
date: 2026-08-21
---

# aryabhatta gets a producer, and forecasting is opt-in

**Decision:** `investigate` writes to aryabhatta. Every attempt records the
conditions it ran under, and, when `ZORP_FORECAST` is set, asks a separate
tool-less agent for a forecast and records that before the work begins.
Forecasting is off by default.

**Why:** the discovery layer was complete and inert. Nothing outside
`zorp-track`'s own tests had ever called `record_condition` or
`record_expectation`, so `conditions` and `expectations` were always empty. The
boredom detectors read an empty table, the confounding search had no graph, the
re-run gate had no expectation to gate, and the calibration report always
answered `NotEnoughEvidence`. Nine modules of readers and no writer. Steps 1
through 7 were built to the design and the thing still could not observe
anything.

**Both writes happen before the attempt runs, and that ordering is the whole
point.** A condition recorded afterwards describes a different run than the one
that happened. An expectation recorded afterwards is a postdiction, which is the
one thing `expectations` exists to refuse.

**The forecast is a separate agent with no tools and one step.**
`record_expectation` refusing a late forecast stops the database being lied to,
but it cannot tell a real forecast from a number the model produced in the same
breath as the result. Asking the working agent to report its expectation
alongside its answer would satisfy the guard and mean nothing. So the forecaster
runs first, sees the hypothesis and the metric name, and sees nothing about how
the work will be done. It gets no tools because a forecaster that can read the
repository can find last run's number and report that, which is measurement
dressed as prediction.

**Off by default.** A forecast costs a model call on every attempt, and the
prior art in the design says the calibration it feeds is more likely to fail its
own gate than pass it. Making every run pay for that unasked would be the wrong
default. Left off, the ledger stays empty, which is the honest state for a
record nobody has fed.

**A forecast never fails an attempt.** A malformed one is skipped with a warning
on stderr and the run continues. The work still happened and its outcome is
still worth recording; an experiment with no expectation is simply one the
calibration report does not score. Nothing substitutes a default interval,
because a forecast worth having is never worth inventing. The skip is said out
loud because silence would look exactly like a forecast that was made, and the
difference decides whether that experiment is ever scored.

**Conditions carry only what the harness observed.** The model name, from a new
`Model::identity`, and the checkpoint mode. Not the hypothesis, which is prose
and would put speculation on the observation side of integrity rule 5, and it is
the tempting one. Not the pre-registered metric name, kill threshold or
threshold direction: those cannot vary within a track by construction, so
recording them would make the invariant-condition detector fire on every track
forever while telling nobody anything. An endpoint configured without a model
name records no model condition rather than a blank one, because a blank string
would group unrelated runs together as though they shared a model.
