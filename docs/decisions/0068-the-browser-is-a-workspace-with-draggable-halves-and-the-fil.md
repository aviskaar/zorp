---
status: accepted
date: 2026-08-22
---

# the browser is a workspace with draggable halves, and the file list is a picker

**Decision:** `zorp-web`'s page is now a resizable split. The artifact
pane defaults to `clamp(320px, 42vw, 760px)` instead of a fixed 420px, so
a document gets a real half of the window. Both side panes have a drag
handle on their inner edge that writes `--sidebar-w` or `--artifacts-w`
and saves the result to `localStorage`. The sessions sidebar collapses to
nothing and comes back from the topbar hamburger, and that survives a
reload too. The file listing left the artifact pane for a popover under
the existing Files button, and the pane's header now names the file it is
showing. The limits and the persistence live in `web/src/layout.ts`, apart
from `main.ts`, so they can be tested without a page.

**Why:** the pane was a strip down the side with a file list eating the
top third of it, and the third it ate was the part a document wanted. A
listing is a picker, and a picker belongs on the control that opens it.

**No pane may be dragged over the conversation.** Every clamp is computed
against `MAIN_MIN`, which the conversation keeps whatever the two side
panes are doing. A layout control that can squeeze the thing being read
down to a gutter is a way to break the page by accident and then not know
how to undo it.

**The handle answers the keyboard.** `role="separator"` with a tabindex,
arrow keys, Home and End, and Enter or a double click for the default
back. A resize only a pointer can do is a resize some people cannot do.

**Collapsing hides the sidebar, it does not remove it.** A grid item set
to `display: none` stops being an item, so the conversation auto-places
into the collapsed 0px column and vanishes with it. `visibility: hidden`
keeps the slot at zero width and still takes the sidebar out of the tab
order and the accessibility tree.

**Collapse is a wide-layout idea only.** Under 820px the sidebar is
already a drawer, so closing it there shuts the drawer and records
nothing; otherwise a phone user shutting a drawer would find their
sidebar gone on the next wide window.

**Nothing here assembles HTML.** Filenames in the popover and in the pane
header are workspace data, so they go through `textContent` like every
other thing on this page the agent had a hand in.
