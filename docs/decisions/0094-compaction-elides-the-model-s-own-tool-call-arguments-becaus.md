---
status: accepted
date: 2026-09-03
---

# compaction elides the model's own tool-call arguments, because that is where the bytes were

**Amended by 2026-09-09**, which adds a model-written summary as a second
stage. Everything below still runs, still runs first, and is still the
fallback when a summary cannot be had.

**Decision:** compaction now elides large string arguments from old assistant
tool calls after it has elided tool results. It leaves the call id, tool name,
paths, commands below the small-argument limit, and a marker with the byte
count. The marker says that the file on disk is the source of truth and that
`read_file` can retrieve it. The newest assistant turn stays whole.

**Why:** a 21-task Terminal-Bench run against local Qwen3.6-35B-A3B made 896
model calls. 461 prompts were above 64k tokens and the largest was 206k. One
task grew from 3k tokens at step 1 to 122k at step 60 without falling once.
The model wrote 150 to 290 KB of file content per task through 13 to 22 full
overwrites. That content was in assistant tool-call arguments, while the old
byte floor only counted tool-result bodies. With `ZORP_CONTEXT_TOKENS` unset,
the unknown-window rule left those arguments unbounded. Tasks took 25 to 50
minutes mostly in prefill, and one failed when the server refused a 196k-token
prompt.

**What it ruled out:** a model-written summary; dropping assistant messages;
and changing the stored transcript. The recorder has the original assistant
message before compaction rewrites its in-memory copy by index. The durable
record remains what was said.

**Amended the same day, after trial 3 of the second run:** a 5 KB `python -c`
command was elided, the model copied the marker back as its next command, and
the shell ran `[tool argument elided: ...]` three times before the repeat
detector stopped it. Two things changed. A `command` argument is never
elided, because a command is the one argument a model re-issues from its own
history. And the agent refuses any argument whose value is a marker before it
reaches a tool, with an error telling the model it copied a placeholder.

**Not changed:** the window remains unknown by default. Tool results still go
first. The 512 KiB floor still applies when no window is configured. The
2026-08-19 rules remain: no model summary, no store write from compaction, and
in-place rewrites only during a live run.
