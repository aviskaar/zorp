/**
 * A file the answer names, made openable.
 *
 * An answer that ends "the PDF is at `report.pdf`" names something the run
 * wrote, and until this existed the only way to see it was to open the Files
 * pane and find that name in the list by hand. This turns the reference into
 * a button that opens the same pane on the same file.
 *
 * The listing is the only thing that decides what is a file. A code span the
 * listing does not know stays plain text, so `ClusterIP` in an answer about
 * Kubernetes is still a code span, and a path the model invented points at
 * nothing. That is also the containment: a name the model wrote can only ever
 * reach a file the workspace has already reported, so there is no reference
 * here that leads out of the workspace.
 *
 * Everything lands through `textContent` and `dataset`, for the reason
 * `markdown.ts` gives at length: a path is a name the model chose, which is
 * model output by another route. Nothing here assembles markup and nothing
 * here reads a file. How a file is displayed stays `artifact-view.ts`'s
 * decision, untouched: this only says which path to open.
 */

/** What the button says when a pointer rests on it. */
export const FILE_LINK_TITLE = "Opens in the file pane";

/** The listing, in the two shapes a lookup needs. */
interface Index {
  /** Every listed path, exactly as listed. */
  paths: Set<string>;
  /** A basename to its one path, or null when more than one file has it. */
  byName: Map<string, string | null>;
}

function indexOf(paths: Iterable<string>): Index {
  const known: Index = { paths: new Set(), byName: new Map() };
  for (const path of paths) {
    known.paths.add(path);
    const name = path.split("/").pop() ?? path;
    const seen = known.byName.get(name);
    known.byName.set(name, seen === undefined || seen === path ? path : null);
  }
  return known;
}

/**
 * The listed path a reference names, or null when it names none.
 *
 * Exact first, then the basename and only when exactly one file carries it.
 * An ambiguous basename resolves to nothing rather than to a guess, because
 * a guess here opens the wrong document and looks like it worked.
 */
function resolveFile(reference: string, known: Index): string | null {
  const ref = reference.trim().replace(/^\.\//, "");
  if (known.paths.has(ref)) {
    return ref;
  }
  // A leading slash is how a markdown link writes a path the workspace holds
  // without one. The listing has no absolute paths, so this is the only way
  // such a link matches exactly.
  const bare = ref.replace(/^\/+/, "");
  if (known.paths.has(bare)) {
    return bare;
  }
  const name = bare.split("/").pop() ?? "";
  return name ? known.byName.get(name) ?? null : null;
}

/** A scheme, a protocol relative URL, or a fragment: not a path in here. */
function isRelativePath(href: string): boolean {
  return !/^[a-z][a-z0-9+.-]*:/i.test(href) && !href.startsWith("//") && !href.startsWith("#");
}

/**
 * Upgrade every reference in `root` that names a listed file.
 *
 * Safe to run over the same subtree again, which it is: an answer can name a
 * file before the listing has caught up, so the caller runs this once when
 * the message is drawn and again whenever the listing changes.
 */
export function linkFiles(
  root: HTMLElement,
  paths: Iterable<string>,
  open: (path: string) => void,
): void {
  const known = indexOf(paths);
  if (known.paths.size === 0) {
    return;
  }
  for (const span of root.querySelectorAll<HTMLElement>("code.inline-code")) {
    upgrade(span, span.textContent ?? "", known, open);
  }
  for (const anchor of root.querySelectorAll<HTMLAnchorElement>("a[href]")) {
    const href = anchor.getAttribute("href") ?? "";
    // Without this an external link whose last segment happened to match a
    // listed name would be captured and never reach the site it named.
    if (!isRelativePath(href)) {
      continue;
    }
    upgrade(anchor, href, known, open);
  }
}

/**
 * Put a button where the reference was.
 *
 * Replacing rather than decorating is what stops a second pass wrapping the
 * same reference twice: once it is a button there is no code span and no
 * anchor left to find. A real button, so the keyboard reaches it and a screen
 * reader says what it is, and the click is handled here so an anchor that
 * matched never navigates.
 */
function upgrade(
  node: Element,
  reference: string,
  known: Index,
  open: (path: string) => void,
): void {
  const path = resolveFile(reference, known);
  if (path === null) {
    return;
  }
  const button = node.ownerDocument.createElement("button");
  button.type = "button";
  button.className = "file-link";
  // The words stay as the answer wrote them. The path handed to the pane is
  // the listing's, which is the only one that names a file that exists.
  button.textContent = node.textContent || path;
  button.dataset.path = path;
  button.title = FILE_LINK_TITLE;
  button.addEventListener("click", (event) => {
    event.preventDefault();
    open(path);
  });
  node.replaceWith(button);
}
