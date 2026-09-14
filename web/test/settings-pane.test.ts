/**
 * The settings pane's four new sections.
 *
 * Two of these matter more than the rest. An MCP server's configuration is
 * where a token lives, and a settings page is exactly where somebody takes
 * a screenshot from. And three of the buttons here delete things that
 * cannot be recovered.
 *
 * Everything rendered is untrusted: an MCP name and command come out of a
 * TOML file that may have arrived by `git clone`, a path came off a
 * filesystem, a doctor detail is assembled from both.
 */

import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";

import {
  CONFIRM_WORD,
  DATA_ACTIONS,
  humanBytes,
  renderDataSection,
  renderDoctorSection,
  renderMcpSection,
} from "../src/settings-pane.ts";
import type { DataListing, DoctorReport, McpListing, McpServer } from "../src/api.ts";

function fixture(): { doc: Document; host: HTMLElement } {
  const dom = new JSDOM("<!doctype html><body><div id='host'></div></body>");
  const doc = dom.window.document as unknown as Document;
  return { doc, host: doc.getElementById("host") as HTMLElement };
}

/* ------------------------------------------------------------------ */
/* MCP                                                                 */
/* ------------------------------------------------------------------ */

function server(over: Partial<McpServer> = {}): McpServer {
  return {
    name: "github",
    transport: "streamable_http",
    command: null,
    args: [],
    url: "https://api.example.com/mcp",
    env_keys: ["GITHUB_TOKEN"],
    header_keys: ["Authorization"],
    trust: "sandbox",
    timeout_secs: 30,
    loaded: false,
    ...over,
  };
}

function mcp(over: Partial<McpListing> = {}): McpListing {
  return {
    servers: [server()],
    loads_servers: false,
    why: "zorp-web has no mcp feature, so a browser turn gets no MCP tools.",
    sources: ["/w/.zorp/mcp.toml", "ZORP_MCP_SERVERS"],
    warning: null,
    ...over,
  };
}

/**
 * The server sends key names only, and this asserts the page shows what it
 * was sent rather than inventing a place to put a value.
 */
test("the MCP section shows key names and has nowhere to put a value", () => {
  const { doc, host } = fixture();
  renderMcpSection(doc, host, mcp());

  assert.match(host.textContent!, /GITHUB_TOKEN/);
  assert.match(host.textContent!, /Authorization/);
  assert.match(host.textContent!, /never shows their values/);
  // No input anywhere: this is a report, and a field here would invite
  // somebody to paste a token into a page that would then hold one.
  assert.equal(host.querySelectorAll("input").length, 0);
});

/**
 * A listing that showed configured servers without saying this build loads
 * none would read as "these are working", which is the opposite of true.
 */
test("the MCP section says this build loads none of them", () => {
  const { doc, host } = fixture();
  renderMcpSection(doc, host, mcp());

  assert.match(host.textContent!, /no MCP tools/);
  assert.match(host.textContent!, /configured but not loaded/);
});

test("an empty MCP configuration says where zorp looked", () => {
  const { doc, host } = fixture();
  renderMcpSection(doc, host, mcp({ servers: [] }));

  assert.match(host.textContent!, /No MCP servers configured/);
  assert.match(host.textContent!, /mcp\.toml/);
  assert.match(host.textContent!, /ZORP_MCP_SERVERS/);
});

/** A server that silently vanishes is a tool that silently stops existing. */
test("a configuration file that did not parse is named", () => {
  const { doc, host } = fixture();
  renderMcpSection(doc, host, mcp({ servers: [], warning: "line 3: expected a table" }));

  assert.match(host.querySelector(".skills-warning")!.textContent!, /expected a table/);
});

test("nothing in the MCP section becomes markup", () => {
  const { doc, host } = fixture();
  renderMcpSection(
    doc,
    host,
    mcp({ servers: [server({ name: "<img src=x onerror=alert(1)>" })] }),
  );

  assert.equal(host.querySelectorAll("img").length, 0);
  assert.ok(host.textContent!.includes("<img src=x onerror=alert(1)>"));
});

/* ------------------------------------------------------------------ */
/* data                                                                */
/* ------------------------------------------------------------------ */

function data(over: Partial<DataListing> = {}): DataListing {
  return {
    files: [
      {
        label: "conversations",
        what: "every conversation both surfaces have had",
        path: "/state/zorp/sessions.db",
        env_var: "ZORP_STATE_DB",
        exists: true,
        bytes: 2048,
      },
      {
        label: "search index",
        what: "embeddings of past conversations",
        path: "/state/zorp/recall.db",
        env_var: "ZORP_RECALL_DB",
        exists: false,
        bytes: null,
      },
    ],
    reset_note: "ZORP_API_KEY is read from the environment, so resetting cannot unset it.",
    ...over,
  };
}

test("the data section names every file, its size and the variable that moves it", () => {
  const { doc, host } = fixture();
  renderDataSection(doc, host, data(), () => {});

  assert.match(host.textContent!, /sessions\.db/);
  assert.match(host.textContent!, /2\.0 KB/);
  assert.match(host.textContent!, /ZORP_STATE_DB/);
});

/** Absent and empty are different answers. */
test("a file that was never written reads as not created rather than zero", () => {
  const { doc, host } = fixture();
  renderDataSection(doc, host, data(), () => {});

  assert.match(host.textContent!, /not created/);
  assert.equal(humanBytes(null), "not created");
  assert.equal(humanBytes(0), "0 B");
});

/**
 * The load bearing one. Every action here is unrecoverable, and a button
 * that fires on the first click is one somebody hits while reading the
 * sentence that would have stopped them.
 */
test("nothing is deleted until the word is typed", () => {
  const { doc, host } = fixture();
  const fired: string[] = [];
  renderDataSection(doc, host, data(), (action) => fired.push(action.key));

  const row = host.querySelector('[data-action="conversations"]')!;
  const input = row.querySelector("input") as HTMLInputElement;
  const button = row.querySelector("button") as HTMLButtonElement;

  assert.equal(button.disabled, true, "the button starts enabled");
  button.click();
  assert.deepEqual(fired, [], "a disabled button fired anyway");

  input.value = "delet";
  input.dispatchEvent(new (doc.defaultView as Window & typeof globalThis).Event("input"));
  assert.equal(button.disabled, true, "a near miss enabled it");

  input.value = CONFIRM_WORD;
  input.dispatchEvent(new (doc.defaultView as Window & typeof globalThis).Event("input"));
  assert.equal(button.disabled, false);
  button.click();
  assert.deepEqual(fired, ["conversations"]);
});

/** A second deletion is a second decision. */
test("the confirmation resets itself after it fires", () => {
  const { doc, host } = fixture();
  const fired: string[] = [];
  renderDataSection(doc, host, data(), (action) => fired.push(action.key));

  const row = host.querySelector('[data-action="index"]')!;
  const input = row.querySelector("input") as HTMLInputElement;
  const button = row.querySelector("button") as HTMLButtonElement;

  input.value = CONFIRM_WORD;
  input.dispatchEvent(new (doc.defaultView as Window & typeof globalThis).Event("input"));
  button.click();

  assert.equal(input.value, "");
  assert.equal(button.disabled, true);
  button.click();
  assert.deepEqual(fired, ["index"], "a second click fired without a second confirmation");
});

/**
 * Somebody resetting settings is usually trying to get rid of a credential,
 * and the environment is the one that survives. The sentence goes before
 * the buttons, not after.
 */
test("the note about the key sits above the buttons", () => {
  const { doc, host } = fixture();
  renderDataSection(doc, host, data(), () => {});

  const note = host.querySelector(".settings-note")!;
  const danger = host.querySelector(".settings-danger")!;
  assert.match(note.textContent!, /ZORP_API_KEY/);
  assert.equal(
    note.compareDocumentPosition(danger) & 4,
    4,
    "the note is not before the buttons",
  );
});

/** A reset that reached into a workspace is the action nobody could undo. */
test("every action says what it takes, and reset says what it does not", () => {
  for (const action of DATA_ACTIONS) {
    assert.ok(action.what.length > 20, `${action.key} does not say what it takes`);
  }
  const everything = DATA_ACTIONS.find((a) => a.key === "everything")!;
  assert.match(everything.what, /workspace/);
  assert.match(everything.what, /scratch\//);
});

/* ------------------------------------------------------------------ */
/* this build                                                          */
/* ------------------------------------------------------------------ */

function doctor(over: Partial<DoctorReport> = {}): DoctorReport {
  return {
    version: "0.5.0",
    healthy: true,
    base_url: "http://localhost:11434/v1",
    checks: [
      { health: "note", label: "features", detail: "none (a default build)" },
      { health: "bad", label: "model", detail: "no model set" },
      { health: "note", label: "api key", detail: "ZORP_API_KEY is set" },
      {
        health: "note",
        label: "endpoint reachable",
        detail: "not checked. Add ?probe=1, or use Test connection.",
      },
    ],
    ...over,
  };
}

test("the build section marks a fault so it is findable without reading every line", () => {
  const { doc, host } = fixture();
  renderDoctorSection(doc, host, doctor(), () => {});

  assert.equal(host.querySelectorAll(".doctor-bad").length, 1);
  assert.match(host.querySelector(".doctor-bad")!.textContent!, /no model set/);
  assert.match(host.textContent!, /0\.5\.0/);
});

/**
 * The probe waits up to thirty seconds, so it is a button rather than part
 * of opening the pane.
 */
test("the endpoint is not probed until somebody asks", () => {
  const { doc, host } = fixture();
  let probes = 0;
  renderDoctorSection(doc, host, doctor(), () => {
    probes += 1;
  });

  assert.equal(probes, 0, "it probed on render");
  assert.match(host.textContent!, /not checked/);
  (host.querySelector("button") as HTMLButtonElement).click();
  assert.equal(probes, 1);
});

/**
 * The report is the thing people paste into bug reports. The server has the
 * test that greps its body; this one says the page adds nothing.
 */
test("the build section shows only what the server sent", () => {
  const { doc, host } = fixture();
  renderDoctorSection(doc, host, doctor(), () => {});

  assert.doesNotMatch(host.textContent!, /sk-/);
  assert.equal(host.querySelectorAll("input").length, 0);
});
