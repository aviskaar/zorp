/**
 * Tests for saved model connections: the settings rows and the composer
 * select. A label is a person's text off the settings file and lands as
 * text, never markup, and the key field is never filled from the server.
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import { JSDOM } from "jsdom";
import {
  connectionSlug,
  fillConnectionSelect,
  renderConnectionRows,
} from "../src/connections.ts";

const list = [
  { id: "deepseek", label: "<b>DeepSeek</b>", base_url: "https://api.deepseek.com/v1", model: "deepseek-chat", has_api_key: true },
  { id: "local", base_url: "http://localhost:11434/v1", model: null },
];

test("a slug is lowercase, dashed and unique", () => {
  assert.equal(connectionSlug("Ollama (remote)", []), "ollama-remote");
  assert.equal(connectionSlug("DeepSeek", ["deepseek"]), "deepseek-2");
  assert.equal(connectionSlug("!!!", []), "connection");
});

test("the select puts the main model first and labels land as text", () => {
  const { document } = new JSDOM("<select></select>").window;
  const select = document.querySelector("select")!;
  fillConnectionSelect(document, select, list, "x/y", "deepseek");
  assert.equal(select.options.length, 3);
  assert.equal(select.options[0].value, "");
  assert.equal(select.value, "deepseek");
  assert.equal(select.options[1].textContent, "<b>DeepSeek</b> (deepseek-chat)");
  assert.equal(select.querySelector("b"), null);
  fillConnectionSelect(document, select, list, "x/y", "gone");
  assert.equal(select.value, "", "an unknown id falls back to the main model");
});

test("rows never prefill a key and report edits by index", () => {
  const { document } = new JSDOM("<div></div>").window;
  const host = document.querySelector("div")!;
  const changes: unknown[] = [];
  let made: string | null = null;
  renderConnectionRows(document, host, list, "local", {
    change: (i, next) => changes.push([i, next.model]),
    key: () => {},
    makeDefault: (id) => (made = id),
    remove: () => {},
  });
  const keys = [...host.querySelectorAll<HTMLInputElement>(".connection-key")];
  assert.deepEqual(keys.map((k) => k.value), ["", ""]);
  assert.match(keys[0].placeholder, /saved/);
  const model = host.querySelectorAll<HTMLInputElement>(".connection-model")[1];
  model.value = "qwen3:8b";
  model.dispatchEvent(new document.defaultView!.Event("change"));
  assert.deepEqual(changes, [[1, "qwen3:8b"]]);
  const radio = host.querySelectorAll<HTMLInputElement>("input[type=radio]")[0];
  radio.checked = true;
  radio.dispatchEvent(new document.defaultView!.Event("change"));
  assert.equal(made, "deepseek");
});
