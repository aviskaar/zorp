# Streaming responses in the web UI

**Status:** approved, implemented
**Date:** 2026-08-18

## The problem

The chat UI streams tool activity live but not the answer. `spawn_turn`
sends one `assistant` event carrying the finished text, because that is
all the agent has: `Model::complete_with_options` is a blocking POST that
returns only when the provider is done.

With a hosted model that is a couple of seconds. With `qwen3.8:27b-mlx`
on a laptop it is a minute or more of a spinner, then a wall of text. The
UI cannot fix this on its own. The text does not exist until the call
returns, so streaming has to start at the model layer.

## Scope

In: the OpenAI-compatible provider, which is Ollama, OpenAI, and most
local runtimes, including the setup this was reported on.

Out, deliberately:

- **Anthropic.** Its event protocol is different enough to be its own
  work. It falls back to the existing buffered path and behaves exactly
  as it does today.
- **The CLI.** The renderer hook defaults to doing nothing, so
  `LineRenderer` is untouched. The CLI streams the same way it always
  has, which is not at all.
- **Streaming reasoning to the browser.** Reasoning is captured, and it
  is deliberately *not* shown. See below.

## Design

### A new module, not a change to the tiny core

`zorp-agent/src/streaming.rs`. The `zorp` core crate stays a one-shot
JSON POST; per `CLAUDE.md`, new capability goes in a clearly named module
rather than into inherited harness code.

Two pieces, both pure and both testable without a socket:

`SseDecoder` turns a byte stream into complete `data:` payloads. It has
to be a decoder and not a line split because chunk boundaries fall
wherever the network puts them, including mid-line and mid-UTF-8.

`DeltaAccumulator` applies those payloads to a growing `AssistantMessage`
and returns the visible text delta, if any.

### Tool calls arrive in fragments

A streamed tool call is not one object. The name arrives in one chunk and
the arguments arrive as string fragments across many, keyed by `index`:

```
{"delta":{"tool_calls":[{"index":0,"id":"call_a","function":{"name":"read_file","arguments":"{\"pa"}}]}}
{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"th\":\"a.txt\"}"}}]}
```

The accumulator joins fragments per index and parses the arguments once
at the end, reusing `ToolCall::from_arguments_str` so malformed arguments
are reported exactly as the buffered path reports them. Getting this
wrong does not look like a streaming bug, it looks like the agent calling
tools with truncated arguments.

### `<think>` tags must not reach the browser

This is the part that makes a naive implementation actively worse than no
streaming. `extract_think_tags` strips `<think>...</think>` out of
content and files it under reasoning, so today the browser never sees it.
A stream of raw content deltas would show every user of a qwen-family
model the raw chain of thought, presented as the answer.

So the accumulator gates on think tags: text inside a think block is
routed to reasoning and never returned as a delta. The tag itself can be
split across chunks (`<thi` then `nk>`), so partial-tag text is withheld
until it is known not to be a tag opening.

The finished message still goes through `extract_think_tags`, so the
buffered and streamed paths produce the same `AssistantMessage`.

### The trait change is additive

```rust
fn complete_streaming(
    &self, messages, tools, options, on_delta: &mut dyn FnMut(&str),
) -> Result<ModelCompletion, BoxErr>
```

with a default body that calls `complete_with_options` and emits the
whole answer as a single delta. Every existing `Model`, including test
doubles and the Anthropic path, keeps working and keeps its behaviour.
Only `HttpModel` on an OpenAI-compatible provider overrides it.

`Renderer` gains `fn assistant_delta(&mut self, _chunk: &str) {}`,
default empty, for the same reason.

### The browser

A new `assistant_delta` event. The UI opens a bubble on the first delta
and re-renders it through the existing markdown renderer as text
arrives, throttled to animation frames so a fast local model does not
re-parse the document per token.

The final `assistant` event still carries the authoritative full text and
replaces the streamed content rather than appending to it. The streamed
text is an optimistic preview of a value the server will state exactly
once; treating it as authoritative is how a dropped delta becomes a
silently truncated answer.

## What implementation added to this design

Four things the design above did not anticipate, each found by running it
rather than by reading it:

**A provider can ignore `stream` and answer with a document.** Proxies,
gateways, mocks and older local runtimes do. Decoding that as an empty
event stream produces no error and no text, which is the worst failure
shape available. `stream_sse` now reports whether it actually streamed,
and a buffered reply is parsed the ordinary way and reported as one
delta.

**`ConfiguredHttpModel` needed its own delegation.** It wraps `HttpModel`
and `zorp-web` builds one, so inheriting the non-streaming default meant
the whole feature would have compiled, passed its own tests, and streamed
nothing in the product.

**Two boundaries in a turn, both easy to get wrong.** A turn is `working,
deltas, working_done, tool, working, deltas, working_done, assistant,
done`. Ending the streamed message on `tool` is required, or text either
side of a tool call merges into one bubble with the activity under both.
Ending it on `working_done` is wrong and looks right: that fires when the
model call returns, which is before the finished answer is sent, so it
closed the message and the answer then arrived again as a duplicate.
`endsStreamedMessage` is where that decision lives.

**The backlog must stay append-only.** Dropping fragments once the
finished answer arrived looked like free memory. `stream_events` holds an
index into that vector across polls, so shortening it panicked the
streaming task and poisoned the session mutex, taking every later request
with it.

## Testing

The accumulator and decoder are pure, so the interesting cases are unit
tests: tags split across chunks, tool-call fragments, keep-alive
comments, `[DONE]`, malformed payloads.

Above that, a stub server that writes a real SSE response in several
writes proves `HttpModel` asks for `stream: true` and assembles what
comes back.

The property worth guarding hardest is that a streamed turn and a
buffered turn of the same response produce the same `AssistantMessage`.
