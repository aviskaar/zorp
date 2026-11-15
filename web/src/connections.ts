/**
 * Saved model connections: the settings rows that edit them and the
 * composer select that picks one for a conversation. Every string lands
 * through `textContent` or an input's `value`: a label and a model id are a
 * person's own text off the settings file, or a provider's listing.
 */

import type { ModelConnection } from "./api";

/** What a connection is called on the page: its label, else its id. */
export function connectionName(c: ModelConnection): string {
  const label = (c.label ?? "").trim();
  return label === "" ? c.id : label;
}

/** An id from a label: lowercase letters, digits and dashes, at most 40. */
export function connectionSlug(text: string, taken: string[]): string {
  const base =
    text
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "")
      .slice(0, 36) || "connection";
  let id = base;
  for (let n = 2; taken.includes(id); n += 1) {
    id = `${base}-${n}`;
  }
  return id;
}

/** The edits the rows report back to the page. */
export interface ConnectionRowCallbacks {
  change(index: number, next: ModelConnection): void;
  key(id: string, key: string): void;
  makeDefault(id: string | null): void;
  remove(index: number): void;
}

/**
 * Draw one editable row per connection: name, base URL, model, a key field
 * that is never filled from the server, a default radio and Remove.
 */
export function renderConnectionRows(
  doc: Document,
  host: HTMLElement,
  list: ModelConnection[],
  defaultId: string | null,
  cb: ConnectionRowCallbacks,
): void {
  host.replaceChildren();
  list.forEach((c, index) => {
    const row = doc.createElement("div");
    row.className = "connection-row";

    const field = (
      name: string,
      value: string,
      placeholder: string,
      set: (v: string) => ModelConnection,
    ): HTMLInputElement => {
      const input = doc.createElement("input");
      input.type = "text";
      input.className = `connection-${name}`;
      input.placeholder = placeholder;
      input.value = value;
      input.setAttribute("aria-label", `${placeholder} for ${connectionName(c)}`);
      input.addEventListener("change", () => cb.change(index, set(input.value.trim())));
      return input;
    };

    const label = field("label", c.label ?? "", "Name", (v) => ({ ...c, label: v || null }));
    const url = field("url", c.base_url, "Base URL", (v) => ({ ...c, base_url: v }));
    const model = field("model", c.model ?? "", "Model", (v) => ({ ...c, model: v || null }));

    const key = doc.createElement("input");
    key.type = "password";
    key.className = "connection-key";
    key.autocomplete = "off";
    key.placeholder = c.has_api_key ? "your key is saved, type to replace" : "your API key (optional)";
    key.setAttribute("aria-label", `API key for ${connectionName(c)}`);
    key.addEventListener("change", () => cb.key(c.id, key.value));

    const env = field("env", c.api_key_env ?? "", "or key from env var", (v) => ({
      ...c,
      api_key_env: v ? v.toUpperCase() : null,
    }));

    const def = doc.createElement("label");
    def.className = "connection-default";
    const radio = doc.createElement("input");
    radio.type = "radio";
    radio.name = "connection-default";
    radio.checked = c.id === defaultId;
    radio.addEventListener("change", () => cb.makeDefault(c.id));
    const defText = doc.createElement("span");
    defText.textContent = "default";
    def.append(radio, defText);

    const remove = doc.createElement("button");
    remove.type = "button";
    remove.className = "connection-remove";
    remove.textContent = "Remove";
    remove.addEventListener("click", () => cb.remove(index));

    row.append(label, url, model, key, env, def, remove);
    host.append(row);
  });
}

/**
 * Fill the composer's select: the main model first (value ""), then each
 * connection. `chosen` is the conversation's connection id or null.
 */
export function fillConnectionSelect(
  doc: Document,
  select: HTMLSelectElement,
  list: ModelConnection[],
  mainModel: string,
  chosen: string | null,
): void {
  select.replaceChildren();
  const main = doc.createElement("option");
  main.value = "";
  main.textContent = mainModel ? `Main model (${mainModel})` : "Main model";
  select.append(main);
  for (const c of list) {
    const option = doc.createElement("option");
    option.value = c.id;
    option.textContent = c.model ? `${connectionName(c)} (${c.model})` : connectionName(c);
    select.append(option);
  }
  select.value = chosen && list.some((c) => c.id === chosen) ? chosen : "";
}
