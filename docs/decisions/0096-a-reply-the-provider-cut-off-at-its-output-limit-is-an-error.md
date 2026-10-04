---
status: accepted
date: 2026-09-03
---

# a reply the provider cut off at its output limit is an error, not an answer

**Finding:** in the second Terminal-Bench run a trial ended with exit 0 and
no terminal line. The model had written one line of Python 2,300 times inside
a text-formatted tool call until oMLX stopped it at 32,768 tokens, nine
minutes of a thirteen minute trial. The reply carried `finish_reason=length`,
the loop never read that field, and the cut-off text was returned as the
finished answer.

**Decision:** a reply with no tool calls and `finish_reason` of `length` ends
the run as an error naming the limit and the byte count. The partial text
stays in the transcript, because it was said, and is not returned as an
answer, because it is not one. This is the 2026-08-23 rule applied to the
case where the provider does say why it stopped: a truncated answer that
returns `Ok` reads exactly like a finished one.

**Not changed:** a cut-off reply that did carry a parsable tool call still
runs it. A reply the provider ended with `stop` is still an answer however
short it is.
