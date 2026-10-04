---
status: accepted
date: 2026-08-19
---

# a run that wrote a file opens the pane showing it

**Decision:** when a turn produces a file, the artifact pane opens by
itself and shows the newest one. This reverses part two of the
2026-08-18 entry below, which had the pane stay shut and put a count on
the Files button instead.

**Why:** the earlier reasoning was that a pane appearing over a half-read
answer is an interruption. That holds for a run that happened to touch a
file on its way to answering something else. It does not hold for the
ordinary case, which is somebody asking for a document: they get a small
dot on a button, read it as nothing having happened, and the document
sits unread behind a click nobody knows to make. The model cannot open
the pane either, so it ends its turn telling the user to go and find the
file, which is the interface asking the user to do its job.

**What it rules out:** the badge. Nothing sets it once the pane always
opens, so `showArtifactBadge`, the `artifacts-badge` span and its styles
are gone rather than left as unreachable code. Somebody who wants the
pane out of the way closes it, and closing it once is a smaller cost
than never finding the file at all.
