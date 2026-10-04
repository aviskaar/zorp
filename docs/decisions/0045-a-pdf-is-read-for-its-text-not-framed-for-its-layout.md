---
status: accepted
date: 2026-08-19
---

# a PDF is read for its text, not framed for its layout

**Decision:** `.pdf` leaves the sandboxed-iframe category and joins the
office formats. The server reads the text out of it with `pdf-extract`,
in a new `zorp-web/src/pdf.rs`, and sends markdown. The pane renders that
through the same renderer `.md` and `.docx` already go through. This
reverses the PDF half of the 2026-08-18 entry below and the "PDFs" section
of `superpowers/specs/2026-08-17-artifact-pane-design.md`.

**Why:** the old design said the browser's own viewer would render the file
inside the iframe. It does not, and never did. The raw endpoint sends every
file with a bare `Content-Security-Policy: sandbox`, which puts the document
in an opaque origin with scripting off, and no browser's PDF viewer starts
under that. What a user actually saw was a broken-document icon on grey.
Confirmed in Chrome against a running server, and the frame was the right
size and visible, so it was never a layout bug.

That left three ways out and only one of them is defensible.

**Ruled out, loosening the sandbox for PDFs.** Making the viewer run needs
`allow-scripts` and `allow-same-origin`, and the raw endpoint is same origin
with the app, so `allow-same-origin` hands the framed document back the
handle the sandbox existed to take away. It also has to be loosened twice,
in the response header and in the iframe's own `sandbox` attribute, and both
would become per-type decisions. Today the header is one string for every
response, which is a property that cannot be got wrong for one type; making
it conditional trades that away. And the thing being made to run is PDFium
plus a PDF's own embedded JavaScript, over a file a model wrote or
downloaded. Paying all of that to render page images, when the request was
to read the file, is the wrong trade in both directions.

**Ruled out, bundling a JavaScript PDF renderer.** `web/` has zero runtime
dependencies on purpose, pdf.js is the opposite of small, and it would parse
a hostile PDF in this origin, which is strictly worse than what is there
now.

**What the extraction gives up:** layout. No page images, no figures, no
columns reassembled, no headings, because a PDF has no headings to recover:
it records where each glyph was drawn, and a heading in one is text that was
set larger. Promoting it to `#` would be inventing structure the file does
not carry. Text and the breaks between blocks survive, and the renderer
reflows them, which is what a narrow side pane wants anyway. Same scope as
the office formats and the same sentence applies: this is for reading what a
run produced, not for rendering a document.

**Cost paid:** one dependency, `pdf-extract`, and nineteen crates the
workspace did not have, all MIT or MIT/Apache-2.0. Roughly half of them are
`lopdf`'s AES and hashing for encrypted files, which lopdf does not put
behind a feature. `pdf-extract` already asks for lopdf with default features
off, so lopdf's chrono, jiff, rayon and time stay out. The alternative was
`lopdf` alone, which is most of that tree anyway and was measured before it
was rejected: on zorp's own paper its text extraction returns
`zorp:AHuman-CheckpointedResearchAgent`, with every space gone and the
ligatures dropped, because it does not apply the kerning offsets that stand
in for spaces or resolve the font's ToUnicode table. `pdf-extract` returns
the sentence. Hand-rolling it, the way `documents.rs` is hand-rolled, lands
at lopdf's quality and not at pdf-extract's, because that gap *is* the font
machinery.

**Also changed:** both readers now run under `spawn_blocking`. Both are
parsers pointed at a file a model wrote, both are slow enough to matter on a
long document, and a panic in one comes back as a join error rather than
taking anything else with it.

**What did not change:** `.svg` and `.html` are still the only sandboxed
types, still served with a bare `sandbox` and `nosniff`, and the test that
says so now asserts the exact header for every type the endpoint serves
rather than for one of them. Verified in the browser alongside the PDF: a
served SVG carrying `parent.document.title = "pwned"` is still refused by
the engine with "Blocked script execution ... the document's frame is
sandboxed", and the title is unchanged.
