---
status: accepted
date: 2026-08-18
---

# one system prompt, and it says zorp is a research agent

**Decision:** `zorp_agent::DEFAULT_SYSTEM_PROMPT` is the single default
system prompt for every zorp surface. The CLI and `zorp-web` both use it.
It positions zorp as a research agent, asks the model to ground claims in
what it actually checked, tells it to say so when it cannot, and says the
question does not have to be about code.

**Why:** the two surfaces had written their own. The CLI carried quecto's
"You are zorp-agent, a helpful coding assistant" unchanged through the
rename. `zorp-web` said "You are zorp, a careful assistant", which is
vague enough that the model fills in the rest itself: asked "zorp", a
local model introduced itself to a user as their "coding buddy". Neither
string matched the README, zorp.dev, or the four research capabilities.

The positioning is not decoration. A prompt that says "coding assistant"
is asking for a different behaviour than one that says claims must be
grounded and counter-evidence reported, and that behaviour is the
product.

**What it rules out:** a surface keeping its own copy. That is the
mechanism that produced two wrong answers here, so `zorp-web` gets a test
asserting its prompt is the shared constant, not merely a similar one.

**What it does not change:** overriding is still supported and still
cheap. `ZORP_SYSTEM` in the environment and `system_prompt` in a flavor
both win over the default, so a coding-focused flavor is a config file,
not a fork. `validate`'s preamble still narrows the frame further for
that one subcommand.
