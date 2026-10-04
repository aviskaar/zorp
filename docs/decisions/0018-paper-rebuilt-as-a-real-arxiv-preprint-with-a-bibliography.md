---
status: accepted
date: 2026-08-13
---

# paper rebuilt as a real arXiv preprint, with a bibliography

**Decision:** the paper is now built through a proper LaTeX toolchain
rather than pandoc's defaults. `docs/paper/arxiv-template.tex` is a
pandoc template implementing the standard arXiv preprint presentation
(Times via `newtx`, ruled abstract block, numbered sections, small-caps
running head), `docs/paper/references.bib` is a real bibliography, and
`docs/paper/Makefile` runs the full pandoc, pdflatex, bibtex, pdflatex,
pdflatex cycle and cleans up after itself. Figures were regenerated in
Times to match the body text, in a restrained academic style rather
than a marketing one. The paper itself was rewritten: a Design
Principles section now states the three commitments the system is
organized around, a Limitations section states what the tamper-evidence
guarantee does and does not cover (it is scoped to the pre-registration
file and inherits git's trust model), and claims are cited throughout.

**Why:** the previous draft had no citations at all, which no venue
would take seriously, and its Helvetica-on-pandoc-defaults presentation
read as a rendered README rather than a paper. The bibliography also
does real argumentative work: zorp's central mechanism, committing a
falsification threshold before gathering evidence, is the
pre-registration literature's answer to undisclosed analytic
flexibility, and the paper is stronger for saying so and citing it than
for presenting the idea as novel. The two AI-Scientist references were
verified against arxiv.org directly, since they carry the paper's
framing; `docs/paper/README.md` flags that the rest need a verification
pass before submission.
