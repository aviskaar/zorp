/**
 * The directory the agent works in, and the chooser for it.
 *
 * zorp-web used to run every agent in the directory the server was started
 * in, which for anyone running from source is zorp's own tree, so every
 * file the agent wrote landed in this repo. The server now works in a
 * directory the person picks. This is where they pick it, and the top bar
 * label that says which one is in use.
 *
 * **Everything reaching the page goes through `textContent`.** Directory
 * names come off a filesystem, so they are somebody else's strings the same
 * way a model id is: a directory can be called anything a filesystem allows,
 * including a tag. There is no `innerHTML` in this file and there must never
 * be one.
 *
 * **Nothing here starts a turn.** It reads and writes one setting, the way
 * `onboarding.ts` reads and writes the model ones.
 */

import {
  browseWorkspace,
  getWorkspace,
  setWorkspace,
  NO_WORKSPACE,
  type Workspace,
  type WorkspaceListing,
} from "./api.ts";

/**
 * Why the picker opened by itself.
 *
 * One sentence, because it interrupted somebody who pressed send and is
 * owed a reason rather than a dialog that just appeared.
 */
export const WORKSPACE_REASON =
  "That message needs a directory to work in and none is set. Pick one and send it again.";

/**
 * Whether a failed start failed for want of a workspace.
 *
 * Reads the message rather than the status, because 409 is also what a busy
 * session answers with. The message may have arrived on the event stream or
 * been dressed up by the error formatter, so this looks for the server's
 * sentence inside it rather than for an exact match.
 */
export function needsWorkspace(message: string): boolean {
  return message.includes(NO_WORKSPACE);
}

/** The last segment of an absolute path. `/` is its own last segment. */
export function lastSegment(path: string): string {
  const trimmed = path.replace(/\/+$/, "");
  if (!trimmed) {
    return "/";
  }
  const cut = trimmed.lastIndexOf("/");
  return cut === -1 ? trimmed : trimmed.slice(cut + 1) || "/";
}

/** What the top bar button says, and what its tooltip says. */
export interface BarLabel {
  label: string;
  title: string;
  /** False when there is no workspace, which is what makes the button loud. */
  set: boolean;
}

/**
 * The top bar button's text.
 *
 * The last segment and not the whole path: a full path eats the toolbar,
 * and the whole of it is one hover away in the tooltip. Nothing is cut
 * here; a long segment is truncated by the same CSS that truncates a long
 * model id.
 */
export function workspaceBar(workspace: Workspace | null): BarLabel {
  if (!workspace?.path) {
    return {
      label: "No workspace",
      title: "No directory is set, so the agent has nowhere to work. Click to choose one.",
      set: false,
    };
  }
  return { label: lastSegment(workspace.path), title: workspace.path, set: true };
}

/**
 * Where generated files go, in one sentence.
 *
 * The path is the server's `scratch` field and is never joined together
 * here. Before any workspace exists there is no path to name, so the
 * sentence says what will happen without claiming to know where.
 */
export function scratchLine(workspace: Workspace | null): string {
  return workspace?.scratch
    ? `Files the agent generates, PDFs included, go in ${workspace.scratch}.`
    : "Files the agent generates, PDFs included, go in a scratch directory inside the workspace.";
}

/** Unique enough to tie a label to its field when two pickers are on a page. */
let pickerSeq = 0;

/**
 * The chooser: where you are, where you can go, and a field for typing a
 * path nobody wants to click their way to.
 *
 * The header, the field and the button all show one value, `path`. Editing
 * the field moves it, clicking a directory moves it, and Save sends it. One
 * value means the button can never send something other than what the
 * person is looking at.
 */
export class WorkspacePicker {
  private readonly doc: Document;
  private readonly onSaved: (workspace: Workspace) => void;
  private readonly reason: HTMLElement;
  private readonly lead: HTMLElement;
  private readonly here: HTMLElement;
  private readonly list: HTMLElement;
  private readonly field: HTMLInputElement;
  private readonly result: HTMLElement;
  private readonly save: HTMLButtonElement;
  private path = "";

  constructor(doc: Document, host: HTMLElement, onSaved: (workspace: Workspace) => void) {
    this.doc = doc;
    this.onSaved = onSaved;

    const id = `ws-path-${(pickerSeq += 1)}`;
    this.reason = this.node("p", "ws-reason");
    this.reason.hidden = true;
    this.lead = this.node("p", "onboard-lead ws-lead");
    this.here = this.node("p", "ws-here");
    this.list = this.node("ul", "ws-list");
    this.result = this.node("p", "settings-result ws-result");

    const field = this.doc.createElement("div");
    field.className = "settings-field";
    const label = this.doc.createElement("label");
    label.htmlFor = id;
    label.textContent = "Directory";
    this.field = this.doc.createElement("input");
    this.field.id = id;
    this.field.type = "text";
    this.field.autocomplete = "off";
    this.field.spellcheck = false;
    this.field.placeholder = "/an/absolute/path";
    field.append(label, this.field);

    const actions = this.doc.createElement("div");
    actions.className = "settings-actions";
    this.save = this.doc.createElement("button");
    this.save.type = "button";
    this.save.className = "btn btn-allow ws-save";
    this.save.textContent = "Work here";
    actions.append(this.save);

    const root = this.doc.createElement("div");
    root.className = "ws-picker";
    root.append(this.reason, this.lead, this.here, this.list, field, this.result, actions);
    host.replaceChildren(root);

    this.field.addEventListener("input", () => this.setPath(this.field.value, false));
    this.field.addEventListener("keydown", (event) => {
      if ((event as KeyboardEvent).key !== "Enter") {
        return;
      }
      event.preventDefault();
      void this.browse(this.path);
    });
    this.save.addEventListener("click", () => void this.commit());
  }

  /**
   * Show the picker's contents for wherever the server is working now.
   *
   * `reason` is the sentence explaining an opening nobody asked for. Empty
   * means the person clicked the button and needs no explanation.
   */
  async open(reason = ""): Promise<void> {
    this.reason.textContent = reason;
    this.reason.hidden = reason === "";
    this.setResult("", null);
    let current: Workspace | null = null;
    try {
      current = await getWorkspace();
    } catch {
      // Not knowing the current one is no reason to refuse to pick a new
      // one. The lead falls back to the sentence that names no path.
    }
    this.lead.textContent = `The agent reads and writes inside this directory and nowhere else. ${scratchLine(current)}`;
    await this.browse(current?.path ?? null);
  }

  /** List one directory, or the home directory when given nothing. */
  private async browse(path: string | null): Promise<void> {
    try {
      const listing = await browseWorkspace(path ?? undefined);
      this.show(listing);
      this.setResult("", null);
    } catch (error) {
      // The listing that is up stays up, so a typo in the field leaves the
      // person where they were rather than in an empty picker.
      this.setResult(sentence(error), "fail");
    }
  }

  private show(listing: WorkspaceListing): void {
    this.setPath(listing.path, true);
    this.list.replaceChildren();
    if (listing.parent !== null) {
      this.list.append(this.row("Parent directory", listing.parent, "ws-row ws-up"));
    }
    for (const entry of listing.entries) {
      this.list.append(this.row(entry.name, entry.path, "ws-row"));
    }
  }

  private row(text: string, path: string, className: string): HTMLElement {
    const item = this.doc.createElement("li");
    const button = this.doc.createElement("button");
    button.type = "button";
    button.className = className;
    button.textContent = text;
    button.title = path;
    button.addEventListener("click", () => void this.browse(path));
    item.append(button);
    return item;
  }

  /** Move the one value the header, the field and Save all read. */
  private setPath(path: string, intoField: boolean): void {
    this.path = path;
    this.here.textContent = path;
    if (intoField) {
      this.field.value = path;
    }
  }

  /** Send the shown directory. A refusal stays on screen and can be retried. */
  private async commit(): Promise<void> {
    const wanted = this.path.trim();
    if (!wanted) {
      this.setResult("Pick a directory first.", "fail");
      return;
    }
    this.save.disabled = true;
    this.setResult("Saving...", null);
    try {
      const saved = await setWorkspace(wanted);
      this.setResult("", null);
      this.onSaved(saved);
    } catch (error) {
      this.setResult(sentence(error), "fail");
    } finally {
      this.save.disabled = false;
    }
  }

  private setResult(text: string, state: "ok" | "fail" | null): void {
    this.result.textContent = text;
    if (state) {
      this.result.dataset.state = state;
    } else {
      delete this.result.dataset.state;
    }
  }

  private node(tag: string, className: string): HTMLElement {
    const node = this.doc.createElement(tag);
    node.className = className;
    return node;
  }
}

/**
 * What the server said, and nothing added to it.
 *
 * The refusal endpoints answer with a plain sentence written for a reader,
 * so it is shown as it came. Dressing it up with a status code would bury
 * the one part that tells somebody what to do next.
 */
function sentence(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
