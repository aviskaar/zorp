/**
 * What the artifact pane shows, and how.
 *
 * This file exists to keep one decision in one place: whether a file goes on
 * the page or into a frame. The pane shows `.svg` and `.html`, which are
 * documents that execute. They are only safe because they load inside the
 * sandbox frame, whose response the server sends with a bare
 * `Content-Security-Policy: sandbox` and `X-Content-Type-Options: nosniff`.
 * That puts them in a unique origin with scripting off, so script inside them
 * neither runs nor gets a handle on this page.
 *
 * A PDF goes into a second frame, and the difference between the two frames
 * is the point of having two. A PDF is drawn by the browser's own viewer,
 * which is itself a scripted document and does not start under a policy that
 * strict: pointing the sandbox frame at one is what the previous attempt did,
 * and it showed a broken-document icon on grey. So the server sends a PDF
 * under `sandbox allow-scripts`, one token wider and no more, and that frame
 * carries no `sandbox` attribute of its own because every value of it,
 * `allow-scripts` included, stops the viewer starting. Without
 * `allow-same-origin` the document is still in an opaque origin: measured in
 * Chrome 151, `parent.document`, `parent.location` and `localStorage` all
 * throw `SecurityError` from inside such a frame and `window.origin` is
 * `null`.
 *
 * The rule that follows, and the reason for the tests next to it: nothing
 * that can execute may ever reach this page's own DOM. Not through
 * `innerHTML`, not through `srcdoc`, not through an inline `<svg>` element.
 * `markdown.ts` goes to great lengths never to assemble markup, and inlining
 * one hostile SVG would make all of that beside the point. A PDF is no
 * exception: it is never fetched into this page, only addressed by URL.
 */

/** How a file is put on screen. */
export type ViewMode = "sandboxed" | "pdf" | "image" | "markdown" | "text";

/**
 * The markdown renderer, passed in rather than imported.
 *
 * The same reason `StreamedMessage` takes it: this module is about the
 * display rules, and a test that a `.svg` never reaches the page should not
 * have to drag the renderer's module graph along to say so.
 */
export type Render = (target: HTMLElement, source: string) => void;

/** The five surfaces the pane can use. Exactly one is visible at a time. */
export interface Pane {
  /** The page's own DOM. Only ever receives nodes this code built. */
  doc: HTMLElement;
  /** The sandbox. Everything that can execute goes here and nowhere else. */
  frame: HTMLIFrameElement;
  /** The PDF surface, whose isolation comes from the response, not the
   * element. See the note at the top of this file. */
  pdf: HTMLIFrameElement;
  image: HTMLImageElement;
  empty: HTMLElement;
}

/**
 * Extensions that go into the sandbox.
 *
 * Both are active documents: an SVG is XML that can carry a `<script>`, and
 * an HTML file needs no explanation. A PDF is framed too, but not here and
 * not under this policy; see the note at the top of this file.
 */
const SANDBOXED = ["svg", "html"];
const IMAGES = ["png", "jpg", "jpeg", "gif", "webp"];
/** The server reads these and sends markdown, so the file itself never
 * reaches the browser at all. */
const EXTRACTED = ["docx", "odt", "xlsx", "pptx"];
const MARKDOWN = ["md", "markdown", ...EXTRACTED];

/** The last extension, lowercased. `notes.md.svg` is an svg. */
function extensionOf(path: string): string {
  const name = path.split("/").pop() ?? "";
  const dot = name.lastIndexOf(".");
  return dot === -1 ? "" : name.slice(dot + 1).toLowerCase();
}

/**
 * Whether this browser will draw a PDF if handed one.
 *
 * Asked rather than assumed, and answered `false` unless the browser says
 * `true` outright. A browser too old to have been asked this question is a
 * browser that gets the text, which is what the pane did for every browser
 * until now and is never an empty pane. The property is what iOS Safari says
 * no with, and iOS Safari really does not have a viewer to frame.
 */
export function pdfViewerAvailable(): boolean {
  return typeof navigator !== "undefined" && navigator.pdfViewerEnabled === true;
}

/**
 * How to show one file.
 *
 * `canRenderPdf` is a parameter and not a lookup, so the rule is one function
 * of two inputs and a test can ask about either browser without pretending to
 * be one.
 */
export function viewMode(path: string, canRenderPdf = pdfViewerAvailable()): ViewMode {
  const ext = extensionOf(path);
  // Sandboxed is checked first, and deliberately so: an extension that
  // executes must never fall through to a branch that renders it on the page.
  if (SANDBOXED.includes(ext)) {
    return "sandboxed";
  }
  if (ext === "pdf") {
    // The text is the fallback and it is a real one: a browser with no viewer
    // still gets the words out of the file rather than an empty pane.
    return canRenderPdf ? "pdf" : "markdown";
  }
  if (IMAGES.includes(ext)) {
    return "image";
  }
  if (MARKDOWN.includes(ext)) {
    return "markdown";
  }
  return "text";
}

/** True when opening this file needs its bytes fetched into the page. */
export function needsText(path: string, canRenderPdf = pdfViewerAvailable()): boolean {
  const mode = viewMode(path, canRenderPdf);
  return mode === "markdown" || mode === "text";
}

/**
 * Put one file in the pane.
 *
 * `text` is the file's contents when they were fetched, and null when they
 * were not. A sandboxed or image file ignores it entirely: those are
 * addressed by URL so the browser fetches them into a context this page does
 * not control, which is the whole point.
 */
export function showArtifact(
  pane: Pane,
  path: string,
  rawUrl: string,
  text: string | null,
  render: Render,
  canRenderPdf = pdfViewerAvailable(),
): void {
  const mode = viewMode(path, canRenderPdf);

  if (mode === "sandboxed") {
    pane.doc.replaceChildren();
    pane.doc.hidden = true;
    pane.image.hidden = true;
    pane.image.removeAttribute("src");
    pane.empty.hidden = true;
    hidePdf(pane);
    // src, never srcdoc. srcdoc would make this page the document's author
    // and hand it whatever the parent grants; src makes the browser go and
    // fetch it, so the server's sandbox header is what governs it.
    pane.frame.removeAttribute("srcdoc");
    pane.frame.src = rawUrl;
    pane.frame.hidden = false;
    return;
  }

  if (mode === "pdf") {
    pane.doc.replaceChildren();
    pane.doc.hidden = true;
    pane.image.hidden = true;
    pane.image.removeAttribute("src");
    pane.empty.hidden = true;
    hideFrame(pane);
    // src for the same reason, and doubly so here: the bytes must reach the
    // browser's viewer without passing through this page at all. srcdoc would
    // mean fetching a PDF into this origin and handing it back, which is the
    // one thing this design is arranged to avoid.
    pane.pdf.removeAttribute("srcdoc");
    pane.pdf.src = rawUrl;
    pane.pdf.hidden = false;
    return;
  }

  if (mode === "image") {
    pane.doc.replaceChildren();
    pane.doc.hidden = true;
    hideFrame(pane);
    hidePdf(pane);
    pane.empty.hidden = true;
    pane.image.src = rawUrl;
    pane.image.alt = path;
    pane.image.hidden = false;
    return;
  }

  hideFrame(pane);
  hidePdf(pane);
  pane.image.hidden = true;
  pane.image.removeAttribute("src");
  pane.empty.hidden = true;
  pane.doc.replaceChildren();
  const body = text ?? "";
  if (mode === "markdown") {
    render(pane.doc, body);
  } else {
    const block = document.createElement("pre");
    block.className = "code-block";
    const code = document.createElement("code");
    // textContent, as everywhere else in this UI. A file the agent wrote is
    // model output by another name.
    code.textContent = body;
    block.append(code);
    pane.doc.append(block);
  }
  pane.doc.hidden = false;
}

/** Blank the iframe as well as hiding it, so a hidden frame holds nothing. */
function hideFrame(pane: Pane): void {
  pane.frame.hidden = true;
  pane.frame.removeAttribute("src");
  pane.frame.removeAttribute("srcdoc");
}

/** The same for the PDF frame. A hidden viewer is still a loaded viewer. */
function hidePdf(pane: Pane): void {
  pane.pdf.hidden = true;
  pane.pdf.removeAttribute("src");
  pane.pdf.removeAttribute("srcdoc");
}

/**
 * The URL to fetch a file's readable text from.
 *
 * Only a PDF has two answers at that path, and `as=text` is how the pane asks
 * for the words rather than the file. Harmless on everything else, where the
 * server has no second answer to give and ignores it.
 */
export function textUrl(rawUrl: string): string {
  return rawUrl + (rawUrl.includes("?") ? "&" : "?") + "as=text";
}

/* ------------------------------------------------------------------ */
/* noticing what a run produced                                        */
/* ------------------------------------------------------------------ */

/** The part of a listing entry that says whether a run touched it. */
export interface ArtifactStamp {
  path: string;
  modified_ms: number;
}

/**
 * Which files a run produced, newest first.
 *
 * The listing is the only input, on purpose. A tool summary cannot answer
 * this: a PDF written by pandoc under `run_command` and one written by
 * `write_file` are the same event as far as the workspace is concerned, and
 * only one of them names a path anywhere. Diffing the directory catches both
 * and catches whatever the next way of writing a file turns out to be.
 *
 * With no snapshot to compare against, the answer is "nothing". Treating
 * every file as new the first time would badge the button on page load for
 * files that have been sitting there for a week.
 */
export function producedSince(
  before: readonly ArtifactStamp[] | null,
  after: readonly ArtifactStamp[],
): ArtifactStamp[] {
  if (!before) {
    return [];
  }
  const was = new Map(before.map((file) => [file.path, file.modified_ms]));
  return after
    .filter((file) => {
      const previous = was.get(file.path);
      return previous === undefined || file.modified_ms > previous;
    })
    .slice()
    .sort((a, b) => b.modified_ms - a.modified_ms);
}
