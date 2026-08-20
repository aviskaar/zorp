/**
 * How the page shows that this session's approval gate is down.
 *
 * Auto-approve is the one setting in this UI that makes the product less
 * careful, so the rule it is held to is that it can never be on quietly. Two
 * things say so at once: a pill in the toolbar, which is on screen whatever
 * the user has scrolled to, and a banner over the composer that spells out
 * what is off and carries the switch that puts it back.
 *
 * Its own module, like `copy-response.ts`, because "is the gate visibly down"
 * is exactly the kind of thing that should be provable without a browser.
 * `main.ts` runs the whole app on import and cannot be loaded from a test.
 */

/** Toolbar pill, gate up. */
export const ASKING_LABEL = "Auto-approve off";

/** Toolbar pill, gate down. */
export const AUTO_LABEL = "Auto-approve on";

/** The elements that between them say what the session is doing. */
export interface AutoApproveView {
  button: HTMLButtonElement;
  label: HTMLElement;
  banner: HTMLElement;
  bannerOff: HTMLButtonElement;
}

/** Collect them from a document that already contains the markup. */
export function autoApproveView(doc: Document): AutoApproveView {
  const byId = <T extends HTMLElement>(id: string): T => {
    const node = doc.getElementById(id);
    if (!node) {
      throw new Error(`index.html is missing #${id}`);
    }
    return node as T;
  };
  return {
    button: byId<HTMLButtonElement>("auto-approve-btn"),
    label: byId("auto-approve-label"),
    banner: byId("auto-approve-banner"),
    bannerOff: byId<HTMLButtonElement>("auto-approve-off"),
  };
}

/**
 * Put the page into the state the server just reported.
 *
 * Called with what the server said, never with what the browser hopes. The
 * server owns this flag; a page that drew the banner from a local guess could
 * show a gate that is up as down, or worse.
 */
export function renderAutoApprove(view: AutoApproveView, on: boolean): void {
  view.button.setAttribute("aria-pressed", on ? "true" : "false");
  view.button.dataset.state = on ? "on" : "asking";
  view.label.textContent = on ? AUTO_LABEL : ASKING_LABEL;
  view.button.title = on
    ? "Tools are running without asking. Click to start asking again."
    : "Every tool that can change this machine asks first. Click to stop asking.";
  view.banner.hidden = !on;
}
