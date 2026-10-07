# Web UI

The browser chat interface served by `zorp-web`. For building the UI itself, see [`web/README.md`](../web/README.md).

A chat interface for the agent, with tool activity streamed as it happens and
an approval prompt before anything is written or run. A long run can stand
those prompts down for one chat with auto-approve, which says so in the
toolbar the whole time it is on and still cannot get a denylisted command
past the policy. See [`web/README.md`](../web/README.md).

```bash
cargo run -p zorp-web -- --workspace ~/research   # http://127.0.0.1:7777
```

The agent works in that directory, and its generated files go in
`scratch/` under it. There is no default: with no `--workspace`, no
`ZORP_WORKSPACE` and nothing saved from the browser, the server starts and
serves the UI but refuses to run a turn, rather than writing into whatever
directory it happened to be started in. You can also pick a directory in
the browser, which saves it for next time.

Or the whole thing in containers: the UI, the server, and an Ollama
sidecar that serves both the chat model and the embeddings the
conversation search needs.

```bash
ZORP_WEB_TOKEN=$(openssl rand -hex 16) docker compose up --build
# UI on http://localhost:8080, server on http://localhost:7777
docker compose exec ollama ollama pull qwen3:4b
```

The compose stack now pulls the default chat model and the embedding model for
recall on startup, so Docker comes up ready for both local chat and
conversation indexing. Point `ZORP_BASE_URL` and `ZORP_MODEL` somewhere else if
you want a different model endpoint.

The server binds loopback by default. Binding anything else requires
`--token` and refuses to start without it, because a reachable `zorp-web`
is agent-driven shell access to whatever the process can see. In the
compose file that mount is `./workspace`, so the agent sees that directory
and nothing else; set `ZORP_WORKSPACE` to point it elsewhere.

The UI container proxies the API onto its own origin and attaches the token
for you, so there is nothing to paste. Both published ports are bound to the
host's loopback for that reason: whatever reaches them is already talking to
the agent.

The image is built with voice and recall compiled in, so the microphone and
the conversation search work rather than answering 501. See
[`docs/docker.md`](docker.md) for what happens on the first microphone
click, and for why the sidecar shares the server's network namespace.

**Choosing a model.** The gear button in the top bar opens a settings
panel: pick a provider, point it at a base URL, and choose from the models
that endpoint actually lists. Ollama is a preset rather than a special
case, since it serves an OpenAI-compatible `/v1/models`:

```bash
ollama serve
cargo run -p zorp-web    # then pick "Ollama (local)" in the panel
```

Ollama serves every model with a 4k context window unless you raise it.
zorp never guesses a window, so with that default a long conversation is
refused by the model before zorp compacts anything. Raise it in the
Ollama app under Settings > Context length, or set `OLLAMA_CONTEXT_LENGTH`
for `ollama serve`, and start zorp with `ZORP_CONTEXT_TOKENS` set to the
same number so it compacts first. The first-run flow says the same thing
under the model list when Ollama is the provider.

A setting saved here beats the matching `ZORP_*` environment variable,
which beats the built-in default, and every field says which of the three
it came from. The API key is the exception to what gets saved: it is held
in memory for the life of the server process and never written to disk.
Set `ZORP_API_KEY` in the environment if you want it to survive a restart.

**Naming a conversation.** The sidebar used to show the first message you
typed, cut off wherever the row ran out, which is a wall of "hello". Once a
session has a question and an answer in it, the model you are already using
is asked for a short name for it, once, and the sidebar updates in place.
Set `ZORP_SESSION_TITLES=0` to turn it off and get the first message back.

A title is a label and nothing else. It is stored in its own column, and
the conversation search index and the memory block both keep reading the
verbatim first message, because a sentence a model wrote must not become
something a later turn is told to cite. Everything the model says is
clamped in code on the way to the column, to one short line, and a call
that fails or declines leaves the first message showing.

**Speaking into the composer.** Voice input is opt-in. It records in the
browser, asks a local Qwen3-ASR model for a transcript, and puts the text in
the composer for you to read and edit. It never sends the message for you.
Qwen3-ASR detects the language, so there is no English setting to choose.

Start zorp-web with voice enabled:

```bash
cargo run -p zorp-web --features voice
```

One microphone click starts readiness and asks for browser permission at the
same time. Recording begins as soon as permission is granted, and the composer
draws a live level meter from your microphone so you can see it listening while
setup is still running. If no runtime is
available, zorp creates a versioned virtual environment below the platform's
local data directory and tries `qwen-asr[vllm]==0.0.6`. When pip cannot resolve
that extra, zorp recreates only its marked environment with
`qwen-asr==0.0.6` and starts its embedded Transformers server. The page reports
the real create, install, download, load, and ready stages without inventing a
percentage. If recording finishes first, it waits for readiness before sending
the audio to the checked loopback endpoint.

Set `ZORP_VOICE_AUTOSTART=0` to disable every install and spawn step. In that
compatibility mode the status API still reports the old operator start command,
but the browser never renders shell text. Setup also refuses to run as root.
Transcription uses the existing OpenAI-compatible
`POST /v1/chat/completions` audio request.

The defaults are `http://127.0.0.1:8000` and
`Qwen/Qwen3-ASR-0.6B`. Override them with `ZORP_VOICE_URL` and
`ZORP_VOICE_MODEL`. A URL override still has to be a loopback address or
`localhost`. Recorded voice goes to that checked local address or nowhere.
The client pins its resolver to the checked host and port, refuses redirects,
and ignores proxy environment variables. There is no cloud ASR fallback.
For an HTTPS or path-prefixed URL, put `qwen-asr-serve` behind your own
loopback proxy. Automatic setup cannot bind that endpoint and leaves it to the
operator.

Design:
[`docs/superpowers/specs/2026-08-23-qwen3-asr-voice-input-design.md`](superpowers/specs/2026-08-23-qwen3-asr-voice-input-design.md).

**Watching the answer arrive.** Answers stream. Text appears as the model
produces it rather than after it finishes, which is the difference between
a spinner and a page on a local 27B model. Reasoning is filtered out on
the way: a model that thinks in `<think>` tags has that thinking recorded
and not shown, the same as in the terminal. Providers that cannot stream,
which includes Anthropic today, still answer exactly as they did before.

**Reading what a run produced.** The Files button opens a pane listing the
files in the workspace, and renders them. It is read-only. Paths are
resolved against the workspace and refused if they
land outside it, and only an allowlist of extensions is served at all, so
this is a window on the workspace rather than a file server.

| Format | Shown as |
|---|---|
| `.md`, `.markdown` | Rendered markdown |
| `.txt`, `.json`, `.csv` | Plain text |
| `.docx`, `.odt` | Extracted to markdown: headings, paragraphs, lists, tables |
| `.xlsx` | One markdown table per sheet |
| `.pptx` | One heading per slide, plus that slide's text |
| `.pdf` | Its text, extracted to markdown |
| `.png`, `.jpg`, `.gif`, `.webp` | Inline image |
| `.svg`, `.html` | Inside a sandboxed iframe |

The office formats and PDFs are read on the server and rendered by the same
markdown renderer the chat uses. The reading is deliberately plain: text
structure comes across, and images, fonts, colours and page layout do not.
It is for reading what a run produced, not for rendering a document. A PDF
gives up more than the rest, because it records where each glyph was drawn
rather than what the document said, so what comes back is the words and the
breaks between them and no headings at all. A scanned PDF holds pictures of
words and no words, and the pane says so instead of showing nothing.

`.svg` and `.html` are the two that can execute. They load into the pane's
iframe by URL and never into the page, because every served file carries
`X-Content-Type-Options: nosniff` and a bare
`Content-Security-Policy: sandbox`. That is a unique origin with scripting
off, so script inside one of these neither runs nor reaches the page that
framed it.

**Noticing what a run produced.** The pane no longer waits to be asked. The
browser takes a snapshot of the file listing when a turn starts and compares
it afterwards, so anything the run wrote or rewrote gets marked. With the
pane open the newest one opens in it; with the pane closed the Files button
gets a count, and nothing appears over what you are reading. This works by
diffing the directory rather than by reading tool output, so a PDF that
pandoc wrote under `run_command` is caught exactly like one `write_file`
wrote.

Design and plan:
[`docs/superpowers/specs/2026-08-17-zorp-web-ui-design.md`](superpowers/specs/2026-08-17-zorp-web-ui-design.md),
[`docs/superpowers/specs/2026-08-17-artifact-pane-design.md`](superpowers/specs/2026-08-17-artifact-pane-design.md).
