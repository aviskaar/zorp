/**
 * The workspace listing, as folders rather than one long column of paths.
 *
 * The server sends a flat list sorted by path, which reads fine for a
 * handful of files and not at all for a run that wrote two hundred images
 * into one directory: every row starts with the same prefix, the part that
 * differs is the part the pane elides, and the folder they share is never
 * said out loud. Grouping puts the shared prefix on one line and leaves the
 * leaf with the only name that tells the rows apart.
 *
 * A folder is a native `details`, the same choice `activity-group.ts` made.
 * It collapses without a line of state, it is keyboard reachable already,
 * and the browser knows what to do with it.
 *
 * This module groups and builds; it decides nothing about what a file row
 * is. `main.ts` passes the row in, so the click, the open mark and the
 * "new" badge stay where they were.
 *
 * Its own module, like `activity-group.ts`, because `main.ts` runs the whole
 * app on import and cannot be loaded from a test.
 */

/** The one field this module reads. Callers pass their own richer type. */
export interface HasPath {
  path: string;
}

export interface Folder<T extends HasPath> {
  /** The last segment, or "" for the root. */
  name: string;
  /** The whole path to this folder, or "" for the root. */
  path: string;
  folders: Folder<T>[];
  /** The files directly in this folder, in the order they arrived. */
  files: T[];
  /** Files at or under this folder. */
  count: number;
}

function emptyFolder<T extends HasPath>(name: string, path: string): Folder<T> {
  return { name, path, folders: [], files: [], count: 0 };
}

/**
 * Group a flat listing by folder, keeping the order it came in.
 *
 * The server already sorted by path and that is the order the pane showed
 * before, so nothing here re-sorts: a caller who changes the server's mind
 * about ordering should not have to change this too. A path with no
 * separator is a file at the root, and a leading `./` or `/` is stripped so
 * two spellings of one directory do not become two folders.
 */
export function groupByFolder<T extends HasPath>(files: T[]): Folder<T> {
  const root = emptyFolder<T>("", "");
  for (const file of files) {
    const trimmed = file.path.replace(/^\.\//, "").replace(/^\/+/, "");
    const segments = trimmed.split("/").filter((s) => s.length > 0);
    // A path that was nothing but separators names no file. Dropping it
    // beats inventing a folder with an empty name for it to live in.
    if (segments.length === 0) {
      continue;
    }
    // The leaf is dropped from the walk; the row carries the whole path.
    segments.pop();
    let here = root;
    here.count += 1;
    for (const segment of segments) {
      let next = here.folders.find((f) => f.name === segment);
      if (!next) {
        next = emptyFolder<T>(segment, here.path ? `${here.path}/${segment}` : segment);
        here.folders.push(next);
      }
      next.count += 1;
      here = next;
    }
    // The row still carries the file's own path; only the label is the leaf
    // name, so nothing downstream has to reassemble anything.
    here.files.push(file);
  }
  return root;
}

/** The leaf name a row shows, which is everything after the last separator. */
export function leafName(path: string): string {
  const trimmed = path.replace(/\/+$/, "");
  const cut = trimmed.lastIndexOf("/");
  return cut === -1 ? trimmed : trimmed.slice(cut + 1);
}

/** How the summary says how much is inside. */
export function countLabel(count: number): string {
  return count === 1 ? "1 file" : `${count} files`;
}

export interface TreeOptions<T extends HasPath> {
  /** Builds one file row. The label it should show is passed in. */
  row: (file: T, label: string) => HTMLElement;
  /**
   * Paths worth revealing: any folder holding one of them opens.
   *
   * Below the top level, folders are shut by default, because a directory
   * of two hundred images open by default is the column this replaced. What
   * must not be hidden is the file the pane is showing and the files the
   * run just wrote, so those open their ancestors and nothing else does.
   */
  reveal?: ReadonlySet<string>;
}

/**
 * The `li` children for the existing list, folders first then files.
 *
 * Folders first because a folder is a heading for the rows under it, and a
 * heading below its own contents is not one.
 */
export function artifactTree<T extends HasPath>(
  doc: Document,
  files: T[],
  options: TreeOptions<T>,
): HTMLElement[] {
  return childrenOf(doc, groupByFolder(files), options, 0);
}

function childrenOf<T extends HasPath>(
  doc: Document,
  folder: Folder<T>,
  options: TreeOptions<T>,
  depth: number,
): HTMLElement[] {
  const items: HTMLElement[] = [];
  for (const child of folder.folders) {
    items.push(folderItem(doc, child, options, depth));
  }
  for (const file of folder.files) {
    const item = doc.createElement("li");
    item.append(options.row(file, leafName(file.path)));
    items.push(item);
  }
  return items;
}

function folderItem<T extends HasPath>(
  doc: Document,
  folder: Folder<T>,
  options: TreeOptions<T>,
  depth: number,
): HTMLElement {
  const item = doc.createElement("li");
  const box = doc.createElement("details") as HTMLDetailsElement;
  box.className = "artifact-folder";
  // The top level is open, everything under it is shut. A workspace with
  // one directory in it would otherwise open to a single row naming that
  // directory, and a pane that shows nothing until you click is not an
  // improvement on one that shows too much. One step down is where the
  // hundreds of files live, and that is the step worth folding.
  box.open = depth === 0 || reveals(folder, options.reveal);
  const summary = doc.createElement("summary");
  summary.className = "artifact-folder-line";
  const marker = doc.createElement("span");
  marker.className = "artifact-folder-marker";
  marker.textContent = "▸";
  const name = doc.createElement("span");
  name.className = "artifact-folder-name";
  // The folder's own name and nothing assembled: a directory can be called
  // whatever the filesystem allows, and this pane draws model-written paths.
  name.textContent = folder.name;
  const count = doc.createElement("span");
  count.className = "artifact-folder-count";
  count.textContent = countLabel(folder.count);
  summary.append(marker, name, count);
  box.append(summary);
  const list = doc.createElement("ul");
  list.className = "artifact-sublist";
  list.append(...childrenOf(doc, folder, options, depth + 1));
  box.append(list);
  item.append(box);
  return item;
}

function reveals<T extends HasPath>(
  folder: Folder<T>,
  reveal: ReadonlySet<string> | undefined,
): boolean {
  if (!reveal || reveal.size === 0) {
    return false;
  }
  if (folder.files.some((file) => reveal.has(file.path))) {
    return true;
  }
  return folder.folders.some((child) => reveals(child, reveal));
}
