/**
 * What the artifact pane shows, and how.
 *
 * This file exists to keep one decision in one place: whether a file goes on
 * the page or into the iframe. The pane now shows `.svg` and `.html`, which
 * are documents that execute. They are only safe because they load inside the
 * iframe, whose response the server sends with a bare
 * `Content-Security-Policy: sandbox` and `X-Content-Type-Options: nosniff`.
 * That puts them in a unique origin with scripting off, so script inside them
 * neither runs nor gets a handle on this page.
 *
 * The rule that follows, and the reason for the tests next to it: nothing
 * that can execute may ever reach this page's own DOM. Not through
 * `innerHTML`, not through `srcdoc`, not through an inline `<svg>` element.
 * `markdown.ts` goes to great lengths never to assemble markup, and inlining
 * one hostile SVG would make all of that beside the point.
 */

/** How a file is put on screen. */
export type ViewMode = "sandboxed" | "image" | "markdown" | "text";

/**
 * The markdown renderer, passed in rather than imported.
 *
 * The same reason `StreamedMessage` takes it: this module is about the
 * display rules, and a test that a `.svg` never reaches the page should not
 * have to drag the renderer's module graph along to say so.
 */
export type Render = (target: HTMLElement, source: string) => void;

/** The four surfaces the pane can use. Exactly one is visible at a time. */
export interface Pane {
  /** The page's own DOM. Only ever receives nodes this code built. */
  doc: HTMLElement;
  /** The sandbox. Everything that can execute goes here and nowhere else. */
  frame: HTMLIFrameElement;
  image: HTMLImageElement;
  empty: HTMLElement;
}

/**
 * Extensions that go into the sandbox.
 *
 * A PDF has been here since the pane existed. `.svg` and `.html` join it
 * because both are active documents: an SVG is XML that can carry a
 * `<script>`, and an HTML file needs no explanation.
 */
const SANDBOXED = ["pdf", "svg", "html"];
const IMAGES = ["png", "jpg", "jpeg", "gif", "webp"];
/** The server extracts these to markdown before they leave it. */
const EXTRACTED = ["docx", "odt", "xlsx", "pptx"];
const MARKDOWN = ["md", "markdown", ...EXTRACTED];

/** The last extension, lowercased. `notes.md.svg` is an svg. */
function extensionOf(path: string): string {
  const name = path.split("/").pop() ?? "";
  const dot = name.lastIndexOf(".");
  return dot === -1 ? "" : name.slice(dot + 1).toLowerCase();
}

export function viewMode(path: string): ViewMode {
  const ext = extensionOf(path);
  // Sandboxed is checked first, and deliberately so: an extension that
  // executes must never fall through to a branch that renders it on the page.
  if (SANDBOXED.includes(ext)) {
    return "sandboxed";
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
export function needsText(path: string): boolean {
  const mode = viewMode(path);
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
): void {
  const mode = viewMode(path);

  if (mode === "sandboxed") {
    pane.doc.replaceChildren();
    pane.doc.hidden = true;
    pane.image.hidden = true;
    pane.image.removeAttribute("src");
    pane.empty.hidden = true;
    // src, never srcdoc. srcdoc would make this page the document's author
    // and hand it whatever the parent grants; src makes the browser go and
    // fetch it, so the server's sandbox header is what governs it.
    pane.frame.removeAttribute("srcdoc");
    pane.frame.src = rawUrl;
    pane.frame.hidden = false;
    return;
  }

  if (mode === "image") {
    pane.doc.replaceChildren();
    pane.doc.hidden = true;
    hideFrame(pane);
    pane.empty.hidden = true;
    pane.image.src = rawUrl;
    pane.image.alt = path;
    pane.image.hidden = false;
    return;
  }

  hideFrame(pane);
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
