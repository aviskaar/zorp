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
import { needsText, showArtifact, viewMode, type Pane } from "../src/artifact-view.ts";
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
    image: make<HTMLImageElement>("img"),
    empty: make<HTMLElement>("p"),
  };
  dom.window.document.body.replaceChildren(
    surfaces.doc as unknown as Node,
    surfaces.frame as unknown as Node,
    surfaces.image as unknown as Node,
    surfaces.empty as unknown as Node,
  );
  return surfaces;
}

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
    assert.equal(viewMode(path), "sandboxed", path);
  }
});

test("text formats are never classified as sandboxed", () => {
  for (const path of [
    "a.md",
    "a.markdown",
    "a.txt",
    "a.json",
    "a.csv",
    "a.docx",
    "a.xlsx",
    "a.pdf",
  ]) {
    assert.notEqual(viewMode(path), "sandboxed", path);
  }
});

/**
 * A PDF is read on the server now, so what arrives here is markdown and it
 * goes on the page like any other document. The iframe was the old answer and
 * it did not work: the raw endpoint sends a bare `Content-Security-Policy:
 * sandbox`, which is an opaque origin with no scripting, and no browser's PDF
 * viewer starts under one, so the pane showed a broken-document icon.
 *
 * Only `.svg` and `.html` still go into the iframe, and the tests above are
 * what says so. Nothing here loosens that.
 */
test("a pdf is read as text and rendered, not framed", () => {
  assert.equal(viewMode("paper.pdf"), "markdown");
  assert.equal(viewMode("out/PAPER.PDF"), "markdown");
  assert.equal(needsText("paper.pdf"), true);

  const surfaces = pane();
  showArtifact(
    surfaces,
    "paper.pdf",
    "/raw?path=paper.pdf",
    "Findings\n\nLatency fell by 40 percent.",
    renderMarkdown,
  );

  assert.equal(surfaces.doc.hidden, false);
  assert.equal(surfaces.doc.querySelector("p")?.textContent, "Findings");
  assert.equal(surfaces.frame.hidden, true, "a pdf was put in the iframe");
  assert.equal(surfaces.frame.getAttribute("src"), null);
});

/**
 * A path is not an extension. `evil.svg.md` ends in `.md` and must be read as
 * markdown; `notes.md.svg` ends in `.svg` and must not.
 */
test("the mode comes from the last extension, not from any extension present", () => {
  assert.equal(viewMode("evil.svg.md"), "markdown");
  assert.equal(viewMode("notes.md.svg"), "sandboxed");
  assert.equal(viewMode("archive.html.txt"), "text");
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
