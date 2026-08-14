# zorp paper

`zorp-paper.md` is the systems/design paper about zorp itself (see
`docs/DECISIONS.md`, 2026-08-09), covering the architecture, the
`zorp-track` foundation, and the four capabilities as of 2026-08-13.
It's a design and status report, not a benchmark study; the paper says
so explicitly and lists the comparative evaluation (vs. AI-Scientist-v2)
still needed as future work.

Build the PDF with:

```bash
python3 make_figures.py   # regenerate figures/ from current repo state
pandoc zorp-paper.md -o zorp-paper.pdf --pdf-engine=xelatex -V mainfont="Helvetica"
```

`figures/` holds the generated architecture, pipeline, and test-count
diagrams (from `make_figures.py`, built from real repo state, not
mockups) plus the logo redrawn from `zorp-landing/public/favicon.svg`.

See `venues.md` for where to submit once there's a real eval story to
report.
