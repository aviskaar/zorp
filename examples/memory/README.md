# Memory Example

Demonstrates how `zorp-agent` indexes conversations, searches them with
semantic recall, and loads relevant past text into a live turn.

## Requirements

```bash
# Ollama must be running with an embedding model loaded
ollama serve &
ollama pull nomic-embed-text
```

Both **recall** and **memory** features must be enabled for this example.

```bash
cargo build --manifest-path ../../Cargo.toml -p zorp-agent --features memory
```

## Usage

```bash
./run.sh
```

This walks you through the three steps:

1. **Indexing** -- bring the conversation index up to date with
   `zorp-agent recall --index`.
2. **Searching** -- run `zorp-agent recall "your query"` to see
   semantic matches across earlier conversations.
3. **Recalling in a turn** -- run a normal task with `--recall` so the
   agent can load past conversations into its context before answering.

## What each step shows

See `README.md` for detailed explanations of each scenario in run.sh.

### Step 1: Indexing

```bash
cd <workspace-that-has-conversations>
zorp-agent recall --index
```

This reads every conversation from the store, chunks the user and
assistant text, asks the embedder for a vector per chunk, and writes
them to the recall index. An unchanged conversation is skipped.

### Step 2: Searching conversations

```bash
zorp-agent recall "how did we configure the database?"
```

Searches the index by meaning, not by keyword. Returns the
conversation name, message position, and a snippet. Each result
carries a provenance line so you know who wrote it and when.

### Step 3: Recall during a turn

```bash
zorp-agent --recall "What database config did we discuss last week?"
```

The agent calls `memory::recall_for` before answering. If relevant
past text is found, it appears in the fenced recall block before the
agent's normal answer. The agent sees it as a user message under a
boundary sentence that says it is reference data, not instructions.

## Manual setup

```bash
# Build with memory feature
cargo build --manifest-path ../../Cargo.toml -p zorp-agent --features memory

# Index conversations in your workspace
cd my-project
zorp-agent recall --index

# Search
zorp-agent recall "my query"

# Recall into a turn
zorp-agent --recall "ask about the topic"
```
