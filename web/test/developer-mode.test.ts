import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";

import { DeveloperModeView } from "../src/developer-mode.ts";

const jsdom = new JSDOM("<!doctype html><body><div id='container'></div></body>");
const doc = jsdom.window.document;

test("DeveloperModeView renders header, navigation tabs, and default pretrain tab", () => {
  const container = doc.createElement("div");
  const view = new DeveloperModeView(container, () => {}, () => {});
  view.render();

  assert.ok(container.querySelector(".dev-mode-shell"));
  assert.ok(container.querySelector(".dev-mode-header"));
  const navBtns = container.querySelectorAll(".nav-btn");
  assert.equal(navBtns.length, 5);

  const tabLabels = Array.from(navBtns).map((b) => b.textContent?.trim());
  assert.deepEqual(tabLabels, [
    "Datasets",
    "Tokenizer",
    "Architecture",
    "Pretrain",
    "Model Registry",
  ]);

  // Default tab is pretrain
  assert.ok(container.querySelector(".pretrain-dashboard"));
  assert.ok(container.querySelector("#loss-svg"));
  assert.ok(container.querySelector("#btn-start"));
});

test("DeveloperModeView tab switching updates active view", async () => {
  const container = doc.createElement("div");
  const view = new DeveloperModeView(container, () => {}, () => {});
  view.render();

  // Switch to Tokenizer
  const tokBtn = Array.from(container.querySelectorAll(".nav-btn")).find(
    (b) => b.getAttribute("data-tab") === "tokenizer"
  ) as HTMLButtonElement;
  assert.ok(tokBtn);
  tokBtn.click();

  // Wait a tick for tab content rendering
  await new Promise((r) => setTimeout(r, 10));
  assert.ok(container.querySelector(".tokenizer-dashboard"));
  assert.ok(container.querySelector("#btn-train-tok"));
  assert.ok(container.querySelector("#token-chips-container"));

  // Switch to Architecture
  const archBtn = Array.from(container.querySelectorAll(".nav-btn")).find(
    (b) => b.getAttribute("data-tab") === "architecture"
  ) as HTMLButtonElement;
  assert.ok(archBtn);
  archBtn.click();

  await new Promise((r) => setTimeout(r, 10));
  assert.ok(container.querySelector(".architecture-dashboard"));
  assert.ok(container.querySelector("#calc-total-params"));

  // Switch to Model Registry
  const regBtn = Array.from(container.querySelectorAll(".nav-btn")).find(
    (b) => b.getAttribute("data-tab") === "registry"
  ) as HTMLButtonElement;
  assert.ok(regBtn);
  regBtn.click();

  await new Promise((r) => setTimeout(r, 10));
  assert.ok(container.querySelector(".registry-dashboard"));
});

test("DeveloperModeView triggers onBackToAgent when back button clicked", () => {
  let backCalled = false;
  const container = doc.createElement("div");
  const view = new DeveloperModeView(
    container,
    () => {},
    () => {
      backCalled = true;
    }
  );
  view.render();

  const backBtn = container.querySelector("#btn-back-agent") as HTMLButtonElement;
  assert.ok(backBtn);
  backBtn.click();
  assert.equal(backCalled, true);
});
