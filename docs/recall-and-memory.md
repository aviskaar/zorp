# Recall and memory

Searching your own conversations, and quoting earlier ones into a new turn. Both run on this machine only.

## Searching your own conversations, on this machine

The browser sidebar can search everything you have ever asked zorp, by
meaning rather than by spelling, behind the `recall` feature:

```bash
ollama pull nomic-embed-text
cargo run -p zorp-web --features recall
```

The server indexes existing conversations after startup, checks them again
every five minutes, and indexes an active conversation after each turn. It
asks the local model for one vector per changed message and writes the vectors
to `recall.db` next to the session store. An unchanged conversation makes no
embedding call. `ZORP_RECALL_SWEEP_SECS` changes the full-store interval, and
0 disables startup and periodic sweeps.

**Conversation text goes to a loopback address or it goes nowhere.** There
is no remote embedding provider, no flag that adds one, and no fallback
when the local model is missing: if nothing answers on `127.0.0.1`, the
search box says so and searches nothing. This corpus is your whole history
with an agent that has been reading your files, and a feature that stayed
working by posting it to an API would be worse than one that stops.

Four things hold that up, and they are layered because any one of them
could be wrong. The endpoint has to be a loopback literal or `localhost`,
and it has to still resolve to loopback. The addresses it resolved to are
the only ones the HTTP client can reach, through a resolver that performs
no lookup of its own. Redirects are refused rather than followed. Proxy
detection from the environment is switched off, so `HTTP_PROXY` cannot
route the text through somebody else's server.

`ZORP_EMBED_URL` and `ZORP_EMBED_MODEL` override the endpoint and the
model, and `ZORP_RECALL_DB` overrides where the index goes. Naming a remote
host in `ZORP_EMBED_URL` does not get you a remote embedder; it gets you a
refusal that names the host.

## Remembering earlier conversations inside a new one

The `memory` feature turns the same index into something a turn can read,
so a fact from a thread you finished in March can be recalled in a thread
you started today:

```bash
cargo run -p zorp-web --features memory
```

Every finished turn indexes its own session in the background, and the
periodic sweep catches anything a failed feed missed. Tick **Recall earlier
conversations for this message** next to the composer and the server embeds
what you typed, finds the closest handful of messages, and quotes them into
the transcript the model reads. Above the answer you get a card listing
exactly what was recalled: the conversation, the date, and whether each line
was written by you or by the assistant.

Three things about it are deliberate.

**The box is unticked on every message.** Retrieval is not a mode you leave
on. It spends context, and it puts text from old conversations in front of
the model, so it is a decision you make per message and can see the result
of. The model cannot ask for a recall on its own; there is no tool for it.

**A memory is a quotation, never a summary.** Nothing reads your history
and writes down what it learned. There is no fact table, no profile, and no
stored sentence a model composed about your past, because that is the shape
in which an agent's guesses turn into its own evidence. What gets recalled
is a message somebody actually sent, with the conversation, the position,
the author and the date attached. Half of any conversation was written by
an assistant, and those lines are labelled as a model's earlier output
rather than presented as fact.

**Recalled text is data.** It arrives inside a fence whose marker is minted
for that one turn, so a payload sitting in an old conversation cannot close
the quotation and start giving orders, and it arrives under the same
sentence a skill body gets: it cannot grant a tool, widen an approval, or
bypass the command denylist. It is a `user` message and never the system
prompt, and it is never written back into your conversation store, which is
what stops the recalled block being re-embedded and recalled again.
