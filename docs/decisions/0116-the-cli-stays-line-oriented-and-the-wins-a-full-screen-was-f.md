---
status: accepted
date: 2026-09-10
---

# the CLI stays line oriented, and the wins a full screen was for are line oriented too

**Decision:** `zorp-agent chat` stays a line oriented REPL. No alternate
screen, no panes, no widget framework, and no `ratatui` in the workspace.
Every improvement a full screen version was supposed to buy is delivered as
better line output instead, and the ones that are not yet built get their
own issues in that shape.

**Why this was asked.** Several CLI issues opened at once, and most of them
name something the browser has and the terminal does not: a session list, a
foldable run of tool lines, a view of what a turn changed, a readable
approval, a context meter. Read together they look like an argument for a
terminal interface with panes. They are not, and working through them one at
a time is what settles it.

**The five wins, each taken seriously.**

*A session picker you arrow through.* This is already done and it is a
printed list. `zorp-agent sessions` prints a short id, a relative time, a
status and a name per line, and `resume` takes a unique id prefix the way
git takes a short sha. A picker you arrow through is nicer for about two
seconds and then it is worse, because a printed list is still on the screen
after you have chosen, it can be piped into `grep`, and the id you copied
out of it works in the next command you type. **Line output, and it shipped
that way.**

*Tool activity as a live tree that folds.* The browser folds a run of tool
lines under a `details` because a page has a fixed viewport and the lines
would push the answer off it. A terminal has scrollback instead, and the
lines are the record of what the agent did to the machine. A pane that folds
them away by default is hiding the thing a person most needs to have seen.
What is worth borrowing is the summary line the browser's `summary` carries,
printed once at the end of a run of calls, so a long turn reads as work
followed by an answer rather than as forty lines. **Line output, and it
needs a summary line rather than a tree.**

*A files pane showing what this turn changed.* `/diff` prints it on demand
today, from `render_change_summary`. Always-on would mean redrawing on every
tool result, which in a line oriented program means reprinting, which is
worse than asking. What is missing is not a pane, it is that a turn which
changed files does not say so when it finishes. One line naming the count,
with `/diff` for the detail, is the whole of it. **Line output.**

*An approval card you can read the whole command in.* This one is a real
defect and it is not a layout problem. `approval_prompt` caps the command at
120 bytes and asks `[y/N]` on one line, so a person can be asked to approve
a command they cannot see the end of. That is a security-relevant truncation
and the fix is to print the untruncated command above the question, which
needs no pane at all. The other half worth having is a third answer:
approving this shape of call for the rest of the session, which the browser
has as its standing approval and the terminal has only as `--yes` for
everything. **Line output, and it is the most important item on this list.**

*A context meter that is always visible.* `/context` prints it on demand. A
meter on the prompt line is a few characters and does not need a frame. It
does need the prompt to be drawn by something that can hold state, which is
what the line editor work brings anyway. **Line output.**

**What a full screen would have cost, stated rather than listed.**

Piping is not a fallback path here, it is a first class one:
`zorp-agent "<task>"` in a script and in CI is how the one-shot is used, and
a program with two interfaces has two of everything to keep working. That is
not hypothetical: this repository already has one renderer trait with a
terminal implementation and a browser implementation, and keeping those two
honest is a standing cost. A third would be a third.

Scrollback is the terminal's own memory and an alternate screen gives it
back when it exits. Everything the agent printed, every command it ran and
every result it got, would be gone at the end of the session unless a pane
reimplemented scrolling, searching and selecting. That is a lot of code
before it is as good as the thing it replaced, and it is worse in the
meantime.

Selection and copying get worse in exactly the case people need them:
copying an error out of a tool result to paste somewhere. Across a pane
boundary a mouse selection takes the borders too, and turning mouse capture
on to fix that breaks the terminal's own selection.

And a line oriented program is readable by a screen reader without anybody
doing anything. A full screen one is not, without deliberate and ongoing
work that nobody here has committed to.

**The comparison that decided it.** Claude Code is what this repository's
users compare zorp to, and it is line oriented on purpose. Being different
here would need a reason better than "the browser has panes", and the
five items above say the browser has panes because a browser has a fixed
viewport, which a terminal does not.

**What this rules out:** `ratatui`, an alternate screen, mouse capture, and
any layout that owns the whole terminal. If one of those starts to look
necessary, it is a new decision and it needs its own entry, and that entry
has to say what happens to piping, to scrollback and to the non-tty path,
because those are the three things that get discovered afterwards otherwise.

**What it does not rule out:** the line editor in #218, which enters raw
mode for the input line only and leaves it around a turn, exactly as the
current hand-rolled loop does. A line editor is not a TUI. Nor does it rule
out colour, a spinner, or a status line on the prompt, all of which the
program already has or can have without owning the screen.

**Follow-on work this identified**, each its own issue rather than buried
here: the untruncated approval prompt with a standing per-shape answer, the
summary line at the end of a run of tool calls, the line naming changed
files when a turn ends, and the context meter on the prompt.
