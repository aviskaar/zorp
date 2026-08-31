# The web UI

A chat interface for the agent, with tool activity streamed as it
happens and an approval prompt before anything is written or run.

```bash
cargo run -p zorp-web    # http://127.0.0.1:7777
```

The server binds loopback by default. Binding anything else requires
`--token` and refuses to start without it, because a reachable
`zorp-web` is agent-driven shell access to whatever the process can see.

## Choosing a model

The gear button opens a settings panel: pick a provider preset, point it
at a base URL, and choose from the models that endpoint actually lists.
Ollama and oMLX are presets rather than special cases, since both serve
an OpenAI-compatible `/v1/models`.

A setting saved here beats the matching `ZORP_*` environment variable,
which beats the built-in default, and every field says which of the
three it came from. The API key is the exception to what gets saved: it
is held in memory for the life of the server process and never written
to disk. Set `ZORP_API_KEY` in the environment if you want it to survive
a restart.

## What you see

- **Streaming.** Text appears as the model produces it. Reasoning in
  `<think>` tags is recorded and not shown.
- **Files pane.** A read-only window on the workspace directory that
  renders markdown, office formats, PDFs, and images, and notices what a
  run wrote while it ran.
- **Session titles.** Once a session has a question and an answer, the
  model is asked once for a short name. The title is a label and nothing
  else; search and memory keep reading the verbatim first message.

## Optional features

Each of these is a compile-time feature, off by default, and each is
opted into on its own:

| Feature | Flag | What it adds |
|---|---|---|
| Web search | `--features search` | The `web_search` built-in tool (Tavily), the only built-in that touches the network |
| Conversation search | `--features recall` | Semantic search over your own conversations, embedded by a local Ollama model |
| Memory | `--features memory` | Recall quoted into a live turn, per message, opt-in per message |
| Voice | `--features voice` | Local Qwen3-ASR transcription into the composer |
| Research | `--features research` | The investigate endpoint and the aryabhatta ledger reader |

The recall, memory, and voice features share one rule: your data goes to
a loopback address or it goes nowhere. There is no remote provider and
no fallback. See [Recall and memory](concepts/memory.md).
