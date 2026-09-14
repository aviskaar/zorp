/**
 * The sections of the settings pane that are not the model form.
 *
 * Model and workspace moved into the pane as they were: same markup, same
 * element ids, same `wireSettings` driving them. This module is the four
 * that are new, and they are all reports with the exception of three
 * buttons that delete things.
 *
 * **Everything here goes through `textContent`.** An MCP server's name and
 * command come out of a TOML file that may have arrived by `git clone`; a
 * path came off a filesystem; a doctor check's detail is assembled from
 * both. None of it is ours and this module builds no HTML strings.
 *
 * `doc` is passed in rather than taken from the global, the same as
 * `skills-view` and for the same reason: it is what lets a test render into
 * a jsdom document and read back what actually landed.
 */

import type { DataListing, DoctorReport, McpListing, SkillListing } from "./api.ts";
import { renderSkillsPanel } from "./skills-view.ts";

/** The word somebody has to type before anything is deleted. */
export const CONFIRM_WORD = "delete";

function el(doc: Document, tag: string, className: string): HTMLElement {
  const node = doc.createElement(tag);
  node.className = className;
  return node;
}

function textNode(doc: Document, tag: string, className: string, value: string): HTMLElement {
  const node = el(doc, tag, className);
  node.textContent = value;
  return node;
}

/** A size a person reads rather than counts digits in. */
export function humanBytes(bytes: number | null): string {
  if (bytes === null || bytes === undefined) {
    // Absent and empty are different answers. A file that was never written
    // shown as "0 B" sends somebody looking for it.
    return "not created";
  }
  const units = ["B", "KB", "MB", "GB"];
  let size = bytes;
  let unit = 0;
  while (size >= 1024 && unit < units.length - 1) {
    size /= 1024;
    unit += 1;
  }
  return unit === 0 ? `${bytes} B` : `${size.toFixed(1)} ${units[unit]}`;
}

/* ------------------------------------------------------------------ */
/* skills                                                              */
/* ------------------------------------------------------------------ */

/**
 * The skills listing, drawn the way the popover draws it.
 *
 * The same function, not a second copy of it, so the pane and the pill
 * cannot drift about what a skill is or what it grants. Read only: there is
 * still no route that loads a skill and there must never be one.
 */
export function renderSkillsSection(
  doc: Document,
  host: HTMLElement,
  installed: SkillListing,
): void {
  host.replaceChildren();
  renderSkillsPanel(doc, host, installed);
}

/* ------------------------------------------------------------------ */
/* MCP                                                                 */
/* ------------------------------------------------------------------ */

/**
 * Configured MCP servers, and the sentence that stops this reading as a
 * list of working ones.
 *
 * Two things this must keep saying. `zorp-web` loads no MCP servers, so
 * every row is configured and not live, and a listing that did not say so
 * would read as "these are working", which is the opposite of true. And the
 * values in `env` and `headers` are not here, because those maps are where
 * a token goes and a settings page is exactly where somebody screenshots.
 * The server sends key names only; this draws what it is sent.
 */
export function renderMcpSection(doc: Document, host: HTMLElement, listing: McpListing): void {
  host.replaceChildren();

  if (!listing.loads_servers) {
    host.append(textNode(doc, "p", "settings-note", listing.why));
  }

  if (listing.warning) {
    // A server that silently vanishes is a tool that silently stops
    // existing, so a file that did not parse is named.
    host.append(textNode(doc, "p", "skills-warning", listing.warning));
  }

  if (!listing.servers.length) {
    host.append(
      textNode(
        doc,
        "p",
        "skills-empty",
        "No MCP servers configured. zorp reads them from " + listing.sources.join(" and "),
      ),
    );
    return;
  }

  const list = el(doc, "ul", "skills-list");
  for (const server of listing.servers) {
    const item = el(doc, "li", "skills-item");
    item.append(
      textNode(doc, "span", "skills-name", server.name),
      textNode(
        doc,
        "span",
        "skills-description",
        server.url ?? [server.command ?? "", ...server.args].join(" ").trim(),
      ),
      textNode(
        doc,
        "span",
        "skills-presence",
        `${server.transport}, trust ${server.trust}` +
          (server.loaded ? ", loaded" : ", configured but not loaded in this build"),
      ),
    );
    // Names, never values. Seeing that a server wants GITHUB_TOKEN tells a
    // person what to set without telling anybody what it is.
    const secrets = [...server.env_keys, ...server.header_keys];
    if (secrets.length) {
      item.append(
        textNode(
          doc,
          "span",
          "skills-declared",
          `reads ${secrets.join(", ")}. zorp never shows their values.`,
        ),
      );
    }
    list.append(item);
  }
  host.append(list);
}

/* ------------------------------------------------------------------ */
/* data                                                                */
/* ------------------------------------------------------------------ */

/** One destructive action, as the pane offers it. */
export interface DataAction {
  key: "conversations" | "index" | "settings" | "everything";
  label: string;
  /** What goes, said plainly enough to change somebody's mind. */
  what: string;
}

/**
 * The three things that can be cleared, and the fourth that is the three in
 * order.
 *
 * "Reset everything" is deliberately not a fourth endpoint. It is the
 * browser calling the other three, so there is one implementation of each
 * thing that can be deleted and no fourth path that forgets one.
 */
export const DATA_ACTIONS: DataAction[] = [
  {
    key: "conversations",
    label: "Clear conversations",
    what: "Every conversation and every project label, from both the browser and the terminal. This cannot be undone.",
  },
  {
    key: "index",
    label: "Delete the search index",
    what: "The embeddings recall searches. The conversations stay, and the next sweep rebuilds the index from them.",
  },
  {
    key: "settings",
    label: "Reset settings",
    what: "The saved model and endpoint, and the list of trusted workspace agents. Your conversations stay.",
  },
  {
    key: "everything",
    label: "Reset everything",
    what: "All three of the above, in order. Nothing in your workspace is touched: scratch/ and every file beside it are yours.",
  },
];

/**
 * What zorp keeps on this machine, and the buttons that clear part of it.
 *
 * A path is not a secret, and a person who cannot see where their
 * conversations are kept cannot back them up, move them, or delete them.
 * The variable that moves each file is named beside it, because
 * `ZORP_STATE_DB` and `XDG_STATE_HOME` mean somebody can easily be looking
 * at a different database than they think.
 *
 * `onAct` is called with the action only after a person has typed the
 * confirmation word. Nothing here is recoverable and a click is too cheap.
 */
export function renderDataSection(
  doc: Document,
  host: HTMLElement,
  listing: DataListing,
  onAct: (action: DataAction) => void,
): void {
  host.replaceChildren();

  const list = el(doc, "ul", "skills-list");
  for (const file of listing.files) {
    const item = el(doc, "li", "skills-item");
    item.append(
      textNode(doc, "span", "skills-name", file.label),
      textNode(doc, "span", "skills-description", file.what),
      textNode(
        doc,
        "span",
        "skills-presence",
        `${humanBytes(file.bytes)} at ${file.path}` +
          (file.env_var ? `, moved by ${file.env_var}` : ""),
      ),
    );
    list.append(item);
  }
  host.append(list);

  // Said before the buttons rather than after, because somebody resetting
  // settings is usually trying to get rid of a credential and this is the
  // one that survives.
  host.append(textNode(doc, "p", "settings-note", listing.reset_note));

  const actions = el(doc, "div", "settings-danger");
  for (const action of DATA_ACTIONS) {
    actions.append(confirmRow(doc, action, onAct));
  }
  host.append(actions);
}

/**
 * One action behind a typed confirmation.
 *
 * The word, not a click. Every one of these is unrecoverable, and a button
 * that fires on the first click is one somebody hits while reading the
 * sentence that would have stopped them. The button stays disabled until
 * the word matches exactly.
 */
function confirmRow(
  doc: Document,
  action: DataAction,
  onAct: (action: DataAction) => void,
): HTMLElement {
  const row = el(doc, "div", "settings-danger-row");
  row.dataset.action = action.key;

  row.append(
    textNode(doc, "div", "skills-name", action.label),
    textNode(doc, "div", "skills-description", action.what),
  );

  const controls = el(doc, "div", "settings-field-row");
  const input = doc.createElement("input");
  input.type = "text";
  input.className = "settings-confirm";
  input.autocomplete = "off";
  input.spellcheck = false;
  input.placeholder = `type ${CONFIRM_WORD} to enable`;
  input.setAttribute("aria-label", `Type ${CONFIRM_WORD} to enable: ${action.label}`);

  const button = doc.createElement("button");
  button.type = "button";
  button.className = "btn btn-deny";
  button.textContent = action.label;
  button.disabled = true;

  input.addEventListener("input", () => {
    button.disabled = input.value.trim().toLowerCase() !== CONFIRM_WORD;
  });
  button.addEventListener("click", () => {
    if (button.disabled) {
      return;
    }
    onAct(action);
    // Back behind the gate, so a second click is a second decision.
    input.value = "";
    button.disabled = true;
  });

  controls.append(input, button);
  row.append(controls);
  return row;
}

/* ------------------------------------------------------------------ */
/* this build                                                          */
/* ------------------------------------------------------------------ */

/**
 * What this build can do and whether it can reach anything.
 *
 * `zorp-agent doctor` for the browser. No secret is in it: the key is
 * reported set or not set, which is `doctor::api_key_check`'s whole
 * contract, and the server has a test grepping the serialized body.
 *
 * The endpoint probe is not run unless asked. `probe_completion` waits up
 * to thirty seconds, and a settings pane that hangs on a slow endpoint is
 * worse than one that says it has not checked.
 */
export function renderDoctorSection(
  doc: Document,
  host: HTMLElement,
  report: DoctorReport,
  onProbe: () => void,
): void {
  host.replaceChildren();

  host.append(textNode(doc, "p", "settings-note", `zorp ${report.version}`));

  const list = el(doc, "ul", "skills-list");
  for (const check of report.checks) {
    const item = el(doc, "li", `skills-item doctor-${check.health}`);
    item.append(
      textNode(doc, "span", "skills-name", check.label),
      textNode(doc, "span", "skills-description", check.detail),
    );
    list.append(item);
  }
  host.append(list);

  const button = doc.createElement("button");
  button.type = "button";
  button.className = "btn btn-deny";
  button.textContent = "Check the endpoint";
  button.addEventListener("click", () => onProbe());
  host.append(button);
}
