---
status: accepted
date: 2026-08-13
---

# zorp's own arXiv-style systems paper written, first draft

**Decision:** `docs/paper/zorp-paper.md` is a first draft of the systems
paper about zorp itself, scoped in `docs/paper/README.md` since
2026-08-09. Covers architecture, the `zorp-track` foundation, and the
four capabilities, all grounded in what's actually built and tested as
of this writing (real test counts and LOC, not aspirational numbers).
Figures (layered architecture, the four-capability pipeline, test
counts) are generated from real repo state by `docs/paper/make_figures.py`,
not mocked up. The header logo is redrawn from
`zorp-landing/public/favicon.svg`. Built to PDF via
`pandoc --pdf-engine=xelatex`; both the Markdown source and the rendered
`zorp-paper.pdf` are committed.

**Why:** the paper needs a real eval story before it's submittable
anywhere ranked (see `docs/paper/venues.md`), which doesn't exist yet.
This draft is explicit about that gap: it reports the design and what's
tested, and lists the comparative evaluation against AI-Scientist-v2 as
future work rather than fabricating numbers to fill the gap. Posting to
arXiv as a preprint doesn't need review or a deadline, so this draft can
go out regardless of the ranked-venue timeline in `venues.md`.
