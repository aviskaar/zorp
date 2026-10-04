---
status: accepted
date: 2026-09-10
---

# the chat REPL grows its own line editor rather than taking one

**Decision:** `zorp-agent/src/line_editor.rs` is zorp's own cursor, motions,
history and completion, driven by the existing crossterm key loop. Neither
`reedline` nor `rustyline` was taken, and the reason is the same for both.

The input buffer here is not a `String`. It is a run of `Segment`s, and two
of the three kinds are not text: a bracketed paste is held whole and drawn
as `[pasted +2000 characters]` rather than dumped onto the prompt, and
Ctrl-V reads an image out of the clipboard. Both crates own the event loop
and hand back a `String` from `read_line`. A paste merged into that string
has lost its marker, and Ctrl-V has to be intercepted outside their loop,
which is to say outside the crate. The two things that are not standard
about this prompt are exactly the two things a line editing crate cannot
carry.

Neither is in `Cargo.lock` either, so taking one would also have been a new
dependency for a prompt that already had a loop.

**Multi line input is a trailing backslash and not Shift-Enter.** Most
terminals do not report Shift-Enter as distinct from Enter, so a backslash
is the thing that works everywhere rather than the thing that reads best in
a screenshot.

**History sits beside the other state files**, through the same
`trust::state_path` helper `ZORP_STATE_DB` and `ZORP_TRUST_FILE` use, capped
at 500 entries, written the way the trust file is: a temp file created owner
only, then renamed over. It records what somebody asked an agent about their
own machine, so it is never written and then chmod-ed. `ZORP_HISTORY=0`
turns it off.

**What it rules out:** a line editing dependency, and a full screen mode.
The 2026-09-10 entry on staying line oriented says why there are no panes; a
line editor is not a TUI and that entry says so itself. If the buffer ever
becomes plain text again, this decision is worth revisiting, because then
the crates would fit.
