# Recall and memory

Two features on one index. `recall` gives the web UI's sidebar a
semantic search over everything you have ever asked zorp. `memory`
turns that same index into something a live turn can read, so a fact
from a thread you finished in March can be recalled in a thread you
started today.

## The loopback rule

**Conversation text goes to a loopback address or it goes nowhere.**
There is no remote embedding provider, no flag that adds one, and no
fallback when the local model is missing. This corpus is your whole
history with an agent that has been reading your files, and a feature
that stayed working by posting it to an API would be worse than one
that stops.

Four layers hold that up, because any one of them could be wrong. The
endpoint has to be a loopback literal or `localhost`, and it has to
still resolve to loopback. The addresses it resolved to are the only
ones the HTTP client can reach, through a resolver that performs no
lookup of its own. Redirects are refused. Proxy detection from the
environment is off, so `HTTP_PROXY` cannot route the text through
somebody else's server. The tests for all of this count connections to
a loopback canary rather than checking for an error, because a failed
request and a request never made look the same from the caller's side.

## How memory stays honest

- **The box is unticked on every message.** Retrieval spends context
  and puts old text in front of the model, so it is a per-message
  decision. The model cannot ask for a recall; there is no tool for it.
- **A memory is a quotation, never a summary.** Nothing reads your
  history and writes down what it learned. There is no fact table and
  no stored sentence a model composed about your past, because that is
  the shape in which an agent's guesses turn into its own evidence.
  Assistant-written lines are labelled as a model's earlier output.
- **Recalled text is data.** It arrives inside a fence whose marker is
  minted for that one turn, under the same boundary sentence a skill
  body gets. It grants no tool, widens no approval, bypasses no
  denylist, and is never written back into the store, which is what
  stops the recalled block being re-embedded and recalled again.

## Setup

```bash
ollama pull nomic-embed-text
cargo run -p zorp-web --features memory   # memory turns recall on too
```

The server indexes existing conversations after startup, sweeps every
five minutes (`ZORP_RECALL_SWEEP_SECS`, 0 disables), and indexes an
active conversation after each turn.
