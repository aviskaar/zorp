# Bounded Task Example

Demonstrates how `zorp-agent` bounds a local task and gates every
action by the approval system.

https://github.com/user-attachments/assets/example-bounded

Zorp agents run with three safety layers you can tune:

1. **Step limit** (`--max-steps` or `ZORP_MAX_STEPS`). One step is one
   model call followed by tool execution. The agent stops after the
   limit, even if the task is unfinished.

2. **Approval mode** (`--approval <preset>` followed by `--yes` for
   auto-approve). The default preset is `read-only`: every edit and shell
   call prompts. `editor` allows file writes without asking. `full`
   allows everything except denylisted commands. Setting `--yes` turns
   interactive approval into auto-approve.

3. **Sandbox mode** (`--sandbox <mode>`). `off` (default) runs commands
   directly. `workspace` wraps each command under `srt`, which blocks
   network access and restricts file reads. `build` adds build-time
   restrictions. `read-only` blocks all writes.

## Requirements

```bash
# macOS: install required tools for sandbox mode
brew install sandbox-capture-run srt
```

`zorp-agent` must be built with the default features. No special flags
are needed for these examples.

## Usage

```bash
./run.sh
```

This will:

1. Build `zorp-agent` with the required features.
2. Create a fresh temporary workspace with a small Rust project.
3. Show interactive approval with a bounded task.
4. Show auto-approve mode.
5. Show sandbox workspace mode.
6. Show sandbox read-only mode.

## What each step shows

See `README.md` for detailed explanations of each scenario in run.sh.

### Interactive mode (default)

```bash
zorp-agent --max-steps 3 --approval read-only \
  "Read README.md and summarize it in three sentences"
```

The agent reads the file, writes a summary, and you approve each
action. With `--yes` the same flow runs silently.

### Auto-approve mode

```bash
zorp-agent --max-steps 3 --approval editor --yes \
  "Read README.md and summarize it in three sentences"
```

File writes run without asking. Shell commands still prompt.

### Sandbox workspace mode

```bash
zorp-agent --max-steps 3 --sandbox workspace \
  "Read README.md and write a summary file"
```

All commands run under `srt`. Writing inside the workspace succeeds; any
attempt to reach the network or leave the workspace is refused.

### Read-only sandbox

```bash
zorp-agent --max-steps 3 --sandbox read-only \
  "Read README.md and write a summary file"
```

Files cannot be written at all. The agent can still read and run
commands, but the denylisted write paths prevent any file change.

## Manual setup

All commands run from this directory:

```bash
# Build the agent
cargo build --manifest-path ../../Cargo.toml -p zorp-agent

# Interactive with sandbox read-only
./run.sh --sandbox read-only

# Auto-approve
./run.sh --auto-approve
```
