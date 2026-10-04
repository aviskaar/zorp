---
status: accepted
date: 2026-09-01
---

# the bolt runs several attempts and ends in a write-up, and `deliver` was not the thing to end in

**Decision:** one press of the bolt runs `ZORP_BOLT_ATTEMPTS` attempts
(three by default, capped at ten), then hands the track to `co_write`
and `critique` and reports the draft's path so the artifact pane opens
it. That completes the last two items of #140.

**`deliver` is not what the issue thought it was.** It searches huiban
for conferences and journals and writes `venues.md`, a ranked
publication shortlist. It is where to send a paper, not the paper. The
document a person wants out of the bolt is `co_write`'s `draft.md`,
audited by `critique` against the track's own evidence record. So the
pipeline is investigate, co-write, critique, and `deliver` stays a
separate step for work actually aimed at publication. It also requires
huiban MCP tools, which most runs will not have, and making every bolt
press depend on them would have been wrong on its own.

Four things about the loop.

**Every attempt starts where the first one did.** The transcript is
truncated back to the seed before each, using the call the re-run gate
added. An attempt that can read the previous attempt's answer is not an
independent measurement, and being able to compare them is the only
reason to run several. Without this the ledger would hold one
measurement and two echoes of it.

**The pre-registration is committed once.** Attempt one writes it;
later attempts pass nothing and `investigate::run` reads the record for
itself. Re-submitting it would only give the mismatch check something
to refuse.

**A killed track stops the loop and gets no write-up.** A breach is the
pre-registered answer to the question, not a failure to reach one.
`co_write` and `critique` both refuse a killed track anyway, and a
document arguing for a hypothesis its own threshold just rejected is
the one artifact this must never produce.

**The write-up cannot fail the run.** `co_write` or `critique` falling
over returns no artifact and says so on the stream; the attempts stay
recorded. A `critique` that fails still returns the draft, because an
unaudited draft is worth more than none as long as nobody is told it
was audited. The notice says which happened.

Ruled out: a success threshold to iterate against. The pre-registration
has a kill threshold and no pass mark, and inventing one so the loop
could stop early would be the model choosing its own pass mark, which
is the exact thing #140 flagged and the whole evidence record exists to
prevent. The loop runs a fixed, stated number of attempts and stops
early only on the one criterion that was actually pre-registered.

Also ruled out: sending the draft's text on the event stream. The
artifact pane already serves and re-reads files under the workspace, so
the text would be a second copy that goes stale the moment `critique`
revises the file. The event carries a path.
