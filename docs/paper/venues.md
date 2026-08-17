# Where to submit

Research findings from a 2026-08-09 pass through the huiban conference/
journal database, filtered to venues that plausibly fit a systems paper
about an agent harness (design plus evals against comparable systems like
AI-Scientist-v2 and Aviskaar's own lab-engine/Catalyst). Not a decision,
just the candidate list and a recommended sequence.

| Venue | Type | Rank | Next deadline | Fit |
|---|---|---|---|---|
| ICLR 2027 | Conference | CCF-A / CORE-A* | 2026-09-18 | Best near-term fit. ML systems/agents work is squarely in scope, and the deadline is the closest of any top venue. |
| NeurIPS 2027 | Conference | CCF-A / CORE-A* / A1 | around May 2027 (2026 cycle closed) | Same fit as ICLR, especially with a Datasets and Benchmarks framing. |
| ICML 2027 | Conference | CCF-A / CORE-A* / A1 | around January 2027 (2026 cycle closed) | Same tier, same fit. |
| ASE 2027 | Conference | CCF-A / CORE-A* / A1 | around March 2027 (2026 cycle closed) | Software-engineering framing: the harness as a tool, not as an ML result. Fits if the design (sandboxing, verification, trust) carries the paper more than the eval numbers do. |
| FSE 2027 | Conference | CCF-A / CORE-A* / A1 | 2026-10-02 | Same SE framing as ASE, closer deadline. |
| OSDI 2027 | Conference | CCF-A / CORE-A* / A1 | 2026-12-01 | Only fits if the systems contribution is genuinely deep (process isolation, sandbox internals), not just "we built an agent." Long shot for a first paper. |
| Empirical Software Engineering (ESE) | Journal | CCF-B, impact factor 3.6 | rolling, no fixed deadline | No deadline pressure, room for a longer, more thorough empirical writeup. Reasonable fallback if the eval story needs more time than a conference cycle allows. |

## Recommended sequence

zorp is pre-alpha. All four capabilities (validate, investigate,
co-write, deliver) are built and tested, and the writeup exists at
`zorp-paper.md`, but the paper still needs a real eval story (zorp
compared against AI-Scientist-v2 and Catalyst) to be worth submitting
anywhere ranked. A ranked submission today would still be a design doc,
not a paper. This paragraph originally said nothing in the
four-capability product was built yet, which was true when it was
written on 2026-08-09 and is not true now; the rest of this file is that
day's search and has not been re-run.

1. Post to arXiv as a preprint whenever the writeup is ready. No review,
   no deadline, immediate. Do this regardless of what happens next. Not
   done yet: the draft is written and built, and has not been posted.
2. Once there's a real eval story: ICLR's September deadline is the
   nearest top-tier target that fits. FSE's October deadline is the
   nearest SE-framed alternative if the tool-design angle ends up
   stronger than the benchmark-numbers angle.
3. ESE is the fallback if neither cycle lines up with when the writeup is
   actually done.

Re-run this search closer to the time; deadlines shift year to year and
this list reflects venues as of 2026-08-09.
