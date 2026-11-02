# zorp threat model

This document says what zorp stores and where, what can leave the machine and
when, how credentials and approvals and the sandbox work, and what a model
provider or a local runtime can observe. It is written from the code as it is,
not as it is wished to be, and it names the gaps it knows about at the end.

zorp is pre-alpha. This is a working threat model, not a security audit, and no
third party has reviewed it. Read the "Known gaps" section as part of the
model, not as an afterthought.

## Three ways to run it, and what changes between them

The single most important thing to know is which of these you are in, because
almost every exposure below depends on it.

1. **Local only.** The default build (`zorp`, `zorp-agent`, `zorp-web` with
   default features) talking to a model endpoint on your own machine, with no
   `search` feature and no MCP servers configured. The only thing that leaves
   the machine is what you send to that local model endpoint, and nothing does
   if the endpoint is loopback.
2. **Opt-in network.** The same, plus one or more of: web search (the `search`
   feature, which is the only built-in that sends a query to a third party),
   conversation search / voice / memory (which embed or transcribe through a
   local runtime and are held to loopback), or a container image pull. In a
   from-source build each of these is a compile-time feature that is off by
   default and turned on deliberately. **The shipped macOS app (`Zorp.dmg`) is
   the exception: its bridge is built with every feature on**
   (`research`, `voice`, `recall`, `memory`, `search`, `train`), so a Mac-app
   install has web search and the rest compiled in by default and is in this
   mode, not "local only", from first launch. A feature being compiled in does
   not send anything on its own (web search still needs a key and a query, a
   turn still has to ask for memory), but a Mac-app reader should not assume
   case 1.
3. **External provider.** The model endpoint is a hosted provider (OpenAI,
   OpenRouter, Anthropic, and so on). Now the provider sees every message, tool
   result and file excerpt the agent puts in the transcript. This is the
   largest exposure and it is the common case; see "What a provider or a local
   model server can observe".

## What is stored, and where

`zorp-agent/src/state.rs` is the one list of what zorp keeps on the machine,
and `zorp-agent data`, `GET /api/data` and the browser's clearing actions all
read it. Nothing in that list is a workspace file, and nothing there can be
reached by a model (a test, `no_tool_clears_state_or_resets_settings`, says
so). The files:

- **Conversation store** (`sessions.db`, SQLite, moved by `ZORP_STATE_DB`).
  Every conversation both surfaces have had: the verbatim messages, the tool
  calls and their results, file-change records, and three side tables that are
  deliberately never read back to a model as evidence (the Dreaming-mode
  `suggestions`, the RL Studio `rl_work_log`, and the `compactions` summary
  table). It holds the whole of what you have typed and what the agent read on
  your behalf. It is the most sensitive file zorp writes.
- **Search index** (`recall.db`, moved by `ZORP_RECALL_DB`). Embeddings of past
  conversations for recall and memory. Derived from the conversation store, so
  it is as sensitive as that store, and it exists only if the `recall` feature
  was ever built and run.
- **Input history** (moved by `ZORP_HISTORY_FILE`, off with `ZORP_HISTORY=0`).
  What was typed at the chat prompt. Owner-only, capped.
- **Trusted agents** (the trust file, `ZORP_TRUST_FILE`). Content hashes of
  project flavors a person approved. Clearing it makes them untrusted again,
  not broken.
- **Settings** (moved by `ZORP_CONFIG`). The saved provider, base URL, model,
  max tokens, workspace path, text-to-speech model and voice, and the saved
  `sandbox` mode (the lowest-precedence source in the sandbox config chain
  below; the one security-relevant field here). **The API key is not here and
  there is no field for it** (`zorp-agent/src/config.rs`): the struct has
  nowhere to serialize a secret, on purpose.

Generated files (rendered PDFs, artifacts) go under `<workspace>/scratch`, a
directory the person chose. zorp never falls back to the directory the server
was started in.

## Who and what is trusted

- **The person** is the only actor that may enable a feature, approve a tool
  call, trust a flavor, pick an agent, start a panel or an investigate run, or
  clear state. No model tool does any of these; several named tests enforce it
  (`no_tool_starts_a_dream_pass`, `no_tool_trains_a_model_or_serves_a_checkpoint`,
  `no_tool_picks_an_agent_or_trusts_one`, and the panel/investigate guards).
- **The model and its provider** are untrusted for control. Everything a model
  emits is data: the browser renders it through `textContent`, never as HTML
  (`web/src/markdown.ts` and the paths around it), and recalled text, memory
  blocks, skill bodies and compaction summaries all reach a model inside a
  fenced, nonce-marked `user` message under a boundary sentence, never the
  system prompt, and grant no tool and loosen no approval. (A generated
  conversation title is drawn in the sidebar as text and is never fed back to
  any model; what the memory block quotes as a title is the verbatim first
  message, not a model-authored line.) Detectors and the
  search layer never read a column holding model-authored text, so the agent's
  own guesses cannot become its own evidence (ADR 0063).
- **MCP servers** are as trusted as you configure them to be. A configured
  server can be a local process (stdio) or a remote endpoint (SSE or
  streamable HTTP, `zorp-mcp/src/transport/`), and its tools run through the
  same approval path as any other tool. A remote MCP server is an egress point
  and a third party that sees whatever arguments a tool call carries.
- **Local runtimes** (Ollama for embeddings, chat and speech; the Qwen3-ASR
  runtime for voice) are trusted to run on loopback. A local SearXNG is **not**
  a loopback capability even though you host it: it forwards every query to the
  engines it aggregates, so a search still leaves the machine.
- **A workspace flavor or skill** is untrusted content, because the model can
  write into `<workspace>/.zorp/flavors/`. A flavor that wants privilege (a
  looser approval preset, verify commands, a base URL, a provider, a looser
  sandbox mode) applies those fields only once a person has trusted its content
  hash, and editing the file revokes that (ADR 0123). A skill body grants no
  tool, loosens no approval and bypasses no denylist, and its `allowed-tools`
  is parsed, warned about and ignored.

## What leaves the machine, and when

The outbound HTTP clients in the workspace, and what each is:

- **Model traffic** goes through `zorp::http_agent` (and the streaming path in
  `zorp-agent/src/streaming.rs`), to the configured endpoint. In local-only
  operation that is a loopback model server and nothing leaves. With an external
  provider, the full request (system prompt, transcript, tool results) goes to
  the provider. This is the main channel and it carries the most.
- **Web search** (`zorp-search`, the `search` feature, off by default) is the
  only built-in that sends anything to a third party by design. `tavily` sends
  the query and the key `ZORP_TAVILY_API_KEY` to Tavily; `searxng` sends the
  query to the configured SearXNG instance, which forwards it to upstream
  engines. The provider is chosen by `ZORP_SEARCH_PROVIDER` and never by a
  flavor file, so a workspace file the model can write cannot move where queries
  go. `research` does not enable `search`.
- **The embedding and transcription steps** of conversation search, voice and
  memory (`zorp-recall`, `zorp-voice`) send conversation text or recorded audio
  to a **loopback address or nowhere**. One shared guard (`zorp-loopback`:
  `LoopbackUrl::parse` plus `LoopbackResolver`) enforces it in four layers, each
  proven by a connection-counting canary test rather than by trusting an error:
  the endpoint must pass written-form and resolution checks, the HTTP client's
  only resolver answers for one host and port and does no lookup, redirects are
  off, and proxy-from-env is off. There is no remote embedding or ASR provider
  and no fallback when the local model is missing. **This loopback guarantee is
  about the embedder, not the whole `memory` feature:** once memory retrieves a
  past conversation (over loopback), it quotes that text into the chat model's
  request, so with an external provider the recalled text goes to the provider
  like the rest of the transcript. The containment is the embedding lookup; the
  recalled text then rides the normal model channel.
- **MCP** (`zorp-mcp`) reaches whatever a configured server points at; a remote
  server is a network egress.
- **Settings probe, model listing and the doctor probe.**
  `zorp-web/src/settings.rs`'s connection test and `zorp-agent doctor` each
  build their own short-bounded HTTP agent and probe the configured endpoint
  (doctor never prints a key). The CLI's interactive model picker in
  `zorp-agent/src/main.rs` lists a local Ollama's models (it runs only for a
  `localhost:11434` base URL) through the shared `zorp::http_agent`, not a
  diagnostic timeout.
- **Titling and compaction summaries.** Titling is on by default and fires
  from both surfaces: the logic is `zorp-agent/src/title.rs`, so a terminal
  conversation also sends a one-line model call (the first message and answer)
  to the configured endpoint. Compaction's stage-two summary
  (`zorp-agent/src/compaction.rs`, driven from the CLI `/compact` as well as
  the browser) likewise sends conversation text to the endpoint. The summary
  is never written back to `messages`; it reaches the model as a fenced
  stand-in only.
- **Container image pull** fetches the published image from a registry when you
  choose to run it.
- **The sandbox** denies the network by default; see below.

`zorp-eval` has its own network clients for benchmarking live models. It is a
developer harness binary (`cargo run -p zorp-eval`), not one of the three
binaries the release tarballs ship (`zorp`, `zorp-agent`, `zorp-web`), so an
end user does not run it unless they build and invoke it themselves.

## Credentials

The API key lives in `ZORP_API_KEY` in the process environment and in the
server's memory, and nowhere on disk. It is never written to the settings file
(there is no field), never printed by `zorp-agent doctor` (which reports a key
as set or not set, never its value, prefix or length, and a test greps the whole
report to prove it), and never put in a feature manifest or a smoke log.
Resetting settings cannot unset it, because a process does not own the
environment it was started in, and the reset action says so. On the macOS
desktop app the key lives in the login keychain and is handed to the in-process
server's memory at launch, never to the settings file. Configured MCP servers
can carry secret-bearing values; `zorp_mcp::ServerConfig::redacted` keeps those
out of `GET /api/mcp`.

## Approvals and the sandbox

- **Approvals.** Every tool call is gated by an approval policy and a hard
  denylist (`run_command`'s denylist reads POSIX shell). An approval preset can
  narrow what a surface asks about but never loosens it: `Decision::Allow` skips
  the approver, so the browser takes the stricter of its own floor and what a
  flavor asked for. `--yes` approves the trusted prompts a preset produces; the
  hard denylist always wins.
- **The srt sandbox** (`zorp-agent/src/sandbox/srt.rs`) can wrap every shell
  command so the OS enforces filesystem and network limits on the command
  itself, as a second wall behind the denylist. Modes: `off` (the default),
  `workspace` (writes only to the workspace and its scratch, reads allowed
  except credential stores like `~/.ssh` and `~/.aws`, no network), `build`
  (`workspace` plus a fixed network allowlist of crates.io, npm, PyPI and
  GitHub, generated in code and listed in `REGISTRY_DOMAINS`), and `read-only`
  (no writes, no network). The mode resolves through the config chain
  (`--sandbox`, then `ZORP_SANDBOX`, then a trusted flavor, then the saved
  setting, then off) and the settings are generated in code per run, never read
  from a file the model can write. A mode other than off refuses the command if
  `srt`, `ripgrep` or (on Linux) `bwrap` is missing, rather than silently
  running unsandboxed, and `zorp-agent doctor` and `GET /api/capabilities`
  report what is installed. The network is denied by default so the agent
  cannot reach zorp-web's own loopback server from inside the sandbox (ADR
  0053), and `build` keeps that true by allowing only the named registries.

## What a provider or a local model server can observe

Whatever model you point zorp at sees the full request on every turn: the
system prompt, the whole transcript including tool-call arguments and tool
results, and therefore the contents of any file the agent read or any web page
it fetched that ended up in a result. If the `memory` feature is on and a turn
asks for it, earlier conversations are quoted into that request too (held to
loopback for the embedding step, but the recalled text itself is then part of
what the chat model sees). Conversation titles are a separate one-line model
call over the first message and answer. With a loopback model server, "observe"
means only that process on your machine; with an external provider it means
that company, under its retention and logging, which zorp does not control.

The embedding, speech and ASR runtimes see only what is sent to them for those
tasks (message text to embed, answer text to speak, recorded audio to
transcribe), and only ever over loopback.

## Known gaps

- **The sandbox is off by default and not bundled.** Until you set a mode and
  install `srt` (and `ripgrep`, and `bwrap` on Linux), a tool call that the
  approval policy allows runs with your full user permissions. The macOS app
  asks you to install `srt`; nothing is shipped.
- **Prompt injection is mitigated, not eliminated.** Tool results, fetched web
  pages, recalled conversations, skill bodies and MCP tool output are treated as
  data (fenced, labelled, rendered as text, never granting tools), but a
  sufficiently clever payload that persuades the model to take an action you did
  not intend is still possible; the approval gate and the sandbox are the
  backstops, which is why running with a sandbox mode and real approvals matters
  against this.
- **A non-loopback bind widens the surface.** `zorp-web` normally binds
  loopback; binding elsewhere requires a token, and `GET /api/workspace/browse`
  exposes directory names (no file names or contents) to anyone who reaches the
  server, so a shared or exposed bind should be treated as granting shell-level
  access to the machine, because the agent runs commands on it.
- **SearXNG is not local privacy.** Hosting SearXNG keeps the key out of it but
  the query still leaves the machine to the upstream engines.
- **MCP servers and external providers are trust you extend.** A remote MCP
  server sees tool arguments; an external model provider sees everything in the
  transcript, under its own logging and retention. zorp cannot constrain what
  either does with that data.
- **The model can write a workspace flavor or skill.** The content-hash trust
  gate (ADR 0123) stops an untrusted flavor from gaining privilege, but a person
  who trusts a flavor without reading it has trusted whatever it asks for.
- **The shipped Mac app enables every feature by default.** `Zorp.dmg`'s
  bridge is built with all features on, so web search and the rest are compiled
  in from first launch. They still need a key, a query or a per-turn request to
  do anything, but a Mac-app install is not the "local only" default a
  from-source build is.
- **No formal audit.** This model is written from the code by its authors; it
  has not been independently reviewed, and the per-workflow egress report (#309)
  and the sandbox test matrix (#310) are the evidence that will back parts of it
  with real runs rather than reading.
