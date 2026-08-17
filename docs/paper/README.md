# zorp paper

`zorp-paper.md` is the systems and design paper about zorp itself (see
`docs/DECISIONS.md`, 2026-08-09): the architecture, the `zorp-track`
evidence foundation, and the four capabilities. It is a design and
status report, not a benchmark study. The paper says so plainly and
states what a real comparative evaluation would require.

## Status

Written and building. `zorp-paper.md` is the full draft, and the built
`zorp-paper.pdf` is committed alongside it, so the paper can be read
without a TeX toolchain installed. Figures, the bibliography, the
arXiv-style template, and the Makefile that ties them together are all
here.

What is still open:

- It has not been posted to arXiv.
- There is no comparative evaluation. The paper's own Limitations
  section says so and states what such a study would require.
- The test and line counts in Section 7 and Table 1 are pinned to commit
  `fd07e81` (2026-08-13) and have drifted since.
- Only the two AI-Scientist entries in `references.bib` have been
  verified against their published records.

## Building

```bash
make          # regenerate figures, then build the PDF
make paper    # PDF only
make figures  # figures only, from current repo state
```

Needs `pandoc`, a TeX distribution with `newtx` (TeX Live works), and
`python3` with `matplotlib`. The build runs the full
pandoc, pdflatex, bibtex, pdflatex, pdflatex cycle and removes its own
intermediates; only `zorp-paper.pdf` is left behind.

`ghostscript` is an optional last step: pdfTeX emits a PDF 1.5 with object
streams, which GitHub's inline PDF viewer renders inconsistently, so the
build downconverts to a linearized PDF 1.4 with all fonts embedded, the most
broadly compatible form. If `gs` is not installed the step is skipped and the
plain pdflatex output is used, which still opens fine in a normal PDF reader
but may error in GitHub's viewer.

## Layout

| File | What it is |
|---|---|
| `zorp-paper.md` | the paper source, the only file to edit for prose |
| `zorp-paper.pdf` | the built paper, committed so it can be read without a TeX toolchain |
| `arxiv-template.tex` | pandoc LaTeX template: arXiv preprint style, Times via newtx |
| `references.bib` | bibliography |
| `Makefile` | the build, see above |
| `make_figures.py` | generates `figures/` from real repo state |
| `figures/logo.png` | the real zorp mark, placed directly, **not** generated |
| `venues.md` | candidate venues from a 2026-08-09 search, not part of the paper build |

Figures are built from actual repository state (crate layout, `cargo
test` output), not mocked up, and use Times to match the body text.
Colors come from `zorp-landing/src/styles/tokens.css`. Do not
regenerate `logo.png`; the script does not produce it.

## Before submitting anywhere

The two AI-Scientist entries in `references.bib` were verified against
arxiv.org directly. Re-verify the rest against their published records
before submission, and refresh the test and line counts in Section 7
and Table 1, which are pinned to a specific commit.

See `venues.md` for candidate venues once there is a real eval story to
report.
