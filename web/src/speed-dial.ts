/**
 * The model speed dial: nine numbered slots, one chord each, switch without
 * leaving the composer. See issue #242.
 *
 * A slot holds a model a person parked. Pressing its chord is an ordinary
 * settings save of that slot's non-secret model fields and nothing more, so
 * the next turn reads the new model the way it already reads a model saved
 * through the form. A slot never holds an API key: the type has no field for
 * one, and the one key in `ZORP_API_KEY` is used as it is for the form.
 *
 * The pure functions here (`slotFromChord`, `slotByNumber`, `slotFields`)
 * are the testable heart; the table and pill helpers build DOM through
 * `textContent`, because a slot's label is a person's own text off a
 * settings file and lands the same way a model name off a provider listing
 * does.
 */

import type { DialSlot, SettingsUpdate } from "./api.ts";

/**
 * The slot number a keyboard event asks for, or `null` when it is not a
 * dial chord.
 *
 * The chord is Control+Alt+digit (Control+Option on a Mac) for 1 to 9. It
 * is read from `event.code`, not `event.key`, because Option+digit on a Mac
 * produces a different character for `key` while the physical digit key is
 * stable in `code`; the numeric keypad counts too. Any extra modifier
 * (Shift or Meta), an open IME composition, or `inSettings` (the caller's
 * word that focus is in the settings pane, where a digit is being typed into
 * a slot's own fields) means it is not a switch.
 */
export function slotFromChord(event: KeyboardEvent, inSettings: boolean): number | null {
  if (inSettings || event.isComposing) {
    return null;
  }
  if (!event.ctrlKey || !event.altKey || event.shiftKey || event.metaKey) {
    return null;
  }
  const match = /^(?:Digit|Numpad)([1-9])$/.exec(event.code);
  return match ? Number(match[1]) : null;
}

/** The slot parked under `n`, or `undefined`. */
export function slotByNumber(dial: DialSlot[], n: number): DialSlot | undefined {
  return dial.find((s) => s.slot === n);
}

/**
 * The settings save a slot press sends: exactly the slot's non-secret model
 * fields, never the label, the slot number, or anything that could carry a
 * key. Fields the slot left unset are omitted, so a press leaves them as the
 * saved settings already have them.
 */
export function slotFields(slot: DialSlot): SettingsUpdate {
  const update: SettingsUpdate = {};
  if (slot.provider !== undefined) update.provider = slot.provider;
  if (slot.base_url !== undefined) update.base_url = slot.base_url;
  if (slot.model !== undefined) update.model = slot.model;
  if (slot.max_tokens !== undefined) update.max_tokens = slot.max_tokens;
  return update;
}

/** Whether a slot actually points at a model (has one parked). */
export function slotIsSet(slot: DialSlot | undefined): slot is DialSlot {
  return slot !== undefined && typeof slot.model === "string" && slot.model.trim() !== "";
}

/** What the model pill shows for a slot name: the label, else the model id. */
export function slotName(slot: DialSlot): string {
  const label = slot.label?.trim();
  return label && label.length > 0 ? label : (slot.model ?? "").trim();
}

/** Callbacks the dial table needs from the page. */
export interface DialTableCallbacks {
  /** The model the form currently shows, to copy into slot `n`. */
  useCurrent(n: number): void;
  /** Clear slot `n`. */
  clear(n: number): void;
  /** A slot's label changed; persist the whole dial. */
  relabel(n: number, label: string): void;
}

/**
 * Draw the nine speed-dial rows into `host`. Each row is a slot number, a
 * label field, what the slot points at (its model id, or "empty"), and
 * "Use current model" / "Clear" buttons. Every string lands through
 * `textContent`: a label is a person's own text off the settings file, the
 * same kind of input a model name off a provider listing is.
 *
 * The chord is Control+Alt+<number> (Control+Option on a Mac); the section
 * says so once, drawn here so the page has one place that states it.
 */
export function renderDialRows(
  doc: Document,
  host: HTMLElement,
  dial: DialSlot[],
  cb: DialTableCallbacks,
): void {
  host.replaceChildren();
  for (let n = 1; n <= 9; n += 1) {
    const slot = slotByNumber(dial, n);
    const row = doc.createElement("div");
    row.className = "dial-row";

    const num = doc.createElement("span");
    num.className = "dial-num";
    num.textContent = String(n);

    const label = doc.createElement("input");
    label.type = "text";
    label.className = "dial-label";
    label.placeholder = "label";
    label.setAttribute("aria-label", `Label for speed-dial slot ${n}`);
    label.value = slot?.label ?? "";
    label.addEventListener("change", () => cb.relabel(n, label.value));

    const target = doc.createElement("span");
    target.className = "dial-target";
    target.textContent = slotIsSet(slot) ? (slot.model ?? "") : "empty";
    if (!slotIsSet(slot)) {
      target.classList.add("dial-empty");
    }

    const use = doc.createElement("button");
    use.type = "button";
    use.className = "dial-use";
    use.textContent = "Use current model";
    use.addEventListener("click", () => cb.useCurrent(n));

    const clear = doc.createElement("button");
    clear.type = "button";
    clear.className = "dial-clear";
    clear.textContent = "Clear";
    clear.disabled = !slot;
    clear.addEventListener("click", () => cb.clear(n));

    row.append(num, label, target, use, clear);
    host.append(row);
  }
}
