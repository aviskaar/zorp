/**
 * Tests for the artifact pane's display rules.
 *
 * The block that matters is the sandboxing one. The pane now shows `.svg` and
 * `.html`, and both of those are documents that execute. The user asked for
 * them after being told so. What makes that answerable is that they only ever
 * appear inside the iframe, whose source the server serves with a bare
 * `Content-Security-Policy: sandbox`. Putting either into the page's own DOM
 * would run their script in this origin and make every precaution in
 * `markdown.ts` pointless, so these tests exist to catch exactly that mistake.
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import { JSDOM } from "jsdom";
import {
  needsText,
  showArtifact,
  textUrl,
  viewMode,
  type Pane,
} from "../src/artifact-view.ts";
import { renderMarkdown } from "../src/markdown.ts";

const dom = new JSDOM("<!doctype html><body></body>");
(globalThis as Record<string, unknown>).document = dom.window.document;
(globalThis as Record<string, unknown>).Node = dom.window.Node;

function pane(): Pane {
  const make = <T extends Element>(tag: string): T =>
    dom.window.document.createElement(tag) as unknown as T;
  const surfaces: Pane = {
    doc: make<HTMLElement>("div"),
    frame: make<HTMLIFrameElement>("iframe"),
    pdf: make<HTMLIFrameElement>("iframe"),
    image: make<HTMLImageElement>("img"),
    empty: make<HTMLElement>("p"),
  };
  dom.window.document.body.replaceChildren(
    surfaces.doc as unknown as Node,
    surfaces.frame as unknown as Node,
    surfaces.pdf as unknown as Node,
    surfaces.image as unknown as Node,
    surfaces.empty as unknown as Node,
  );
  return surfaces;
}

/**
 * jsdom does not implement `navigator.pdfViewerEnabled`, and every test below
 * says which browser it means rather than letting the default decide. Both
 * halves matter: a browser with a viewer must get the file, and one without
 * must get the words, and neither is the other's edge case.
 */
const WITH_VIEWER = true;
const WITHOUT_VIEWER = false;

/* ------------------------------------------------------------------ */
/* the formats that execute                                            */
/* ------------------------------------------------------------------ */

const HOSTILE_SVG =
  '<svg xmlns="http://www.w3.org/2000/svg">' +
  "<script>window.pwned = true; parent.document.title = 'pwned';</script>" +
  "</svg>";

const HOSTILE_HTML =
  "<html><body><img src=x onerror=\"window.pwned = true\">" +
  "<script>top.location = 'http://evil.example'</script></body></html>";

for (const [what, path, bytes] of [
  ["svg", "chart.svg", HOSTILE_SVG],
  ["html", "report.html", HOSTILE_HTML],
] as const) {
  test(`a served ${what} never reaches the page's own DOM`, () => {
    const surfaces = pane();
    // The bytes are passed in as if they had been fetched, which is the
    // strongest form of the test: even handed the file's contents, the pane
    // must not put them on the page.
    showArtifact(surfaces, path, "/api/artifacts/raw?path=" + path, bytes, renderMarkdown);

    assert.equal(surfaces.doc.childNodes.length, 0, "the file was rendered into the page");
    assert.ok(!surfaces.doc.innerHTML.includes("script"), surfaces.doc.innerHTML);
    assert.ok(!surfaces.doc.innerHTML.includes("onerror"), surfaces.doc.innerHTML);
    assert.equal(
      dom.window.document.querySelectorAll("script").length,
      0,
      "a script element was created in this origin",
    );
    assert.equal(
      (dom.window as unknown as Record<string, unknown>).pwned,
      undefined,
      "script from the served file ran in the page",
    );
  });

  test(`a served ${what} is shown only through the sandboxed iframe`, () => {
    const surfaces = pane();
    showArtifact(surfaces, path, "/api/artifacts/raw?path=" + path, bytes, renderMarkdown);

    assert.equal(surfaces.frame.hidden, false, "the iframe was not used");
    assert.equal(surfaces.frame.getAttribute("src"), "/api/artifacts/raw?path=" + path);
    assert.equal(surfaces.doc.hidden, true);
    assert.equal(surfaces.image.hidden, true);
    // The srcdoc attribute would load the bytes as a document this page
    // controls, which is the same hole by another route.
    assert.equal(surfaces.frame.getAttribute("srcdoc"), null);
    // And never the PDF frame, which is served under a policy one token
    // wider. A file that executes must not reach the surface that allows
    // scripting, whichever browser this is.
    assert.equal(surfaces.pdf.hidden, true, "a file that executes reached the PDF frame");
    assert.equal(surfaces.pdf.getAttribute("src"), null);
  });

  test(`a served ${what} stays sandboxed on a browser that can render PDFs`, () => {
    // The PDF surface arriving must not have moved anything else onto it.
    assert.equal(viewMode(path, WITH_VIEWER), "sandboxed", path);
    const surfaces = pane();
    showArtifact(surfaces, path, "/raw?path=" + path, bytes, renderMarkdown, WITH_VIEWER);
    assert.equal(surfaces.frame.hidden, false);
    assert.equal(surfaces.pdf.hidden, true);
    assert.equal(surfaces.doc.childNodes.length, 0);
  });
}

/**
 * The mode is what decides between the iframe and the page. Nothing that
 * executes may be anything other than "sandboxed", so this is asserted
 * separately from the rendering above: a regression in either one is enough
 * to matter.
 */
test("every format that can execute is classified as sandboxed", () => {
  for (const path of ["a.svg", "a.SVG", "a.html", "a.HTML", "deep/dir/x.svg"]) {
    // On both browsers, because the PDF branch must not have opened a way
    // round this one.
    assert.equal(viewMode(path, WITH_VIEWER), "sandboxed", path);
    assert.equal(viewMode(path, WITHOUT_VIEWER), "sandboxed", path);
  }
});

test("nothing but svg and html is classified as sandboxed", () => {
  for (const path of [
    "a.md",
    "a.markdown",
    "a.txt",
    "a.json",
    "a.csv",
    "a.docx",
    "a.xlsx",
    "a.pdf",
    "a.png",
  ]) {
    assert.notEqual(viewMode(path, WITH_VIEWER), "sandboxed", path);
    assert.notEqual(viewMode(path, WITHOUT_VIEWER), "sandboxed", path);
  }
});

/**
 * The other half of that: only a PDF may reach the surface served under
 * `sandbox allow-scripts`. It is the one frame on this page where a document
 * is allowed to run script at all, so what is allowed onto it is a list of
 * one and this is what says so.
 */
test("nothing but a pdf is classified as pdf", () => {
  for (const path of [
    "a.md",
    "a.txt",
    "a.json",
    "a.csv",
    "a.docx",
    "a.xlsx",
    "a.png",
    "a.svg",
    "a.html",
    "a.pdfx",
    "a.xpdf",
  ]) {
    assert.notEqual(viewMode(path, WITH_VIEWER), "pdf", path);
  }
  for (const path of ["a.pdf", "a.PDF", "deep/dir/paper.Pdf"]) {
    assert.equal(viewMode(path, WITH_VIEWER), "pdf", path);
  }
});

/* ------------------------------------------------------------------ */
/* the pdf, which is a third thing                                     */
/* ------------------------------------------------------------------ */

/**
 * The bug this file exists to pin: a PDF a run produced must look like a PDF.
 * It is addressed by URL from the PDF frame, and the bytes never come into
 * this page at all, which is why `needsText` is false for it.
 */
test("a pdf goes to the browser's viewer, by URL, and never into this page", () => {
  assert.equal(viewMode("paper.pdf", WITH_VIEWER), "pdf");
  assert.equal(viewMode("out/PAPER.PDF", WITH_VIEWER), "pdf");
  assert.equal(needsText("paper.pdf", WITH_VIEWER), false);

  const surfaces = pane();
  // Handed the bytes as well, which is the strongest form of the test: even
  // with contents in hand the pane must address the file by URL.
  showArtifact(surfaces, "paper.pdf", "/raw?path=paper.pdf", "%PDF-1.4 ...", renderMarkdown, WITH_VIEWER);

  assert.equal(surfaces.pdf.hidden, false, "the PDF frame was not used");
  assert.equal(surfaces.pdf.getAttribute("src"), "/raw?path=paper.pdf");
  // srcdoc would make this page the document's author and hand it whatever
  // the parent grants, which is the whole thing this avoids.
  assert.equal(surfaces.pdf.getAttribute("srcdoc"), null);
  assert.equal(surfaces.doc.childNodes.length, 0, "the file was rendered into the page");
  assert.equal(surfaces.doc.hidden, true);
  assert.equal(surfaces.frame.hidden, true, "a pdf reached the no-script sandbox");
  assert.equal(surfaces.image.hidden, true);
});

/**
 * The fallback, and the reason the extraction on the server is still there. A
 * browser with no viewer gets the words out of the file. An empty pane would
 * be the one outcome worse than the bug being fixed.
 */
test("with no viewer in the browser, a pdf is read as text and rendered", () => {
  assert.equal(viewMode("paper.pdf", WITHOUT_VIEWER), "markdown");
  assert.equal(needsText("paper.pdf", WITHOUT_VIEWER), true);

  const surfaces = pane();
  showArtifact(
    surfaces,
    "paper.pdf",
    "/raw?path=paper.pdf",
    "Findings\n\nLatency fell by 40 percent.",
    renderMarkdown,
    WITHOUT_VIEWER,
  );

  assert.equal(surfaces.doc.hidden, false);
  assert.equal(surfaces.doc.querySelector("p")?.textContent, "Findings");
  assert.equal(surfaces.pdf.hidden, true, "a frame was used on a browser with no viewer");
  assert.equal(surfaces.pdf.getAttribute("src"), null);
  assert.equal(surfaces.frame.hidden, true);
});

/**
 * Switching from a PDF to anything else must empty the frame, not just hide
 * it. A hidden viewer is still a loaded viewer, and the next file's contents
 * are not the last file's.
 */
test("leaving a pdf empties the frame rather than only hiding it", () => {
  const surfaces = pane();
  showArtifact(surfaces, "paper.pdf", "/raw?path=paper.pdf", null, renderMarkdown, WITH_VIEWER);
  assert.equal(surfaces.pdf.getAttribute("src"), "/raw?path=paper.pdf");

  showArtifact(surfaces, "draft.md", "/raw?path=draft.md", "# Findings", renderMarkdown, WITH_VIEWER);
  assert.equal(surfaces.pdf.hidden, true);
  assert.equal(surfaces.pdf.getAttribute("src"), null, "the old PDF was still loaded");
});

/**
 * The fallback is a different URL, not the same one. The server has two
 * answers at that path and `as=text` is which one this asks for.
 */
test("the text of a document is asked for with as=text", () => {
  assert.equal(textUrl("/api/artifacts/raw?path=a.pdf"), "/api/artifacts/raw?path=a.pdf&as=text");
  // A URL with no query of its own still gets a well formed one.
  assert.equal(textUrl("/raw"), "/raw?as=text");
});

/**
 * A path is not an extension. `evil.svg.md` ends in `.md` and must be read as
 * markdown; `notes.md.svg` ends in `.svg` and must not.
 */
test("the mode comes from the last extension, not from any extension present", () => {
  assert.equal(viewMode("evil.svg.md", WITH_VIEWER), "markdown");
  assert.equal(viewMode("notes.md.svg", WITH_VIEWER), "sandboxed");
  assert.equal(viewMode("archive.html.txt", WITH_VIEWER), "text");
  // And the same for the PDF branch: the name has to end in it.
  assert.equal(viewMode("paper.pdf.txt", WITH_VIEWER), "text");
  assert.equal(viewMode("notes.md.pdf", WITH_VIEWER), "pdf");
});

/* ------------------------------------------------------------------ */
/* the formats that do not                                             */
/* ------------------------------------------------------------------ */

test("an image goes into an img element, not into the markdown renderer", () => {
  const surfaces = pane();
  showArtifact(surfaces, "shot.png", "/raw?path=shot.png", null, renderMarkdown);

  assert.equal(surfaces.image.hidden, false);
  assert.equal(surfaces.image.getAttribute("src"), "/raw?path=shot.png");
  assert.equal(surfaces.frame.hidden, true);
  assert.equal(surfaces.doc.hidden, true);
});

test("markdown is rendered as markdown", () => {
  const surfaces = pane();
  showArtifact(surfaces, "draft.md", "/raw?path=draft.md", "# Findings\n\nLatency fell.", renderMarkdown);

  assert.equal(surfaces.doc.hidden, false);
  assert.equal(surfaces.doc.querySelector("h1")?.textContent, "Findings");
  assert.equal(surfaces.frame.hidden, true);
});

/** The server extracts these to markdown, so the pane renders them as such. */
test("an office document is rendered through the same markdown renderer", () => {
  for (const path of ["memo.docx", "memo.odt", "book.xlsx", "deck.pptx"]) {
    const surfaces = pane();
    showArtifact(surfaces, path, `/raw?path=${path}`, "## Slide 1\n\nOpening", renderMarkdown);
    assert.equal(surfaces.doc.querySelector("h2")?.textContent, "Slide 1", path);
    assert.equal(surfaces.frame.hidden, true, path);
  }
});

test("anything else is shown as plain text in a code block", () => {
  const surfaces = pane();
  showArtifact(surfaces, "data.csv", "/raw?path=data.csv", "a,b\n<script>x</script>,2", renderMarkdown);

  const code = surfaces.doc.querySelector("code");
  assert.equal(code?.textContent, "a,b\n<script>x</script>,2");
  assert.equal(surfaces.doc.querySelectorAll("script").length, 0);
});

/* ------------------------------------------------------------------ */
/* noticing what a run produced                                        */
/* ------------------------------------------------------------------ */

import { producedSince, type ArtifactStamp } from "../src/artifact-view.ts";

const stamp = (path: string, modified_ms: number): ArtifactStamp => ({ path, modified_ms });

test("a file that did not exist before the turn counts as produced", () => {
  const before = [stamp("notes.md", 100)];
  const after = [stamp("notes.md", 100), stamp("draft.md", 200)];
  assert.deepEqual(
    producedSince(before, after).map((f) => f.path),
    ["draft.md"],
  );
});

/** The case size alone would miss: same path, same length, new contents. */
test("a file rewritten during the turn counts as produced", () => {
  const before = [stamp("draft.md", 100)];
  const after = [stamp("draft.md", 175)];
  assert.deepEqual(
    producedSince(before, after).map((f) => f.path),
    ["draft.md"],
  );
});

test("a file nothing touched does not count as produced", () => {
  const before = [stamp("draft.md", 100), stamp("notes.md", 50)];
  assert.deepEqual(producedSince(before, after_unchanged()), []);
  function after_unchanged() {
    return [stamp("draft.md", 100), stamp("notes.md", 50)];
  }
});

/**
 * The point of diffing the listing rather than reading tool events: how the
 * file was made is not knowable from a tool summary, and a PDF written by
 * pandoc through `run_command` has to be caught exactly like one written by
 * `write_file`.
 */
test("a file produced by any means at all is caught, because only the listing is consulted", () => {
  const before = [stamp("notes.md", 100)];
  const after = [stamp("notes.md", 100), stamp("out/paper.pdf", 300), stamp("chart.svg", 250)];
  assert.deepEqual(
    producedSince(before, after).map((f) => f.path),
    // Newest first, so "show what the run produced" shows the latest one.
    ["out/paper.pdf", "chart.svg"],
  );
});

test("with no snapshot at all, nothing is claimed to be new", () => {
  assert.deepEqual(producedSince(null, [stamp("draft.md", 200)]), []);
});

/**
 * A turn that edits an existing file and then creates a new one, in that
 * order, must still open the new one: the edit's later timestamp is not
 * allowed to bury the file the turn actually made. `main.ts` opens
 * `fresh[0]`, so this ordering is what decides what the pane shows.
 */
test("a newly created file is shown ahead of an edit made later in the same turn", () => {
  const before = [stamp("existing.md", 100)];
  const after = [stamp("existing.md", 300), stamp("new.md", 200)];
  assert.deepEqual(
    producedSince(before, after).map((f) => f.path),
    ["new.md", "existing.md"],
  );
});
