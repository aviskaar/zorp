---
status: accepted
date: 2026-08-13
---

# paper corrected to zorp-landing's real branding and arXiv formatting

**Decision:** the first paper draft (below) used the wrong logo (the
purple-on-dark favicon glyph) and an assumed dark/purple color scheme
that doesn't match zorp-landing. Corrected: the header and title page
now use the real zorp mark (a node-and-edge "Z", navy with two
electric-blue accent nodes), and all three generated figures were
recolored to zorp-landing's actual palette
(`zorp-landing/src/styles/tokens.css`: light theme, `--z-fg` navy,
`--z-accent` electric blue). The paper's language was also brought in
line with the live site (`zorp-landing/src/config/site.ts`): opens with
the site's own hook ("Answers are cheap. Evidence is not."), names the
kill threshold the way the site does, and documents the real six-table
evidence store (`tracks`, `preregistrations`, `experiments`, `metrics`,
`checkpoints`, `validations`) rather than a general two-store
description. Test and line counts were re-verified directly against the
repo rather than trusted from the landing page, which turned out to be
stale in places (a screenshot asset showed a different tagline and an
older test count than the current site config); the paper now cites
538 passing tests and 24,965 lines, both freshly confirmed against
HEAD. Formatting changed to a single-column arXiv-preprint style:
numbered sections, colored running header, boxed abstract. Also, per
house style (`CLAUDE.md`), all em/en dashes used as punctuation were
rewritten as periods, commas, colons, or plain hyphenated compounds.

**Why:** a systems paper about zorp representing zorp with the wrong
brand mark and colors undermines the credibility it's trying to
establish. Trusting a stale marketing asset over the actual repository
state would have repeated the same kind of unverified-claim mistake the
paper explicitly argues against.
