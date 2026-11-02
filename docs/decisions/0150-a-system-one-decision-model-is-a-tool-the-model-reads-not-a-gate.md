---
status: accepted
date: 2026-11-02
---

# a System One decision model is a tool the model reads, not a gate code reads

**Decision:** System One decision models (Ollama's `POST /v1/systemone`) are
exposed as a built-in tool, `decide`, that the model calls and reads the
answers back from. They are not wired as a gate, a detector, or any other
place where code consumes the probabilities to make a decision. The client is
`zorp-agent/src/systemone.rs`, one non-streaming request through
`zorp::http_agent` and `zorp::send_json` with no new transport, under the
existing retry rule (only 429, 502 and 503 retry). The tool is gated behind a
non-default `systemone` feature, the way `web_search` is behind `search`,
because it is a networked built-in; the client itself is always compiled and
tested.

**Why:** the issue (#279) listed three placements, a tool, a gate, or a
standalone command, and one of them is ruled out by the rule the whole repo is
built on (ADR 0063, 0144): detection is code and a model only interprets, so a
probability a model returns is model-authored and nothing that decides may read
it as evidence. A gate that consumed System One's numbers would be exactly the
tail-eating that rule forbids: the agent's own output coming back as its
evidence. That leaves a tool or a command, and a tool is the more useful of the
two (the agent can classify or score mid-task) while staying on the safe side
of the rule: the answers go into the transcript as a tool result for the model
to read, and no detector, recall, memory or title path ever sees them. A
standalone command can come later without changing any of this.

**How the settings resolve:** the endpoint resolves like the other model
settings, `ZORP_SYSTEMONE_URL` then `ZORP_BASE_URL` then the saved config then
the loopback default, read from the environment and the saved file only and
never from a flavor, for the reason the search provider is read that way: a
workspace file the model can write must not move where a person's `state`
(which can hold their files) is sent. The decision model is named by
`ZORP_SYSTEMONE_MODEL` or the tool's `model` argument and has no default,
because a specific local GGUF a person pulled is the only thing that answers and
guessing one turns a missing setting into a 404. Decision models never appear in
the chat model picker, by construction: nothing adds them to it, and they
cannot answer a turn.

**Not negotiable:**

- The probabilities are model-authored and read only by the model. No gate, no
  detector, no evidence path consumes them.
- The endpoint is read from the environment and the saved file, never a flavor.
- A broken request, an unreachable endpoint and an unparseable reply are three
  distinct errors and none is reported as an empty or wrong set of answers.

**Still open (this PR is Part of #279, not Closes):** a live decision call
against a real Ollama model is not verified here, because the GGUF models are
multi-gigabyte and not present in this environment; the client is verified by
connection-level tests against a local server (a well-formed reply, a malformed
one, and a non-2xx status). A `zorp-eval harness` TOML case needs the harness
runner, `zorp-stub` and the case schema to serve a second endpoint beside the
chat provider, which is its own piece of work; until then the client's own
socket tests cover the request and response, including a malformed reply, one
level over. Image input for the Clef multimodal models (base64, shared across
questions, 32 MiB cap) is carried by the request type but not yet surfaced in
the browser or CLI.

See issue #279.
