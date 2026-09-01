/**
 * The first-run flow's rules, and the one place it puts model names on the
 * page.
 *
 * Somebody opening zorp-web for the first time lands on a composer that
 * cannot send anything, because no model is configured. The panel behind
 * the model badge has always been able to fix that. This is the guided way
 * to the same place: a start page, a provider, a key if that provider needs
 * one, a model, and a check that it works. Every step of it writes through
 * the same `PUT /api/settings` the panel uses, and nothing here keeps its
 * own copy of the API key.
 *
 * **Everything reaching the page goes through `textContent`.** Model ids,
 * names and prices are text from a remote listing that zorp did not write,
 * and `meta-llama/llama-3.3-70b-instruct` is a string somebody else chose.
 * There is no `innerHTML` in this file and there must never be one.
 *
 * **Nothing here asks a model anything.** The free choice below is picked
 * by a rule in code from the listing that came back, and the rule is
 * printed next to the choice. A classifier deciding which model suits a
 * question would be a model call the person did not ask for, and it would
 * be spending their money to decide how to spend their money.
 */

import type { ModelDetail, Settings } from "./api.ts";

/**
 * Nobody has ever configured anything.
 *
 * Read off what the server already reports rather than remembered here. A
 * field's source is `default` only when no save and no `ZORP_*` env var
 * gave it a value, so all three at `default` with no key anywhere means the
 * settings are the hardcoded fallback and not a choice. An operator who
 * exported `ZORP_BASE_URL` has configured this server, gets `env`, and is
 * not shown a setup flow for work they already did.
 *
 * This is deliberately the same shape the server calls `configured`. Both
 * exist: the server refuses a turn on it, and this decides whether to open
 * a flow, and reading the fields keeps the second from silently changing
 * meaning if the first ever grows a term.
 */
export function isFirstRun(settings: Settings): boolean {
  return (
    !settings.has_api_key &&
    settings.provider_source === "default" &&
    settings.base_url_source === "default" &&
    settings.model_source === "default"
  );
}

/**
 * What a listing said a model costs.
 *
 * Three values and not two. "Free" is the provider stating zero. "Unstated"
 * is the provider saying nothing, which is every local server and most
 * OpenAI-compatible ones, and it is not the same claim. A negative price is
 * also unstated: OpenRouter uses it for a model whose cost is decided per
 * request, which is not a price and not free.
 */
export type PriceClass = "free" | "paid" | "unstated";

export function priceClass(detail: ModelDetail): PriceClass {
  const prompt = detail.prompt_price;
  const completion = detail.completion_price;
  if (prompt === undefined || completion === undefined) {
    return "unstated";
  }
  if (prompt < 0 || completion < 0) {
    return "unstated";
  }
  if (prompt === 0 && completion === 0) {
    return "free";
  }
  return "paid";
}

/** One model a person can pick, with whatever is worth saying about it. */
export interface ModelChoice {
  /** What gets saved as the model. */
  id: string;
  /** What the reader sees first. The provider's name for it, or its id. */
  label: string;
  /** One line under the label. Empty when there is nothing to add. */
  note: string;
}

/** A titled run of choices, with a line saying what the title means. */
export interface ChoiceGroup {
  title: string;
  note: string;
  choices: ModelChoice[];
}

/** OpenRouter's own router. A real model id, sent like any other. */
export const AUTO_ROUTER_ID = "openrouter/auto";

/**
 * What the router actually does, said plainly.
 *
 * It routes across everything OpenRouter serves, paid models included, and
 * it is not "the best free model for the task". Labelling it that way would
 * be the one sentence on this page that could cost somebody money they did
 * not agree to spend.
 */
export const AUTO_ROUTER_NOTE =
  "OpenRouter picks the model for each message, paid models included. It is their router, not a zorp rule, and it can pick a model you pay for.";

function contextOf(detail: ModelDetail): number {
  return detail.context_length ?? 0;
}

/**
 * Whether the listing says text is the only thing this answers with.
 *
 * A model that stated no modalities passes: nothing was said, and treating
 * silence as "not text" would throw away every provider that does not
 * publish this at all.
 *
 * "Includes text" was tried first and was not enough. OpenRouter lists
 * `google/lyria-3-clip-preview` as answering with `["text", "audio"]`, it
 * ties for the largest free context window on the real listing, and the id
 * tiebreak then handed a first-time user a music generator as the model to
 * chat with. Text and nothing else is the checkable line, and it is what
 * the note beside the choice says.
 */
function answersWithTextOnly(detail: ModelDetail): boolean {
  const stated = detail.output_modalities;
  return stated === undefined || (stated.length === 1 && stated[0] === "text");
}

/**
 * The free model to offer as an automatic choice, or null when the listing
 * had no free one that answers with text and nothing else.
 *
 * The rule is the largest stated context window among the free models the
 * listing shows answering with text and nothing else, ties broken by id so
 * that the same listing always gives the same answer. It is arithmetic over
 * the listing and nothing else: no model is asked, and no attempt is made to
 * guess which model suits a question.
 * The UI prints this rule beside the choice, because a pick nobody can
 * check is a recommendation and this is not one.
 *
 * The modality guard applies here and not to the lists below it. This is
 * the one choice zorp makes on somebody's behalf, so it owes them a model
 * that can answer a message. The lists are what the provider serves, and
 * choosing from those is theirs to do.
 */
export function freeAutoPick(details: ModelDetail[]): ModelDetail | null {
  let best: ModelDetail | null = null;
  for (const detail of details) {
    if (priceClass(detail) !== "free" || !answersWithTextOnly(detail)) {
      continue;
    }
    if (
      best === null ||
      contextOf(detail) > contextOf(best) ||
      (contextOf(detail) === contextOf(best) && detail.id < best.id)
    ) {
      best = detail;
    }
  }
  return best;
}

function tokens(count: number): string {
  if (count >= 1000) {
    return `${Math.round(count / 1000)}K context`;
  }
  return `${count} context`;
}

function noteFor(detail: ModelDetail): string {
  const parts: string[] = [];
  if (detail.context_length) {
    parts.push(tokens(detail.context_length));
  }
  if (detail.name && detail.name !== detail.id) {
    parts.push(detail.id);
  }
  return parts.join(" · ");
}

function choiceFor(detail: ModelDetail): ModelChoice {
  return { id: detail.id, label: detail.name || detail.id, note: noteFor(detail) };
}

/**
 * The automatic choices to offer above the list, for this preset.
 *
 * Only OpenRouter has any, and only for what its listing actually
 * contained. The router is offered when the listing names it, so a renamed
 * or withdrawn id is never presented as selectable. The free pick is
 * offered when there is at least one free model to pick from.
 */
export function automaticChoices(preset: string, details: ModelDetail[]): ChoiceGroup | null {
  if (preset !== "openrouter") {
    return null;
  }
  const choices: ModelChoice[] = [];
  if (details.some((detail) => detail.id === AUTO_ROUTER_ID)) {
    choices.push({ id: AUTO_ROUTER_ID, label: "AutoRouter", note: AUTO_ROUTER_NOTE });
  }
  const free = freeAutoPick(details);
  if (free) {
    choices.push({
      id: free.id,
      label: `Largest free model: ${free.name || free.id}`,
      note: `Picked here in code from this listing: of the models OpenRouter prices at zero and lists as answering with text and nothing else, the one with the largest context window, ties broken by id. That is ${free.id} today, and it is saved as that id rather than as a rule.`,
    });
  }
  return choices.length > 0
    ? {
        title: "Pick one for me",
        note: "What each one does is written under it, and they are not the same thing. Nothing here reads your question.",
        choices,
      }
    : null;
}

/**
 * Sort a listing into free, paid and unpriced.
 *
 * A listing where nothing carries a price is one group and no headings: an
 * endpoint that says nothing about cost has not told anyone anything is
 * free, and splitting it into "free" and "paid" would be inventing the
 * answer. That is the normal case for Ollama, oMLX and most
 * OpenAI-compatible servers.
 *
 * `skip` drops ids already offered above the list, so the router does not
 * appear twice.
 */
export function modelGroups(details: ModelDetail[], skip: string[] = []): ChoiceGroup[] {
  const left = details.filter((detail) => !skip.includes(detail.id));
  const free = left.filter((detail) => priceClass(detail) === "free");
  const paid = left.filter((detail) => priceClass(detail) === "paid");
  const unstated = left.filter((detail) => priceClass(detail) === "unstated");

  if (free.length === 0 && paid.length === 0) {
    return unstated.length === 0
      ? []
      : [
          {
            title: "Models",
            note: "This endpoint states no prices, so nothing here is sorted into free and paid.",
            choices: unstated.map(choiceFor),
          },
        ];
  }

  const groups: ChoiceGroup[] = [];
  if (free.length > 0) {
    groups.push({
      title: `Free (${free.length})`,
      note: "The provider states a price of zero for both prompt and completion tokens. Rate limits usually still apply.",
      choices: free.map(choiceFor),
    });
  }
  if (paid.length > 0) {
    groups.push({
      title: `Paid (${paid.length})`,
      note: "The provider states a price above zero. Every message is billed to your account with them.",
      choices: paid.map(choiceFor),
    });
  }
  if (unstated.length > 0) {
    groups.push({
      title: `Price not stated (${unstated.length})`,
      note: "The listing gave no price for these, so zorp will not say what they cost.",
      choices: unstated.map(choiceFor),
    });
  }
  return groups;
}

/**
 * Draw the groups as one radio group.
 *
 * Every string here is remote text and every one of them lands through
 * `textContent`. Returns the number of choices drawn, so a caller can tell
 * an empty listing from a drawn one without reading the DOM back.
 */
export function renderModelGroups(
  doc: Document,
  into: HTMLElement,
  groups: ChoiceGroup[],
  selected: string,
): number {
  into.replaceChildren();
  // Decided once, before anything is drawn. A model that is already saved
  // stays selected; otherwise the first row is, so Continue always has
  // something to save and never silently saves nothing.
  const ids = groups.flatMap((group) => group.choices.map((choice) => choice.id));
  const checked = ids.includes(selected) ? selected : ids[0];
  let count = 0;
  for (const group of groups) {
    const section = doc.createElement("div");
    section.className = "onboard-group";

    const title = doc.createElement("h4");
    title.className = "onboard-group-title";
    title.textContent = group.title;
    section.append(title);

    const note = doc.createElement("p");
    note.className = "onboard-group-note";
    note.textContent = group.note;
    section.append(note);

    for (const choice of group.choices) {
      const row = doc.createElement("label");
      row.className = "onboard-choice";

      const input = doc.createElement("input");
      input.type = "radio";
      input.name = "onboard-model";
      input.value = choice.id;
      input.checked = choice.id === checked;

      const body = doc.createElement("span");
      body.className = "onboard-choice-body";

      const label = doc.createElement("span");
      label.className = "onboard-choice-label";
      label.textContent = choice.label;
      body.append(label);

      if (choice.note) {
        const sub = doc.createElement("span");
        sub.className = "onboard-choice-note";
        sub.textContent = choice.note;
        body.append(sub);
      }

      row.append(input, body);
      section.append(row);
      count += 1;
    }

    into.append(section);
  }
  return count;
}
