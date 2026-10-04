---
status: accepted
date: 2026-08-14
---

# subcommands win over the bare-task positional

**Decision:** the CLI uses `subcommand_precedence_over_arg` instead of
`args_conflicts_with_subcommands`. A task whose first word is a
subcommand name is still reachable with `--`.

**Why:** with the old setting, any global flag before a subcommand made
the trailing task positional swallow the subcommand. `zorp-agent --yes
undo` did not undo anything: it sent the word "undo" to the model as a
task, ran an agent with auto-approval on, printed whatever came back,
and exited 0. Every subcommand was affected, and it failed silently in
the direction that looks like success, which is the worst way for a CLI
to be wrong.
