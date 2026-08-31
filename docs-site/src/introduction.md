# Introduction

zorp is a research agent for scientific discovery. It turns an uncertain
question into a defensible answer, using evidence: question,
investigation, sources, evidence, conflicting evidence, reasoning,
validation, answer or artifact.

That covers more than academic research. A technical decision (should we
migrate off Kafka), a competitive teardown, an investment thesis, a
due-diligence package, a market question, and an academic hypothesis are
all the same shape of problem to zorp.

## Why

A confident answer is not a defensible one. An LLM will produce a fluent
answer to a hard question in seconds. What it will not do is tell you
whether to believe it, what evidence it weighed, or what it found that
pointed the other way. zorp treats that gap as the actual problem: a
question becomes an investigation, the investigation produces an evidence
record, and the record is what the answer is accountable to.

The core primitive is the [Kill Threshold](concepts/kill-threshold.md), a
number a human supplies that says, in advance, what would prove the
investigation wrong. It is written to a file, hashed, and committed to
git before any evidence is gathered, so a run cannot quietly rewrite what
it set out to test.

## What is here

- **Guide**: install zorp, run the agent, use the web UI.
- **Concepts**: the ideas the system is built on. Tracks, the four
  capabilities, the critique gate, the panel, the discovery layer.
- **Reference**: CLI, environment variables, the HTTP API.

## Status

zorp is early, pre-alpha software. The execution harness, the research
foundation, and all four capabilities (validate, investigate, co-write,
deliver) are built and tested. See the
[roadmap](https://github.com/aviskaar/zorp#status--roadmap) for what is
next.

Source: [github.com/aviskaar/zorp](https://github.com/aviskaar/zorp).
Built by [Aviskaar](https://github.com/aviskaar), an applied AI research
lab. MIT licensed.
