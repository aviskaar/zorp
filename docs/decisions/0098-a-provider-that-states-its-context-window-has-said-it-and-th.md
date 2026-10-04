---
status: accepted
date: 2026-09-03
---

# a provider that states its context window has said it, and the turn is retried once after compaction

**Finding:** against a local Ollama model at its default 4096 token
`num_ctx`, a routine turn (list_files, then read_file on two 409 line
docs) failed with a 400 whose body was `exceed_context_size_error`,
`n_prompt_tokens` 8919, `n_ctx` 4096, nested and escaped inside an outer
error object. The browser showed "Something went wrong" over that raw JSON
and the turn was lost. Nothing compacted: `ZORP_CONTEXT_TOKENS` was unset,
so the window was unknown and the always-on 512 KiB byte floor was nowhere
near reached.

**Decision:** the window stays unknown by default. But a provider that
states its window in a refusal has said it, and that is the one number it
is safe to take. `context_window::stated_window` reads `n_ctx` and
`n_prompt_tokens` out of Ollama's error wherever they sit in the nesting,
and the sentences "exceeds the available context size (N tokens)" and
"maximum context length is N tokens" from llama.cpp and OpenAI-style
servers. When a model call fails that way and the budget's window is
unknown or larger than the stated one, the agent loop adopts the stated
window with the existing headroom, runs the same deterministic compaction
every other trigger uses, tells the renderer that the provider stated its
window and what compaction took, and sends the same step once more. Once.
A second refusal, or a compaction that elided nothing or left the estimate
over the target, ends the run with an error a person can read: the tokens
the turn needed, the window the model is serving, where to raise it (the
Ollama app under Settings > Context length, or `OLLAMA_CONTEXT_LENGTH` for
`ollama serve`), and that `ZORP_CONTEXT_TOKENS` should be set to the same
number so zorp compacts before the model refuses. The raw refusal goes to
stderr and the trace file, so the readable error hides nothing. The
browser gets it through the same `error` event and `textContent` path as
every other error, and the web renderer did not change.

**What it ruled out:** a default window, because none is right for
everyone and the 2026-08-19 entry says why. Probing an endpoint for its
window, because none can be asked. Sending `num_ctx` in the request,
because Ollama's OpenAI-compatible endpoint ignores request options and
this repo sends no provider-specific fields. More than one retry per step:
a loop here sends the same bytes until something else breaks, and a
compaction that could not make room the first time will not make it the
second. Retrying in the transport, where 429 and 503 are handled: the
transport cannot compact a transcript it does not own, and this retry only
makes sense after one.

**Not changed:** the window is unknown by default. Compaction is
deterministic, elides the same things in the same order, and no model
writes a summary. Nothing here writes to the store; what is sent shrinks
and what was said does not.
