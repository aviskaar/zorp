---
status: accepted
date: 2026-09-05
---

# the activity group and the settled approval card fold to one line

**Finding:** a long run's transcript is the answers, not the plumbing. A
turn that ran twenty tool calls put twenty lines above its answer, and
every approval it asked for stayed on the page with its whole arguments
block after it had been decided. Reading a session back meant scrolling
through the machinery to find what it produced.

**Decision:** both fold to one line, and both are native `details`
elements with no toggle code of their own. The run of consecutive activity
lines is one closed `details` (`web/src/activity-group.ts`) whose summary
reads "Working on it" and the phrase of the latest line while the turn is
still adding to it, and the count, "3 steps" or "3 steps, 1 failed", once
it is closed. The phrase is read off the line and written back through
`textContent` after the same clamp the line used. A group is closed in one
place, `closeActivityGroup` in `main.ts`, by whatever interrupts the run:
an answer, a card, a panel block, the end of the turn, a replay. The
approval card (`web/src/approval-card.ts`) is an open `details` while it
waits and refuses a click on its head, because a person must see what
they are deciding on; once settled it folds to its head, the outcome and
the tool name, with the arguments and the note one click under it. The
group's marker takes its colour from the lines' state classes, and a line
carrying none counts as ok. Appending a line never toggles a group, so a
reader who opened one keeps it open.

**What it ruled out:** a custom expand and collapse in code, when the
browser has one. Hiding the individual lines, which stay exactly as
`activity-line.ts` builds them and are one click away. Folding a card
before it is decided.

**Not changed:** what is stored or sent. This is the page's reading of
events it already had, and nothing about the event stream, the store, or
the tool line's phrase moved.
