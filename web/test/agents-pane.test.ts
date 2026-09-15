/**
 * Tests for the agents pane: agent cards, default agent, trust button, and selection.
 */

import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";

import { AgentsPaneView } from "../src/agents-pane.ts";
import type { AgentListing } from "../src/api.ts";

const MARKUP = `
<!doctype html><body>
  <div id="agents-host"></div>
</body>`;

function fixture(): { doc: Document; host: HTMLElement } {
  const dom = new JSDOM(MARKUP);
  const doc = dom.window.document as unknown as Document;
  const host = doc.getElementById("agents-host")!;
  return { doc, host };
}

test("AgentsPaneView renders default agent and active state", () => {
  const { doc, host } = fixture();
  let current: string | null = null;
  const callbacks = {
    onClose: () => {},
    onSelectAgent: async (name: string | null) => {
      current = name;
    },
    getCurrentAgent: () => current,
  };
  const view = new AgentsPaneView(doc, host, callbacks);

  const listing: AgentListing = {
    default: "zorp",
    agents: [],
  };

  (view as unknown as { render(l: AgentListing): void }).render(listing);

  const card = host.querySelector(".agent-card");
  assert.ok(card);
  assert.equal(card.textContent?.includes("zorp"), true);
  assert.equal(card.textContent?.includes("Active"), true);
  assert.equal(card.getAttribute("data-active"), "true");
});

test("AgentsPaneView renders custom agents and scopes", () => {
  const { doc, host } = fixture();
  let current: string | null = "researcher";
  const callbacks = {
    onClose: () => {},
    onSelectAgent: async (name: string | null) => {
      current = name;
    },
    getCurrentAgent: () => current,
  };
  const view = new AgentsPaneView(doc, host, callbacks);

  const listing: AgentListing = {
    default: "zorp",
    agents: [
      {
        name: "researcher",
        description: "An evidence-based researcher agent.",
        scope: "user",
        model: "llama3",
        tools: ["search"],
        approval_preset: "strict",
        wants_privilege: false,
        privilege_summary: [],
        trusted: true,
        fully_applied: true,
        broken: null,
      },
      {
        name: "untrusted-agent",
        description: "An untrusted project agent requesting privilege.",
        scope: "workspace",
        model: null,
        tools: null,
        approval_preset: null,
        wants_privilege: true,
        privilege_summary: ["Allows bash command execution"],
        trusted: false,
        fully_applied: false,
        broken: null,
      },
    ],
  };

  (view as unknown as { render(l: AgentListing): void }).render(listing);

  const cards = host.querySelectorAll(".agent-card");
  assert.equal(cards.length, 3); // zorp default + 2 custom

  // Researcher card
  const researcherCard = cards[1];
  assert.equal(researcherCard.textContent?.includes("researcher"), true);
  assert.equal(researcherCard.textContent?.includes("user"), true);
  assert.equal(researcherCard.textContent?.includes("Active"), true);

  // Untrusted agent card
  const untrustedCard = cards[2];
  assert.equal(untrustedCard.textContent?.includes("untrusted-agent"), true);
  assert.equal(untrustedCard.textContent?.includes("workspace"), true);
  assert.equal(untrustedCard.textContent?.includes("untrusted"), true);
  assert.ok(untrustedCard.querySelector(".agent-trust-btn"));
});

test("AgentsPaneView selecting agent triggers onSelectAgent callback", async () => {
  const { doc, host } = fixture();
  let selectedName: string | null = "initial";
  const callbacks = {
    onClose: () => {},
    onSelectAgent: async (name: string | null) => {
      selectedName = name;
    },
    getCurrentAgent: () => "zorp",
  };
  const view = new AgentsPaneView(doc, host, callbacks);

  const listing: AgentListing = {
    default: "zorp",
    agents: [
      {
        name: "coder",
        description: "A fast coding agent.",
        scope: "user",
        model: "codellama",
        tools: [],
        approval_preset: null,
        wants_privilege: false,
        privilege_summary: [],
        trusted: true,
        fully_applied: true,
        broken: null,
      },
    ],
  };

  (view as unknown as { render(l: AgentListing): void }).render(listing);

  const cards = host.querySelectorAll(".agent-card");
  const coderCard = cards[1];

  // Click coder card
  coderCard.dispatchEvent(new (doc.defaultView!.Event)("click"));
  await new Promise((resolve) => setTimeout(resolve, 10));

  assert.equal(selectedName, "coder");

  // Click default zorp card
  const defaultCard = cards[0];
  defaultCard.dispatchEvent(new (doc.defaultView!.Event)("click"));
  await new Promise((resolve) => setTimeout(resolve, 10));

  assert.equal(selectedName, null);
});

test("AgentsPaneView notice banner renders safe text", () => {
  const { doc, host } = fixture();
  const callbacks = {
    onClose: () => {},
    onSelectAgent: async () => {},
    getCurrentAgent: () => null,
  };
  const view = new AgentsPaneView(doc, host, callbacks);

  view.setNotice("Agent is locked after conversation started.", true);
  const notice = host.querySelector(".agents-notice") as HTMLElement;
  assert.ok(notice);
  assert.equal(notice.hidden, false);
  assert.equal(notice.textContent, "Agent is locked after conversation started.");
  assert.ok(notice.className.includes("settings-callout-danger"));

  view.setNotice("");
  assert.equal(notice.hidden, true);
});
