---
status: accepted
date: 2026-08-22
---

# a PDF in the artifact pane is a PDF, and the isolation is the response header

**Decision:** `GET /api/artifacts/raw` sends a `.pdf` as
`application/pdf` again, and the pane frames it for the browser's own
viewer to draw. The frame it goes into carries no `sandbox` attribute.
What isolates it is the response:
`Content-Security-Policy: sandbox allow-scripts`, one token wider than
the bare `sandbox` every other served type still gets. The text read out
of the file stays, as the fallback, behind `?as=text`.

Showing the extracted text was the previous answer and it was reported as
a bug, correctly: a run that produces a paper produces a paper, and a
pane that shows the words out of it has thrown away the layout,
the figures and the page count. The reason it was doing that is in the
2026-08-17 pane design and in the comments that were sitting on this
code: an earlier attempt pointed the existing sandbox frame at the raw
endpoint, got a broken-document icon on grey, and concluded no browser's
viewer starts in a sandbox. Half right. The viewer does not start with
scripting off, and that is the only thing wrong with the old
configuration.

Measured in Chrome 151 on macOS, one PDF under six header and attribute
combinations, captured over CDP with `fromSurface` because neither
`captureVisibleTab` nor headless `--screenshot` composites the viewer at
all and both come back a plausible empty grey:

- iframe `sandbox=""`, response `sandbox`: broken-document icon. This is
  what was shipped and what was reported.
- iframe `sandbox="allow-scripts"`, response `sandbox allow-scripts`:
  broken-document icon.
- iframe `sandbox="allow-scripts"`, no response CSP: broken-document
  icon. So it is the attribute, not the header, and no value of the
  attribute rescues it.
- no attribute, response `sandbox allow-scripts`: the viewer starts. Two
  page thumbnails, a toolbar, the text on the page.

The security question that leaves is whether the header alone isolates,
and that was measured too rather than read off the spec. A hostile page
served under exactly `sandbox allow-scripts`, framed with no attribute:
`parent.document`, `parent.location` and `localStorage` each throw
`SecurityError`, `window.origin` is `null`, and the framing page's title
is untouched. `allow-same-origin` is the token that would undo all of
that and it is not there.

Two things carry the rest. `served_as` gives a PDF its own variant rather
than making it a third `Sandboxed` one, so the widened policy cannot
spread to the two types that really do execute, and the test that pins
the sandboxed list now pins the framed list beside it. And
`content_type(path)` became `response_type(path, form)`, because the type
of a file and the type of a response stopped being the same question the
moment a PDF had two answers at one URL: keying the header off the
extension alone is how a body full of extracted text ends up labelled
`application/pdf`.

The fallback is real and has two triggers, neither of which is a guess.
The pane frames a PDF only when `navigator.pdfViewerEnabled` is `true`,
so a browser that says no, or is too old to have been asked, gets the
words instead of an empty pane. And the server checks for a `%PDF-`
header before handing bytes to a viewer, because a model writing markdown
into `report.pdf` is an ordinary Tuesday and the viewer's answer to that
is the same broken-document icon this entry is about.

**Ruled out:** bundling pdf.js and rendering to a `<canvas>`. It would
work in every browser, and it is easier to screenshot, which given the
capture trouble above was a real temptation. But it parses hostile PDF
bytes in this page's own origin, which is the one thing
`web/src/markdown.ts` and everything around it exists to prevent, and
CVE-2024-4367 is what that costs when it goes wrong. It also puts about a
megabyte and a half of runtime dependency into a `web/` that has none at
all today, and a worker that cannot be loaded from an opaque origin
anyway. The browser's viewer runs in a process this page cannot reach,
and that is a stronger boundary than any bundle.

**Not verified:** Safari. There is no Safari automation on this machine
and the claim is not made. Desktop Safari reports
`navigator.pdfViewerEnabled` and has a viewer, so the framed path is what
it should take; if it does not render, the capability check is the wrong
question and the fix is to widen the fallback, not the sandbox.

See `docs/superpowers/specs/2026-08-17-artifact-pane-design.md`, whose
PDF section described this design and whose one wrong sentence, that the
bare `sandbox` CSP is what keeps a hostile PDF away from the page, is
corrected here.

---
