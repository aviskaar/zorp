---
status: accepted
date: 2026-09-05
---

# a chat branches at an answer by copying the stored messages up to it into a new session

**Finding:** the only way to try a different path from the middle of a
conversation was to start a new chat and paste the context back in. What
a person wants is the conversation as it stood after one particular
answer, kept as it was, with the rest going on somewhere else.

**Decision:** `POST /api/sessions/:id/branch` takes `{"answer": N}` and
`Store::branch_session` copies every stored message with `seq` up to and
including the Nth answer into a new session, in one transaction, with the
messages' images and tool calls, seq for seq. The source is not touched.
The browser puts a Branch button next to Copy under every answer, and
pressing it opens the copy.

The answer is named by ordinal rather than by seq. An answer is an
assistant message with non-empty text, the same rule `transcript` uses to
emit an `assistant` entry and the same thing the page draws as a zorp
message, so the browser can count answers as it draws them, on a reopened
transcript and on a live turn alike, and no seq has to travel. The page
does not count a row the store never got: an answer cut off by a stop or
an error stays on the page but the loop recorded nothing for it, so it
has no number and no button.

The session row is copied verbatim, `task` included. `task` is the
verbatim first message and the 2026-08-22 title entry says why it must
stay so: `recall::index_one` reads it into the search index and
`memory::block` quotes it into a later turn as something to cite. The
branch's first message is the same message, so the same text is the
right thing in that column; a title or a suffix there would be a sentence
nobody typed coming back as evidence. `display_title` is copied when
there is one. Recorded file changes are not copied: the branch has changed
no files yet, and a copied record would claim it had.

A running turn is refused with the same 409 as delete, and the page
disables the button while a turn runs, because the store is being written
to and the answers the page counted are not the answers the store has by
the time the copy is taken.

**Accepted for now:** the copy is a session like any other, so recall
indexes it, and a branched conversation can come back twice in a search
and twice in a memory block. Deduplicating that means teaching recall
about lineage the store does not record, and nothing has needed it yet.

**What it ruled out:** identifying the answer by seq, which the browser
does not have on the live path and would have to be handed. A branch
that shares storage with its source, which would make "the original is
untouched" a promise about copy-on-write rather than a fact. Copying
`changes`, for the reason above.
