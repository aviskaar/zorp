/**
 * Tests for the first-run flow's rules.
 *
 * Four things matter here and none of them is cosmetic.
 *
 * First run must be read off the server's own provenance fields. Getting
 * it wrong in one direction shows a setup flow to somebody who configured
 * this server through the environment; in the other it drops a new user
 * into a composer that cannot send anything.
 *
 * Free must mean the provider said zero. A missing price is not a price of
 * zero and a negative price is not one either, and a UI that tells someone
 * a model is free had better be right, because being wrong costs them
 * money.
 *
 * The automatic choices must not overclaim. `openrouter/auto` routes
 * across paid models, and the free pick is a rule over the listing that
 * came back, not a judgement about a task.
 *
 * And nothing reaches the page as markup. A model id is a string somebody
 * else chose, and it lands in the DOM through `textContent` and nothing
 * else.
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import { JSDOM } from "jsdom";
import type { ModelDetail, Settings } from "../src/api.ts";
import {
  AUTO_ROUTER_ID,
  automaticChoices,
  freeAutoPick,
  isFirstRun,
  modelGroups,
  priceClass,
  renderModelGroups,
} from "../src/onboarding.ts";
import { PRESET_DEFAULTS, presetFor } from "../src/providers.ts";

function settings(over: Partial<Settings> = {}): Settings {
  return {
    provider: "openai",
    provider_source: "default",
    base_url: "https://api.openai.com/v1",
    base_url_source: "default",
    model: "gpt-4o",
    model_source: "default",
    max_tokens: null,
    max_tokens_source: "default",
    has_api_key: false,
    api_key_source: "default",
    configured: false,
    ...over,
  };
}

function detail(over: Partial<ModelDetail> = {}): ModelDetail {
  return { id: "some/model", ...over };
}

const free = (id: string, context?: number): ModelDetail =>
  detail({ id, context_length: context, prompt_price: 0, completion_price: 0 });

const paid = (id: string): ModelDetail =>
  detail({ id, prompt_price: 0.000003, completion_price: 0.000015 });

/* ---------------------------------------------------------------- */
/* first run                                                         */
/* ---------------------------------------------------------------- */

test("nothing configured anywhere is a first run", () => {
  assert.equal(isFirstRun(settings()), true);
});

test("a key on the server is not a first run, whatever the other fields say", () => {
  assert.equal(isFirstRun(settings({ has_api_key: true, api_key_source: "env" })), false);
});

test("an operator who set ZORP_BASE_URL is not shown setup", () => {
  assert.equal(isFirstRun(settings({ base_url_source: "env" })), false);
  assert.equal(isFirstRun(settings({ model_source: "env" })), false);
  assert.equal(isFirstRun(settings({ provider_source: "env" })), false);
});

test("anything saved through the panel is not a first run", () => {
  assert.equal(isFirstRun(settings({ model_source: "ui" })), false);
});

/* ---------------------------------------------------------------- */
/* free and paid                                                     */
/* ---------------------------------------------------------------- */

test("a stated price of zero on both halves is free", () => {
  assert.equal(priceClass(free("x")), "free");
});

test("a price above zero on either half is paid", () => {
  assert.equal(priceClass(paid("x")), "paid");
  assert.equal(priceClass(detail({ prompt_price: 0, completion_price: 0.001 })), "paid");
});

test("a missing price is not a price of zero", () => {
  assert.equal(priceClass(detail()), "unstated");
  assert.equal(priceClass(detail({ prompt_price: 0 })), "unstated");
});

test("a negative price is not free either, which is what the router states", () => {
  assert.equal(
    priceClass(detail({ id: AUTO_ROUTER_ID, prompt_price: -1, completion_price: -1 })),
    "unstated",
  );
});

test("a listing that states no prices is one group and claims nothing", () => {
  const groups = modelGroups([detail({ id: "qwen3:4b" }), detail({ id: "llama3.2" })]);
  assert.equal(groups.length, 1);
  assert.equal(groups[0].title, "Models");
  assert.match(groups[0].note, /no prices/);
  assert.equal(groups[0].choices.length, 2);
});

test("a priced listing splits into free first, then paid, then unpriced", () => {
  const groups = modelGroups([
    paid("anthropic/claude-sonnet-4"),
    free("meta-llama/llama-3.3-70b-instruct:free"),
    detail({ id: AUTO_ROUTER_ID, prompt_price: -1, completion_price: -1 }),
  ]);
  assert.deepEqual(
    groups.map((group) => group.title),
    ["Free (1)", "Paid (1)", "Price not stated (1)"],
  );
  assert.equal(groups[0].choices[0].id, "meta-llama/llama-3.3-70b-instruct:free");
});

test("an id already offered above the list is not offered twice in it", () => {
  const groups = modelGroups([free("a"), free("b")], ["a"]);
  assert.deepEqual(
    groups[0].choices.map((choice) => choice.id),
    ["b"],
  );
});

/* ---------------------------------------------------------------- */
/* the automatic choices                                             */
/* ---------------------------------------------------------------- */

test("the free pick is the largest free context window, ties broken by id", () => {
  const picked = freeAutoPick([free("small", 8000), free("big", 128000), paid("huge")]);
  assert.equal(picked?.id, "big");

  const tied = freeAutoPick([free("zeta", 64000), free("alpha", 64000)]);
  assert.equal(tied?.id, "alpha", "a tie has to resolve the same way every time");
});

test("a paid-only listing has no free pick to offer", () => {
  assert.equal(freeAutoPick([paid("a"), detail({ id: "b" })]), null);
});

/**
 * Seen on the real OpenRouter listing, not imagined. Several free models
 * tie at the largest context window it publishes and two of them are music
 * models, so the id tiebreak handed a first-time user
 * `google/lyria-3-clip-preview` as the model to chat with. Note that it
 * lists `["text", "audio"]`, so "outputs text" does not exclude it and a
 * guard written that way would have passed this test while shipping the
 * bug. Text and nothing else is the line.
 */
test("a free model that answers with audio is never the automatic pick", () => {
  const audio = detail({
    id: "google/lyria-3-clip-preview",
    context_length: 1048576,
    prompt_price: 0,
    completion_price: 0,
    output_modalities: ["text", "audio"],
  });
  const chat = detail({
    id: "minimax/minimax-m3:free",
    context_length: 1048576,
    prompt_price: 0,
    completion_price: 0,
    output_modalities: ["text"],
  });
  assert.equal(freeAutoPick([audio, chat])?.id, "minimax/minimax-m3:free");
  assert.equal(freeAutoPick([audio]), null, "no text model means no pick, not a music model");
});

test("a listing that states no modalities is not read as stating 'not text'", () => {
  assert.equal(freeAutoPick([free("plain", 8000)])?.id, "plain");
});

test("the audio model is still listed, it just is not picked for anyone", () => {
  const audio = detail({
    id: "google/lyria-3-clip-preview",
    prompt_price: 0,
    completion_price: 0,
    output_modalities: ["text", "audio"],
  });
  const groups = modelGroups([audio]);
  assert.equal(groups[0].title, "Free (1)");
  assert.equal(groups[0].choices[0].id, "google/lyria-3-clip-preview");
});

test("only OpenRouter gets automatic choices", () => {
  assert.equal(automaticChoices("ollama", [free("a", 1000)]), null);
  assert.equal(automaticChoices("openai", [free("a", 1000)]), null);
});

test("the router is only offered when the listing actually named it", () => {
  const without = automaticChoices("openrouter", [free("a", 1000)]);
  assert.deepEqual(
    without?.choices.map((choice) => choice.id),
    ["a"],
  );

  const with_ = automaticChoices("openrouter", [
    detail({ id: AUTO_ROUTER_ID, prompt_price: -1, completion_price: -1 }),
    free("a", 1000),
  ]);
  assert.deepEqual(
    with_?.choices.map((choice) => choice.id),
    [AUTO_ROUTER_ID, "a"],
  );
});

test("the router says it can pick a paid model and never that it picks free ones", () => {
  const group = automaticChoices("openrouter", [
    detail({ id: AUTO_ROUTER_ID, prompt_price: -1, completion_price: -1 }),
  ]);
  const note = group?.choices[0].note ?? "";
  assert.match(note, /paid models included/);
  assert.doesNotMatch(note, /best free/i);
});

test("the free pick says what rule picked it and saves a real model id", () => {
  const group = automaticChoices("openrouter", [free("big/one", 128000), free("small/one", 8000)]);
  const choice = group?.choices[0];
  assert.equal(choice?.id, "big/one", "it saves the model, not a rule");
  assert.match(choice?.note ?? "", /largest context window/);
});

test("a listing with nothing to automate offers no automatic group", () => {
  assert.equal(automaticChoices("openrouter", [paid("only/paid")]), null);
});

/* ---------------------------------------------------------------- */
/* drawing                                                           */
/* ---------------------------------------------------------------- */

function page(): { doc: Document; into: HTMLElement } {
  const dom = new JSDOM("<!doctype html><div id='models'></div>");
  const doc = dom.window.document;
  return { doc, into: doc.getElementById("models") as HTMLElement };
}

test("a model name carrying markup lands as text and not as elements", () => {
  const { doc, into } = page();
  const hostile = detail({
    id: "<img src=x onerror=alert(1)>",
    name: "<script>alert(1)</script>",
    prompt_price: 0,
    completion_price: 0,
  });
  renderModelGroups(doc, into, modelGroups([hostile]), "");
  assert.equal(into.querySelectorAll("script, img").length, 0);
  assert.match(into.textContent ?? "", /<script>alert\(1\)<\/script>/);
  const input = into.querySelector("input") as HTMLInputElement;
  assert.equal(input.value, "<img src=x onerror=alert(1)>");
});

test("the saved model is the checked one when the listing still has it", () => {
  const { doc, into } = page();
  const drawn = renderModelGroups(doc, into, modelGroups([free("a"), free("b")]), "b");
  assert.equal(drawn, 2);
  const checked = into.querySelectorAll("input:checked");
  assert.equal(checked.length, 1);
  assert.equal((checked[0] as HTMLInputElement).value, "b");
});

test("with nothing saved the first row is checked, so Continue always has a model", () => {
  const { doc, into } = page();
  renderModelGroups(doc, into, modelGroups([free("a"), free("b")]), "gpt-4o");
  const checked = into.querySelector("input:checked") as HTMLInputElement;
  assert.equal(checked.value, "a");
});

test("an empty listing draws nothing and says so by returning zero", () => {
  const { doc, into } = page();
  assert.equal(renderModelGroups(doc, into, modelGroups([]), ""), 0);
  assert.equal(into.children.length, 0);
});

/* ---------------------------------------------------------------- */
/* the OpenRouter preset                                             */
/* ---------------------------------------------------------------- */

test("OpenRouter is a preset and it needs a key", () => {
  const openrouter = PRESET_DEFAULTS.openrouter;
  assert.equal(openrouter.baseUrl, "https://openrouter.ai/api/v1");
  assert.equal(openrouter.provider, "openai");
  assert.equal(openrouter.needsKey, true);
  assert.equal(openrouter.keyUrl, "https://openrouter.ai/workspaces/default/keys");
});

test("reopening the panel on an OpenRouter base URL shows OpenRouter, not custom", () => {
  assert.equal(presetFor("openai", "https://openrouter.ai/api/v1"), "openrouter");
  assert.equal(presetFor("openai", "https://openrouter.ai/api/v1/"), "openrouter");
  assert.equal(presetFor("openai", "https://api.openai.com/v1"), "openai");
  assert.equal(presetFor("openai", "http://localhost:11434/v1"), "ollama");
  assert.equal(presetFor("anthropic", "https://api.anthropic.com/v1"), "anthropic");
  assert.equal(presetFor("openai", "https://example.invalid/v1"), "custom");
});
