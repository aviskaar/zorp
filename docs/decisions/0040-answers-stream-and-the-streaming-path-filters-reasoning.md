---
status: accepted
date: 2026-08-18
---

# answers stream, and the streaming path filters reasoning

**Decision:** `Model::complete_streaming` is the agent loop's model call.
It has a default body that runs the buffered path and reports the answer
as one delta, so every existing `Model` keeps working; only `HttpModel`
on an OpenAI-compatible provider genuinely streams. `Renderer` gains
`assistant_delta`, empty by default, so only the browser changes.
Anthropic and the CLI are unchanged on purpose.

**Why the default body matters:** it means there is one code path in the
agent loop rather than a streaming branch and a buffered branch that
drift. A provider that cannot stream is not a special case, it is a
`Model` that reports its answer in one piece.

**The part that is not obvious:** `extract_think_tags` strips
`<think>...</think>` out of content before anyone sees it, so a
qwen-family model's chain of thought never reaches the browser today.
Streaming raw content deltas would put it on screen, formatted as the
answer. `streaming::ThinkGate` filters it, and withholds text that could
still turn out to be a tag opening, because the tags arrive split across
chunks like anything else. The accumulator then rebuilds a
buffered-shaped response and hands it to `parse_assistant_completion`, so
streamed and buffered turns cannot produce different messages.

**What it rules out:** shortening the SSE backlog. Pruning fragments once
the finished answer arrived was tried, and `stream_events` holds an index
into that vector across polls, so it panicked the streaming task and
poisoned the session mutex. The backlog is append-only and the doc
comment on `record` says why.

**Also decided:** a provider that ignores `stream` and returns a whole
JSON body is handled rather than treated as an empty stream. Silence is a
worse answer than a slow one.
