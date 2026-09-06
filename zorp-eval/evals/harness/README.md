# The deterministic harness suite

Each `.toml` file here is one case: a task for the real `zorp-agent` binary,
a script of provider replies, and what must be true when the run is over. No
network, no key, no model, so the only thing that can fail a case is the
code. The other half of `zorp-eval`, `compat`, spawns the agent against a
live provider; that answers a question about models and cannot gate a merge,
because a provider outage fails it for reasons that have nothing to do with
this repository.

Run them:

```
cargo build -p zorp-agent
cargo run -p zorp-eval -- harness \
  --cases zorp-eval/evals/harness \
  --agent-binary target/debug/zorp-agent
```

The runner exits non-zero if any case fails, which is what the `harness`
continuous integration job gates on.

## What belongs here

The layer below the `Model` trait: the HTTP client, the streaming parser, the
retry bound, the read timeout, and the store on disk once the process is
gone. `zorp-agent/src/agent.rs` already drives a scripted model through the
run loop in about eighty in-process tests, and those cover tool calls,
approval, compaction and termination far more cheaply than spawning a
process. A case that could have been one of those belongs there instead.

`docs/superpowers/specs/2026-09-05-harness-eval-catalogue.md` is the
catalogue of cases worth writing, and it says which ones are already proved
at a cheaper level.

## Writing a case

Every field is read by `zorp-eval/src/harness/case.rs`, and an unknown field
is an error rather than a silent skip. A misspelled expectation that gets
dropped quietly is a case that passes without checking anything.

```toml
name = "a tool call writes a file"      # optional, defaults to the file stem
about = "prose for whoever reads this"  # the runner never looks at it

[agent]
prompt = "write notes.txt"
# fixture = "some-dir"   copied into the workspace before the run
# env = { ZORP_MAX_STEPS = "2" }   applied after every inherited ZORP_ var is cleared

[[reply]]                                # the provider's replies, in order
[[reply.tool_call]]
name = "write_file"
arguments = { path = "notes.txt", content = "hi\n" }

[[reply]]
text = "Wrote it."

[expect]
exit = "success"
connections = 2
transcript_roles = ["system", "user", "assistant", "tool", "assistant"]

[[expect.file]]
path = "notes.txt"
contents = "hi\n"      # or contains = "hi", or absent = true
```

The last reply answers every further request, which is how "a provider that
always refuses" is written.

`[reply.transport]` is how the bytes reach the client, and it is the reason
this suite drives the binary rather than the library:

| `kind` | what happens | reads |
| --- | --- | --- |
| `ok` | a whole stream that says it finished | |
| `cut_off` | deltas, then the body ends with no `[DONE]` and no finish reason | `after` |
| `error_in_stream` | an error object delivered inside a 200 | `after`, `code`, `message`, `provider_name` |
| `stall` | the socket held open and silent | `after` |
| `status` | a status line and a JSON body, no stream | `code`, `retry_after`, `body` |
| `reset` | deltas, then the connection reset with no close handshake | `after` |
| `reset_before_headers` | the request read, the connection reset before a byte of reply | |

A field the kind does not read is an error. A `retry_after` on an
`error_in_stream` does nothing, and a case that thinks it does is a case that
is not testing what it says. A `stall` case must set its own
`ZORP_HTTP_TIMEOUT_SECS`; there is no default worth inheriting.

`connections` is the only way to tell a retry from a slow first send, so any
case about the retry bound states it.

## Before you commit a case

Break it on purpose and watch it fail. A case that passes against a mutated
expectation is checking nothing, and a green suite that checked nothing is
worse than no suite.
