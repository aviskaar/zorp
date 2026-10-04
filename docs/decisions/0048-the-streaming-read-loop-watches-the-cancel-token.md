---
status: accepted
date: 2026-08-19
---

# the streaming read loop watches the cancel token

**Decision:** `streaming::stream_sse` takes an optional cancel token and
checks it between reads, on both the event-stream path and the path where
an endpoint ignored `stream` and answered with a document. A raised token
abandons the response and returns an error. The agent, which owns the
token, reads it when a model call fails and reports `Outcome::Cancelled`
rather than `Outcome::Error`, so a deliberate stop is still not a
failure.

The token reaches the transport as a new argument on
`Model::complete_streaming`. Three signatures in the workspace define
that method and no test double overrides it, so this is a smaller change
than putting a cancel field on `HttpModel`, whose public struct literal
appears seventeen times including in the public-API compatibility test.

**Why:** the bound recorded above was measured against a real local model
and was not a bound worth accepting. Pressing stop twenty seconds into a
`qwen3.8:27b-mlx` answer took **303 seconds** to end the turn. For most
of that the browser showed only a spinner, because a thinking model's
output is withheld by `ThinkGate`, so the button said stop, the page said
running, and nothing visibly happened for five minutes. Waiting on a long
answer is exactly when somebody reaches for stop. The same press now ends
the turn in about one second.

**What happens to the half-arrived response:** it is dropped. Nothing is
pushed to the transcript and nothing is recorded. A response cut off
partway has text that stops mid-word and tool calls that may be half
parsed, and recording one would leave an assistant turn holding calls
that no tool result answers, which is a transcript the next turn has to
send back to the provider. The stopped turn therefore ends with the user
message and no assistant reply, and the next turn works normally.
Whatever had already streamed stays on the page for the reader; it just
does not become part of what the model is told it said.

**What this still does not cover:** the check sits between blocking
reads, not inside one, so a provider that accepts a request and then
sends nothing at all is still waited on. A model that is producing an
answer sends something several times a second, including while it is
reasoning, so this covers the case that occurs. Genuinely buffered
completions are not interruptible either: `Provider::Anthropic` and every
`complete()` caller go through `zorp::zorp_raw`, which is one blocking
request that reads the whole body, and making that interruptible means
restructuring a primitive shared with non-agent callers. For those the
run stops when the call returns, which is the behavior described in the
superseded entry above.
