/**
 * How the page shows that the agent's shell commands run under an OS-level
 * sandbox.
 *
 * Like the web-search pill beside it, this is a report and not a switch. The
 * mode is chosen in Settings (or in the environment); this only draws what
 * `GET /api/capabilities` said. `off` draws nothing, because an unsandboxed
 * agent is the default and a pill for "no wall" would be noise. A mode that
 * is on but that `srt` cannot actually run is drawn with a warning in the
 * tooltip, because a command under it will refuse and the person should know
 * before they run one.
 *
 * Everything reaching the page goes through `textContent` and attribute
 * values. This module builds no HTML strings, for the same reason
 * `markdown.ts` does not: the `detail` comes from a tool's own output.
 */

import type { SandboxCapability } from "./api.ts";

/** The elements the indicator writes into. */
export interface SandboxIndicatorView {
  root: HTMLElement;
  text: HTMLElement;
}

/** Collect them from a document that already contains the markup. */
export function sandboxIndicatorView(doc: Document): SandboxIndicatorView {
  const byId = (id: string): HTMLElement => {
    const node = doc.getElementById(id);
    if (!node) {
      throw new Error(`index.html is missing #${id}`);
    }
    return node;
  };
  return { root: byId("sandbox-indicator"), text: byId("sandbox-indicator-text") };
}

/**
 * Draw what the server reported.
 *
 * `null` or an `off` mode draws nothing. A real mode draws "Sandbox: <mode>",
 * and when the mode cannot run the tooltip says so and the pill carries a
 * `data-warn` attribute the stylesheet can pick up.
 */
export function renderSandboxIndicator(
  view: SandboxIndicatorView,
  capability: SandboxCapability | null,
): void {
  if (!capability || capability.mode === "off") {
    view.root.hidden = true;
    view.text.textContent = "";
    view.root.removeAttribute("title");
    view.root.removeAttribute("aria-label");
    view.root.removeAttribute("data-warn");
    return;
  }
  view.root.hidden = false;
  view.text.textContent = `Sandbox: ${capability.mode}`;
  const tip = capability.available
    ? capability.detail
    : `cannot run: ${capability.detail}`;
  view.root.title = tip;
  view.root.setAttribute("aria-label", `Command sandbox ${capability.mode}: ${tip}`);
  if (capability.available) {
    view.root.removeAttribute("data-warn");
  } else {
    view.root.setAttribute("data-warn", "true");
  }
}
