---
status: accepted
date: 2026-09-05
---

# the file list is folders, not paths

**Decision:** the Files pane groups the workspace listing by directory.
`web/src/artifact-tree.ts` does the grouping and builds a native
`details` per folder; `main.ts` still builds the file row, so the click,
the open mark and the "new" badge did not move. A row shows the leaf
name and carries the whole path in its `title` and its `data-path`. The
top level is open and everything under it is shut, unless a folder holds
the file the pane is showing or a file the turn just wrote.

**Why:** a run that wrote 351 files put 349 of them in one directory,
and the pane drew all 351 as full paths, one per line. Every row began
with the same prefix, the part that told them apart was the part the
pane elided, and the folder they shared was never said out loud. The
prefix belongs on one line above the rows, not repeated down the column.

**Top level open, deeper shut.** Those are two different problems. A
workspace with one directory in it would open to a single row naming
that directory, and a pane that shows nothing until you click is not an
improvement on one that shows too much. The hundreds of files are one
step down, and that is the step worth folding. Nothing here is tuned to
a count, because a rule that opens a folder when it is small enough
turns a listing into an argument about what small is.

**Native `details`, the same choice `activity-group.ts` made.** It
collapses with no state of ours, the keyboard already reaches it, and
the browser knows what to do with it.

**A folder name is model-written text.** It is part of a path the agent
chose, so it reaches the page through `textContent` like everything else
in `web/`, and the first test in `artifact-tree.test.ts` is the
injection case. Nothing in that module assembles markup.
