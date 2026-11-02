# Network egress, per workflow

This report says, for each workflow that could touch the network, what it
connects to and whether that is expected, optional or prohibited. It builds on
the [threat model](THREAT_MODEL.md), which gives the design; this gives the
procedure to observe egress and the destinations observed.

It is honest about what was run here and what was not. Some workflows (the
loopback-guarded ones, and a purely local pass) were observed directly in this
environment and their raw results are below. Others (a real model provider, a
live web search) need a credential or a network this environment does not have;
for those the expected destination is stated from the code and the procedure to
reproduce it is given, and the row says plainly that it was not live-observed
here. No destination below is invented.

## Classification

- **expected** - the workflow is supposed to connect here when it is in use.
- **optional** - only when a feature or a server is configured on; off by
  default.
- **prohibited** - the workflow must never connect here, and there is a test
  that fails if it does.

## How egress is observed

Two methods, both used below.

1. **Connection-counting canaries.** `zorp-recall` and `zorp-voice` ship tests
   (`tests/no_remote.rs`, `tests/no_proxy.rs`) over the runtime channel (the
   loopback embedder, ASR and speech endpoints). For the off-device, redirect
   and proxy cases they stand up a canary listener and assert it received
   **zero** connections, which proves a request was not made rather than that
   it merely errored, the only honest way to show something did not leave: a
   failed request and a request never made look the same to the caller. Other
   cases in the same suites assert instead that an off-device URL is refused
   before any request, that the resolver answers only for the validated
   address, and that a valid request does reach the named local endpoint. These
   run in CI. They cover the runtime channel only, not the one-time setup
   downloads in the table below.

   ```
   cargo test -p zorp-recall --test no_remote --test no_proxy
   cargo test -p zorp-voice  --test no_remote --test no_proxy
   ```

2. **syscall tracing.** For a whole workflow, trace the `connect(2)` calls the
   process makes and read off the real destinations:

   ```
   strace -f -e trace=connect -qq -o out.strace <command>
   grep 'connect(' out.strace | grep -vE 'AF_UNIX|127\.0\.0\.1|::1'
   ```

   The second line leaves only off-device connections; empty output means none.

## The table

| Workflow | Destination | Class | Observed here |
|---|---|---|---|
| Local inference (loopback model) | the configured loopback endpoint (e.g. `127.0.0.1:11434`) | expected (loopback); off-device prohibited by config, not enforced | No; needs a running model. Procedure: strace a turn. |
| Conversation storage | none (local SQLite on disk) | expected: none | Yes: 0 `connect()` calls (below). |
| Recall indexing (embedder) | loopback Ollama embeddings endpoint | expected (loopback); off-device **prohibited** | Yes: canary tests, 0 off-device connections. |
| Memory retrieval | embedding lookup is loopback; the recalled text is then quoted into the chat request and goes wherever the model endpoint is | embedding expected (loopback); recalled text follows the model channel | Embedding: yes (same canaries). Model channel: see provider row. |
| Voice (ASR) transcription channel | loopback Qwen3-ASR runtime | expected (loopback); off-device **prohibited** | Yes: canary tests, 0 off-device connections. |
| Speech (TTS) channel | loopback Ollama `phonon` endpoint | expected (loopback); off-device **prohibited** | Yes: `zorp-voice` canary tests, 0 off-device connections. |
| Voice first-run setup | `pypi.org` / `files.pythonhosted.org` (`pip install qwen-asr[vllm]==0.0.6`) and `huggingface.co` (`huggingface_hub.snapshot_download` of the model) | optional and expected: a one-time download when a person enables voice; **not** prohibited | No; needs network and a non-root user (setup refuses root). Destinations from `zorp-voice/src/setup.rs`. |
| RL Studio / training setup | `pypi.org` (and `uv`'s own fetch) installing into `~/.zorp/training-env` via `uv`/`pip install` | optional and expected: a one-time download when a person sets up training; **not** prohibited | No; needs network and Apple Silicon. Destinations from `zorp-train/src/environment.rs`. |
| MCP | whatever a configured server points at (a local stdio process, or a remote SSE / HTTP host) | optional; a remote host is expected egress when that server is configured | No; depends on configuration. Procedure: strace a turn that calls the server. |
| Web search | `api.tavily.com` (tavily), or the configured SearXNG instance which forwards to upstream engines (searxng) | optional (the `search` feature, off by default); the destination is expected when it is on | No; needs a key or a SearXNG instance. Procedure: strace a search with a throwaway key. |
| Model provider calls (and titling, compaction, settings/doctor probes) | the configured model endpoint; loopback locally, or the hosted provider in external-provider operation | expected | No; needs a provider. Procedure: strace a turn, or watch the provider's own request log. |

## Raw results observed in this environment

### Loopback-guarded workflows: zero off-device connections

The canary suites passed, each asserting the off-device/redirect/proxy canary
received no connection:

```
zorp-recall  no_remote: 7 passed    no_proxy: 1 passed
zorp-voice   no_remote: 8 passed    no_proxy: 1 passed
```

This covers recall indexing, memory's embedding lookup, voice transcription and
speech synthesis. Across the suites the off-device, redirect and proxy cases
assert the canary received zero connections (proving the request was never
made), while the remaining cases assert that an off-device URL is refused
before any request, that the resolver answers only for the validated address,
and that a valid request does reach the one loopback host and port. Together
they show the runtime channel cannot be pointed off the machine.

### A purely local workflow: no egress at all

`zorp-agent dreams run` (a Dreaming-mode pass: reads the local store, writes
suggestions, no model call under the default phraser) traced under
`strace -f -e trace=connect`:

```
connect() syscalls observed: 0
off-device connections:      none
```

Conversation storage and a local Dreaming pass make no network connections.

## Gaps and what was not observed here

- **The model provider channel was not live-observed.** Local inference,
  model-provider calls, titling, compaction and the settings/doctor probes all
  go to the configured model endpoint, and this environment has no model
  provider configured, so the destination is stated from the code
  (`zorp::http_agent` to the configured base URL) rather than from a captured
  connection. The `strace` procedure above reproduces it against any endpoint.
- **Web search was not live-observed** for the same reason: no Tavily key and no
  SearXNG instance here. The destinations (`api.tavily.com`, or the configured
  SearXNG host and from there the upstream engines) are from the code
  (`zorp-search`), and a search still leaves the machine even with a local
  SearXNG, as the threat model says.
- **The loopback canaries cover the runtime channel, not setup.** Enabling
  voice for the first time downloads its runtime from PyPI and its model from
  HuggingFace, and setting up RL Studio installs packages from PyPI; those are
  expected one-time, off-device downloads, separate from the loopback-only
  transcription, speech and embedding channels the canaries prove. They were
  not live-observed here (no network, and voice setup refuses root); the
  destinations are from `zorp-voice/src/setup.rs` and
  `zorp-train/src/environment.rs`.
- **MCP depends on configuration**, so there is no single destination to
  observe; a remote server is an egress and should be treated as one.
- **Off-device prohibition for the loopback workflows is enforced and tested;
  for the model endpoint it is a configuration choice, not an enforced wall**:
  pointing the model base URL at a remote provider is how external-provider
  operation works, and nothing stops it, by design.
