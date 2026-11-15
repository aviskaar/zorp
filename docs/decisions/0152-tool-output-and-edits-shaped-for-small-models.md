---
status: accepted
date: 2026-11-15
---

# Tool output and edits shaped for small local models

**Decision:** Five changes to the built-in tools, taken from comparing zorp
with the pi coding agent after a Qwen 3.6 turn in zorp launched a Streamlit
app where pi ran the pygame script the person meant.

- `read_file` pages a long file by whole lines and ends each page with
  "Showing lines A-B of N. Call read_file with start_line=B+1 to continue."
  A small file still comes back byte for byte.
- `cap_output`'s shared marker no longer names `read_file`, because
  `list_files` and `apply_patch` print it too.
- When a command's output outgrows the cap, the whole redacted stream is
  written to an owner-only file under `$TMPDIR/zorp-output/`, and the marker
  names it so the model reads the middle instead of running the command
  again. The file opens only once the cap is passed, so a short command
  writes nothing. It holds what the redactor already passed, never a raw
  secret. Files are left for the OS to clear.
- `apply_patch` tries an exact match first. With none, it matches whole lines
  ignoring trailing whitespace and typographic quotes, dashes and the
  no-break space, and replaces only the lines that matched. Leading
  whitespace still has to match, because indentation is meaning in Python and
  YAML, and two loose matches are refused as ambiguous.
- `run_command` takes `timeout_secs`, held between 1 and 600 seconds, and its
  description says that a server, watcher or windowed program goes to
  `start_background_process`. A tool may carry one `guideline()` line, and
  the agent appends the registered tools' lines to the system message once
  per run under "# Tool use".

**Why:** Each was a retry or a wrong turn for a small model: guessing the
next `start_line` after a cut mid-line, re-running a long build to see its
middle, failing an edit over a trailing space or a curly quote, and blocking
on a GUI program until the timeout killed it. Small models follow the system
prompt more closely than a tool schema.

**Not done:** pi's image reading, extensions and TUI. The stored system
message carries the "# Tool use" section, which is harmless because seeding
never replays a stored system message.
