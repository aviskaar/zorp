/**
 * The "While you were away" card.
 *
 * Dreaming mode looks over how a person has worked and comes back with
 * suggestions: a flavor for a request they keep making, a project for threads
 * that are about one thing, a saved fix for a command that keeps failing. Each
 * is a proposal, backed by the exact conversations it came from, and does
 * nothing until the person accepts it (ADR 0144).
 *
 * **Everything here goes through `textContent`.** The `wording` of a
 * suggestion is the one model-authored string on the page, and detection ran
 * over the person's own conversations, so a suggestion can carry whatever an
 * old thread carried. Assembling markup out of it would be the stored
 * cross-site scripting hole `memory-note.ts` and `markdown.ts` both refuse.
 * There is no `innerHTML` in this file and there must never be one.
 *
 * It is a reader with two buttons, not a model surface: nothing here starts a
 * turn, and accepting a suggestion creates an ordinary project or an untrusted
 * flavor through the same route a person's own click would.
 */

import {
  acceptDream,
  dismissDream,
  listDreams,
  runDreamPass,
  type DreamSuggestion,
} from "./api.ts";

/** The DOM the view drives. All of it already exists in `index.html`. */
export interface DreamsDom {
  section: HTMLElement;
  runButton: HTMLButtonElement;
  status: HTMLElement;
  list: HTMLElement;
}

/**
 * A one-line, code-written lead-in for a kind, so a reader can tell the kinds
 * apart at a glance without trusting the model-authored wording to say which
 * it is. The wording follows, labelled and through `textContent`.
 */
function leadFor(kind: string): string {
  switch (kind) {
    case "recurring_request":
      return "A request you keep making";
    case "unfiled_cluster":
      return "Conversations that look like one topic";
    case "repeated_failure":
      return "A command that keeps failing";
    default:
      return "A suggestion";
  }
}

/** A short, code-derived label for the button that accepts this kind. */
function acceptLabel(kind: string): string {
  switch (kind) {
    case "unfiled_cluster":
      return "File them";
    default:
      return "Save a flavor";
  }
}

function el(doc: Document, tag: string, className = ""): HTMLElement {
  const node = doc.createElement(tag);
  if (className) {
    node.className = className;
  }
  return node;
}

function text(doc: Document, tag: string, className: string, value: string): HTMLElement {
  const node = el(doc, tag, className);
  node.textContent = value;
  return node;
}

/**
 * Build one suggestion's row. `onOpen` opens a cited conversation; `onAct`
 * runs after an accept or dismiss so the card can refresh.
 */
export function suggestionItem(
  doc: Document,
  suggestion: DreamSuggestion,
  onOpen: (conversationId: string) => void,
  onAct: () => void,
): HTMLElement {
  const item = el(doc, "li", "dream-item");
  item.append(text(doc, "p", "dream-lead", leadFor(suggestion.kind)));
  // The model-authored line, labelled by its class so it can be styled as a
  // suggestion and never as the person's own words.
  item.append(text(doc, "p", "dream-wording", suggestion.wording));

  const evidence = Array.isArray(suggestion.evidence_ids) ? suggestion.evidence_ids : [];
  if (evidence.length) {
    const links = el(doc, "div", "dream-evidence");
    links.append(text(doc, "span", "dream-evidence-label", "From:"));
    evidence.forEach((id, index) => {
      const button = el(doc, "button", "dream-open") as HTMLButtonElement;
      button.type = "button";
      button.dataset.id = id;
      const label = `conversation ${index + 1}`;
      button.setAttribute("aria-label", `Open ${label}`);
      button.textContent = label;
      button.addEventListener("click", () => onOpen(id));
      links.append(button);
    });
    item.append(links);
  }

  const actions = el(doc, "div", "dream-actions");
  const accept = el(doc, "button", "dream-accept") as HTMLButtonElement;
  accept.type = "button";
  accept.textContent = acceptLabel(suggestion.kind);
  const dismiss = el(doc, "button", "dream-dismiss") as HTMLButtonElement;
  dismiss.type = "button";
  dismiss.textContent = "Dismiss";

  const feedback = text(doc, "span", "dream-feedback", "");

  const settle = (message: string, failed: boolean) => {
    feedback.textContent = message;
    feedback.classList.toggle("dream-feedback-fail", failed);
    if (!failed) {
      accept.disabled = true;
      dismiss.disabled = true;
    } else {
      accept.disabled = false;
      dismiss.disabled = false;
    }
  };

  accept.addEventListener("click", () => {
    accept.disabled = true;
    dismiss.disabled = true;
    acceptDream(suggestion.id)
      .then((result) => {
        if (result.created === "project") {
          settle(`Filed ${result.filed ?? 0} conversation(s).`, false);
        } else {
          settle("Saved a flavor. It is untrusted until you trust it in Agents.", false);
        }
        onAct();
      })
      .catch((error: unknown) => settle(messageOf(error), true));
  });

  dismiss.addEventListener("click", () => {
    accept.disabled = true;
    dismiss.disabled = true;
    dismissDream(suggestion.id)
      .then(() => {
        settle("Dismissed.", false);
        onAct();
      })
      .catch((error: unknown) => settle(messageOf(error), true));
  });

  actions.append(accept, dismiss, feedback);
  item.append(actions);
  return item;
}

/** A thrown request failure's message, always a string for `textContent`. */
function messageOf(error: unknown): string {
  if (error instanceof Error && error.message) {
    return error.message;
  }
  return "That did not work. Try again.";
}

/**
 * The card. `refresh()` fetches the open suggestions and redraws; it hides the
 * whole section when there is nothing to show and the automatic pass is on, so
 * an idle machine shows no empty card. When the pass is off it keeps the
 * "Look now" button visible so a person can run one.
 */
export class DreamsView {
  private readonly doc: Document;
  private readonly dom: DreamsDom;
  private readonly onOpen: (conversationId: string) => void;

  constructor(dom: DreamsDom, onOpen: (conversationId: string) => void) {
    this.dom = dom;
    this.onOpen = onOpen;
    this.doc = dom.section.ownerDocument;
    this.dom.runButton.addEventListener("click", () => {
      this.runPass();
    });
  }

  async refresh(): Promise<void> {
    let result;
    try {
      result = await listDreams();
    } catch {
      // A server that cannot answer is not an error the sidebar should shout
      // about; the card simply stays hidden.
      this.dom.section.hidden = true;
      return;
    }
    this.render(result.suggestions, result.enabled);
  }

  private render(suggestions: DreamSuggestion[], enabled: boolean): void {
    this.dom.list.replaceChildren();
    const has = suggestions.length > 0;
    // Hide the whole card only when there is nothing to show and the pass is
    // on anyway, so a person with the pass off can always find "Look now".
    this.dom.section.hidden = !has && enabled;
    if (!has) {
      this.dom.status.textContent = enabled
        ? "Nothing to suggest yet."
        : "Dreaming is off. Look now to find suggestions from your conversations.";
      return;
    }
    this.dom.status.textContent = "";
    for (const suggestion of suggestions) {
      this.dom.list.append(
        suggestionItem(this.doc, suggestion, this.onOpen, () => {
          void this.refresh();
        }),
      );
    }
  }

  private runPass(): void {
    this.dom.runButton.disabled = true;
    this.dom.status.textContent = "Looking over your conversations…";
    runDreamPass()
      .then((result) => {
        this.dom.runButton.disabled = false;
        if (result.recorded === 0) {
          this.dom.status.textContent = "Nothing new to suggest.";
        }
        void this.refresh();
      })
      .catch((error: unknown) => {
        this.dom.runButton.disabled = false;
        this.dom.status.textContent = messageOf(error);
      });
  }
}
