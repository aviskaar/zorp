/**
 * The agents pane.
 *
 * An agent carries the model, the instructions, the tool allow-list and the
 * approval preset for a whole conversation, so these tests are about the
 * three things a wrong pane would do: offer a choice the server would
 * refuse, let somebody trust a file without reading what it wants, or let a
 * name out of a `.toml` become markup.
 */

import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";

import { agentState, agentSummary, renderAgents } from "../src/agents-view.ts";
import type { Agent, AgentListing } from "../src/api.ts";

function fixture(): { doc: Document; host: HTMLElement } {
  const dom = new JSDOM("<!doctype html><body><div id='host'></div></body>");
  const doc = dom.window.document as unknown as Document;
  return { doc, host: doc.getElementById("host") as HTMLElement };
}

function agent(over: Partial<Agent> = {}): Agent {
  return {
    name: "reviewer",
    scope: "user",
    description: "Reads and summarises, never writes.",
    model: "local-small",
    tools: ["read_file", "list_files"],
    approval_preset: "read-only",
    wants_privilege: false,
    privilege_summary: [],
    trusted: true,
    fully_applied: true,
    broken: null,
    ...over,
  };
}

function listing(over: Partial<AgentListing> = {}): AgentListing {
  return { agents: [agent()], default: "zorp", ...over };
}

const noop = { onPick: () => {}, onTrust: () => {} };

/* ------------------------------------------------------------------ */
/* the cards                                                           */
/* ------------------------------------------------------------------ */

/**
 * The default first. A list that only showed the alternatives would not
 * say what this conversation is running as now.
 */
test("the default card is first and is the active one when nothing is chosen", () => {
  const { doc, host } = fixture();
  renderAgents(doc, host, listing(), null, false, noop);

  const cards = [...host.querySelectorAll(".agent-card")];
  assert.equal(cards[0].getAttribute("data-agent"), "zorp");
  assert.equal(cards[0].getAttribute("data-active"), "yes");
  assert.equal(cards[1].getAttribute("data-active"), null);
});

test("a card says what the agent changes about a run", () => {
  const { doc, host } = fixture();
  renderAgents(doc, host, listing(), "reviewer", false, noop);

  const card = host.querySelector('[data-agent="reviewer"]')!;
  assert.match(card.textContent!, /Reads and summarises/);
  assert.match(card.textContent!, /model local-small/);
  assert.match(card.textContent!, /2 tools/);
  assert.match(card.textContent!, /approval read-only/);
  assert.equal(card.getAttribute("data-active"), "yes");
});

test("an agent with no overrides says so rather than showing an empty line", () => {
  assert.match(
    agentSummary(agent({ model: null, tools: null, approval_preset: null })),
    /its own instructions/,
  );
});

test("picking an agent calls back with its name, and the default with null", () => {
  const { doc, host } = fixture();
  const picked: (string | null)[] = [];
  renderAgents(doc, host, listing(), "reviewer", false, {
    onPick: (name) => picked.push(name),
    onTrust: () => {},
  });

  const zorp = host.querySelector('[data-agent="zorp"] button') as HTMLButtonElement;
  zorp.click();
  assert.deepEqual(picked, [null]);
});

/** A button that cannot do anything is worse than no button. */
test("the agent already in use offers no button that would do nothing", () => {
  const { doc, host } = fixture();
  renderAgents(doc, host, listing(), "reviewer", false, noop);

  const button = host.querySelector('[data-agent="reviewer"] button') as HTMLButtonElement;
  assert.equal(button.disabled, true);
  assert.match(button.textContent!, /Running under this/);
});

/* ------------------------------------------------------------------ */
/* the lock                                                            */
/* ------------------------------------------------------------------ */

/**
 * An agent carries the prompt and the tool set for a whole conversation, so
 * a transcript whose halves ran under different ones is one nobody can read
 * back honestly. The pane has to say what to do instead of only refusing.
 */
test("a conversation that has answered offers no way to change its agent", () => {
  const { doc, host } = fixture();
  renderAgents(doc, host, listing(), "reviewer", true, noop);

  const buttons = [...host.querySelectorAll("button")] as HTMLButtonElement[];
  assert.ok(buttons.length > 0);
  assert.ok(
    buttons.every((b) => b.disabled),
    "a locked conversation offered an enabled pick button",
  );
  assert.match(host.textContent!, /Branch it to carry on/);
});

/* ------------------------------------------------------------------ */
/* trust                                                               */
/* ------------------------------------------------------------------ */

const builder = () =>
  agent({
    name: "builder",
    scope: "workspace",
    description: "Fixes code and runs the tests.",
    tools: null,
    approval_preset: "full",
    wants_privilege: true,
    privilege_summary: ["run commands: cargo test", "loosen approval: preset full"],
    trusted: false,
    fully_applied: false,
  });

/**
 * The whole reason the gate exists. The model can write a file into
 * `<workspace>/.zorp/flavors/`, so an agent that arrived that way runs
 * without its command-bearing fields until a person reads what it wants.
 */
test("an untrusted workspace agent shows what it wants before offering to trust it", () => {
  const { doc, host } = fixture();
  renderAgents(doc, host, listing({ agents: [builder()] }), null, false, noop);

  const card = host.querySelector('[data-agent="builder"]')!;
  assert.ok(card.classList.contains("agent-untrusted"));
  assert.match(card.textContent!, /not applied/);
  assert.match(card.textContent!, /cargo test/);
  assert.match(card.textContent!, /preset full/);

  const trust = [...card.querySelectorAll("button")].find((b) =>
    b.textContent?.includes("Trust"),
  );
  assert.ok(trust, "no way to trust it");
});

test("trusting calls back with the agent", () => {
  const { doc, host } = fixture();
  const trusted: string[] = [];
  renderAgents(doc, host, listing({ agents: [builder()] }), null, false, {
    onPick: () => {},
    onTrust: (a) => trusted.push(a.name),
  });

  const button = [...host.querySelectorAll('[data-agent="builder"] button')].find((b) =>
    b.textContent?.includes("Trust"),
  ) as HTMLButtonElement;
  button.click();
  assert.deepEqual(trusted, ["builder"]);
});

/**
 * A user agent is already trusted because the person put the file there,
 * which is the rule the CLI applies. A button offering to trust one would
 * do nothing.
 */
test("a user agent is never offered a trust button", () => {
  const { doc, host } = fixture();
  const theirs = { ...builder(), scope: "user" as const, trusted: true };
  renderAgents(doc, host, listing({ agents: [theirs] }), null, false, noop);

  const buttons = [...host.querySelectorAll('[data-agent="builder"] button')];
  assert.equal(
    buttons.filter((b) => b.textContent?.includes("Trust")).length,
    0,
  );
});

test("an agent that wants nothing is ready without a trust step", () => {
  assert.equal(agentState(agent()).key, "ready");
  assert.equal(agentState(builder()).key, "untrusted");
  assert.equal(agentState(agent({ broken: "line 3: expected a table" })).key, "broken");
});

/* ------------------------------------------------------------------ */
/* broken and untrusted text                                           */
/* ------------------------------------------------------------------ */

/**
 * An agent that silently vanishes is a run that silently loses its
 * restrictions, so a file that did not parse is listed with the reason and
 * cannot be picked.
 */
test("a broken agent is listed with its error and cannot be chosen", () => {
  const { doc, host } = fixture();
  const bad = agent({ name: "typo", broken: "line 3: expected a table" });
  renderAgents(doc, host, listing({ agents: [bad] }), null, false, noop);

  const card = host.querySelector('[data-agent="typo"]')!;
  assert.ok(card.classList.contains("agent-broken"));
  assert.match(card.textContent!, /expected a table/);
  assert.equal(card.querySelectorAll("button").length, 0);
});

/**
 * A name comes out of a file that may have arrived by `git clone`. The
 * server scrubs the control and override characters; this asserts the page
 * does not turn what is left into markup.
 */
test("nothing in a card becomes markup", () => {
  const { doc, host } = fixture();
  const nasty = agent({
    name: "<img src=x onerror=alert(1)>",
    description: "<script>alert(1)</script>",
  });
  renderAgents(doc, host, listing({ agents: [nasty] }), null, false, noop);

  assert.equal(host.querySelectorAll("img").length, 0);
  assert.equal(host.querySelectorAll("script").length, 0);
  assert.ok(host.textContent!.includes("<img src=x onerror=alert(1)>"));
  assert.ok(host.textContent!.includes("<script>alert(1)</script>"));
});

test("no agents at all says where to put one", () => {
  const { doc, host } = fixture();
  renderAgents(doc, host, listing({ agents: [] }), null, false, noop);

  assert.match(host.textContent!, /No agents yet/);
  assert.match(host.textContent!, /flavors/);
  // The default is still there, because it is what the conversation runs as.
  assert.ok(host.querySelector('[data-agent="zorp"]'));
});
