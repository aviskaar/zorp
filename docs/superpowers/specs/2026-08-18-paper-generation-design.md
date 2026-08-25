# Paper generation: a paper-shaped artifact, and a PDF of it

Status: built. 2026-08-18.

`deliver` had one mode: match a finished draft against real venues. This
adds a second, `deliver --paper`, which turns `draft.md` plus the track's
evidence record into `paper.md` and `paper.pdf`. It closes the gap the
2026-08-17 artifact-pane decision named out loud: "generating PDFs, which
needs LaTeX or typst and is its own project."

## What it produces

A document with a title, a byline (only if you give one), the track it
came from, an abstract, numbered sections, and a reference list. The
markdown carries pandoc-style front matter, so anyone who does want a
LaTeX toolchain can hand `paper.md` to one. The PDF is a rendering of the
same document, not a second source of truth.

No model is called. The whole thing is a function of `draft.md` and the
record, so it runs offline, needs no API key, needs no MCP server, and
produces the same bytes on every run.

## The PDF engine: written here, not shelled out to and not vendored

The engine is `zorp-paper`, a new workspace member with zero
dependencies. It writes the PDF file format directly: indirect objects,
an xref table, uncompressed content streams, and text placed with the
base-14 fonts a viewer already has.

**Why not shell out to typst, pandoc or xelatex.** Fidelity would be
better and the build weight would be zero. The cost is that
`deliver --paper` would do nothing useful on a machine without them,
which is most machines, and the path most users hit first would be the
degradation path. zorp is a tool people run once to see whether it is
worth running again. A first run that says "install a 400 MB TeX
distribution and try again" is a first run that does not happen. There is
also a supply-chain edge: shelling out to whatever `typst` resolves to on
`PATH` is a code-execution surface, and the document being typeset is
model output.

**Why not vendor a Rust typesetting crate.** Self-contained, and someone
else's problem to maintain. But `Cargo.lock` is committed, CI builds
`--locked`, and MSRV is 1.82, so every dependency here is a standing
commitment. The crates that would do this well pull in font parsing,
shaping and often an image stack, which is a large surface for a document
with no figures, no tables and no maths. The repository has already made
this call once, in the other direction that mattered: `web/src/markdown.ts`
is hand-written because every markdown library hands you an `innerHTML`
call. Same shape of decision, same answer.

**Why writing it is smaller than it sounds.** The parts of typesetting
that are genuinely hard are the parts this document does not have. There
are no floats, no footnotes, no maths, no tables, no figures, no
multi-column flow. What is left is one column of text, and the base-14
fonts mean no font file is embedded, parsed or subset: the viewer already
has Times and Courier. The only thing the writer needs that it cannot get
for free is character widths, so it can decide where to break a line, and
those are Adobe's published AFM numbers for printable ASCII. A wrong
number there costs line-fill quality and nothing else, because the viewer
places glyphs from its own copy of the metrics.

## Citation integrity

Two mechanisms, one structural and one checked.

**Structural.** The reference list is built by `evidence::for_track`,
which reads validate's cited sources and every metric investigate
recorded, and assigns keys `E1` upward by position. Nothing in the draft
can add an entry to it. A `References` section written by the model is
dropped from the draft on the way in, along with anything nested under
it, and replaced by the record-derived list. That is the case worth
naming: a model asked for a paper will produce a plausible bibliography,
and a plausible bibliography is the artifact this feature exists to keep
out of the output.

**Checked.** `Paper::assemble` is the only constructor for a `Paper`, and
it refuses one whose prose cites a reference the list does not have.
Markers are `[E1]` by key or `[1]` by position. Numeric markers count
because that is how a model writes citations when asked for a paper, and
an unresolvable `[7]` is exactly the failure worth catching. The grammar
is narrow enough that ordinary prose does not trip it: a bracket group
only counts when it sits at a word boundary and every token in it is a
key or a number, so `samples[0]` is an index expression, `[see the
appendix]` is prose, and code blocks are not scanned at all.

An unresolvable citation is fatal, and nothing is written. That is the
one refusal worth making here: a paper whose citations do not resolve is
worse than no paper.

## What never fails the run

Typesetting. `paper.md` is written first, then the PDF is attempted. A
PDF that cannot be written leaves `pdf_error` set, prints the reason on
stderr, says so in the checkpoint prompt, and the delivery still
succeeds. The markdown is the artifact.

## Determinism

The document's date comes from the track's `created_at`, not from the
clock, so re-running deliver on an unchanged track rewrites the same
bytes. `PdfOptions::creation_date` is the only value in the PDF that
could vary, and the caller supplies it; `None` omits the key entirely.
Nothing in `zorp-paper` reads the clock, the environment or the
filesystem.

## co-write, changed by one prompt

co-write now lists the evidence record by key and asks the model to cite
inline as `[E1]`, and tells it not to write a references section. Both
ends read the same `evidence::for_track`, so a draft that follows the
instruction produces a paper whose citations resolve, and a draft that
does not gets refused with the offending marker named.

## What this does not do

- No figures, tables, or maths. A markdown table survives as a
  fixed-width verbatim block, which keeps the columns readable and is
  honestly all it is.
- No hyphenation, no justification, no kerning or ligature control. The
  text is ragged right.
- WinAnsi only. Text outside it becomes `?`, so this cannot typeset CJK
  or Greek. A document that needed to would need embedded fonts, which is
  a different project.
- No bibliography formats. References are the record's own claim and
  source strings, not BibTeX and not APA.
- Free-text citations in prose that do not use the marker syntax, such as
  a bare "(Smith 2020)", are not detected. The reference list stays
  clean, because nothing can add to it, but the sentence still reads as a
  citation.
