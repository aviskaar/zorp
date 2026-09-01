/**
 * Messages typed while a turn is running.
 *
 * Enter used to submit to nothing while the composer was busy: `submitMessage`
 * returned early and the message just vanished, with no sign it had been
 * read. This holds it in view instead, in the order it was typed, until the
 * run in front of it ends and it goes out as the next turn.
 *
 * Its own module, like `session-row.ts`, so the list can be built and read
 * back without a page. Every string here is a person's own typing, not model
 * output, but it still goes on through `textContent` and never `innerHTML`:
 * the one rule this UI holds everywhere, queued text included.
 */

export interface QueueView {
  container: HTMLElement;
  list: HTMLElement;
}

/** Collect the elements from a document that already contains the markup. */
export function queueView(doc: Document): QueueView {
  const byId = <T extends HTMLElement>(id: string): T => {
    const node = doc.getElementById(id);
    if (!node) {
      throw new Error(`index.html is missing #${id}`);
    }
    return node as T;
  };
  return {
    container: byId("message-queue"),
    list: byId("message-queue-list"),
  };
}

/**
 * Redraw the queue from scratch. The list is short and rebuilding it on
 * every change is simpler than patching it in place, the same trade
 * `renderSessions` makes for the sidebar.
 */
export function renderQueue(
  doc: Document,
  view: QueueView,
  messages: readonly string[],
  onRemove: (index: number) => void,
): void {
  view.list.replaceChildren();
  view.container.hidden = messages.length === 0;

  messages.forEach((text, index) => {
    const item = doc.createElement("li");
    item.className = "message-queue-item";

    const span = doc.createElement("span");
    span.className = "message-queue-text";
    span.textContent = text;

    const remove = doc.createElement("button");
    remove.type = "button";
    remove.className = "message-queue-remove";
    remove.setAttribute("aria-label", `Remove queued message: ${text}`);
    remove.textContent = "×";
    remove.addEventListener("click", () => onRemove(index));

    item.append(span, remove);
    view.list.append(item);
  });
}
