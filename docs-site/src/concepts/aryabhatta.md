# aryabhatta, the discovery layer

aryabhatta is zorp's discovery layer: a record of what every
investigation attempt expected and what actually happened, plus readers
that look for structure in it. It is a record plus readers, not a fifth
capability, and it ships no CLI command on purpose.

## Who writes it

Only `investigate`. Every attempt records the conditions it ran under,
and, when `ZORP_FORECAST` is set, the agent is asked for a forecast
before doing the work and that is recorded too. Both happen before the
attempt runs. A condition recorded afterwards describes a different run,
and an expectation recorded afterwards is a postdiction, so the
`expectations` module refuses a forecast once its outcome exists. That
refusal is the one guarantee that separates a prediction from a
postdiction, and it has a mutation test because that test is the point.

Forecasting is off by default because it costs a model call on every
attempt. Left off, the ledger stays empty, which is the honest state for
a record nobody has fed.

## Two rules

Neither is negotiable:

- **Detection is code, and the model only interprets.** The same split
  [critique](critique.md) uses.
- **No detector, and nothing in the search layer, may read a column
  holding model-authored text.** Otherwise the agent's own speculation
  becomes tomorrow's observation.

## Calibration before anything else

`calibration` is a go/no-go for whoever builds on the ledger. It
compares stated forecast confidence against actual outcomes, band by
band. No code enforces the verdict; a person reads it and decides. If
the stated intervals do not have real coverage, the right move is to
stop and not build the anomaly ledger.

A band with too few forecasts to judge is its own no-go and never a
miss: a gap computed over three rows is arithmetic about three rows,
and reporting it as a demonstrated miss makes it look exactly like one.

## The modules

`conditions`, `expectations`, `calibration`, `detectors`, `partition`,
`rerun`, `anomalies`, `families`, and `inquiry`, all inside
`zorp-track`. The search layer can use `erbga`, a standalone genetic
algorithm for graph community detection, as its large-graph backend;
above the crossover a reported bundle is a floor on the confounding
rather than the whole of it, because the search can split a true bundle
but never invent one.

## In the browser

"Zorp mode" in the web UI is one `investigate` attempt plus a read of
what landed in the ledger. A run is launched by a person and never by a
model, and the ledger reader names no model-authored text column.
