/**
 * Tests for the settings pane: sections, data state, MCP listings, and doctor report.
 */

import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";

import { formatBytes, SettingsPaneView } from "../src/settings-pane.ts";
import type { DataState, DoctorReport, McpListing } from "../src/api.ts";

const MARKUP = `
<!doctype html><body>
  <div id="settings-pane">
    <div id="settings-section-model"></div>
    <div id="settings-section-workspace"></div>
    <div id="settings-section-skills"></div>
    <div id="settings-skills-host"></div>
    <div id="settings-section-mcp"></div>
    <div id="settings-mcp-host"></div>
    <div id="settings-section-data"></div>
    <div id="settings-data-host"></div>
    <div id="settings-section-doctor"></div>
    <div id="settings-doctor-host"></div>
    <div id="settings-section-approval"></div>
  </div>
</body>`;

function fixture(): { doc: Document; host: HTMLElement } {
  const dom = new JSDOM(MARKUP);
  const doc = dom.window.document as unknown as Document;
  const host = doc.getElementById("settings-pane")!;
  return { doc, host };
}

test("formatBytes formats byte sizes appropriately", () => {
  assert.equal(formatBytes(null), "empty");
  assert.equal(formatBytes(undefined), "empty");
  assert.equal(formatBytes(-1), "empty");
  assert.equal(formatBytes(0), "0 B");
  assert.equal(formatBytes(512), "512 B");
  assert.equal(formatBytes(1024), "1.0 KB");
  assert.equal(formatBytes(1048576), "1.0 MB");
  assert.equal(formatBytes(1073741824), "1.0 GB");
});

test("SettingsPaneView mounts containers to host elements", () => {
  const { doc, host } = fixture();
  const callbacks = {
    onClose: () => {},
    onResetAll: async () => {},
    onClearSessions: async () => {},
  };
  const view = new SettingsPaneView(doc, host, callbacks);

  const skillsHost = host.querySelector("#settings-skills-host");
  const mcpHost = host.querySelector("#settings-mcp-host");
  const dataHost = host.querySelector("#settings-data-host");
  const doctorHost = host.querySelector("#settings-doctor-host");

  assert.ok(skillsHost?.querySelector(".settings-skills-container"));
  assert.ok(mcpHost?.querySelector(".settings-mcp-container"));
  assert.ok(dataHost?.querySelector(".settings-data-container"));
  assert.ok(doctorHost?.querySelector(".settings-doctor-container"));
});

test("SettingsPaneView scrolls to target sections", () => {
  const { doc, host } = fixture();
  const callbacks = {
    onClose: () => {},
    onResetAll: async () => {},
    onClearSessions: async () => {},
  };
  const view = new SettingsPaneView(doc, host, callbacks);

  const target = host.querySelector("#settings-section-workspace")!;
  let scrolled = false;
  target.scrollIntoView = () => {
    scrolled = true;
  };

  view.scrollTo("workspace");
  assert.equal(scrolled, true);
});

test("SettingsPaneView renders data state with typed confirmation delete requirement", async () => {
  const { doc, host } = fixture();
  let cleared = false;
  const callbacks = {
    onClose: () => {},
    onResetAll: async () => {},
    onClearSessions: async () => {
      cleared = true;
    },
  };
  const view = new SettingsPaneView(doc, host, callbacks);

  const state: DataState = {
    files: [
      {
        label: "Conversations",
        what: "sqlite database",
        path: "/path/to/conversations.db",
        env_var: null,
        exists: true,
        bytes: 2048,
      },
    ],
    reset_note: "Resetting settings leaves API key.",
  };

  // Call private renderData method via any
  (view as unknown as { renderData(s: DataState): void }).renderData(state);

  const table = host.querySelector(".data-table");
  assert.ok(table);
  assert.equal(table.textContent?.includes("Conversations"), true);
  assert.equal(table.textContent?.includes("2.0 KB"), true);

  const actionRows = host.querySelectorAll(".data-action-row");
  assert.ok(actionRows.length >= 4);

  const firstRow = actionRows[0];
  const input = firstRow.querySelector<HTMLInputElement>(".data-confirm-input")!;
  const btn = firstRow.querySelector<HTMLButtonElement>("button")!;

  assert.equal(btn.disabled, true);

  // Type wrong word
  input.value = "del";
  input.dispatchEvent(new (doc.defaultView!.Event)("input"));
  assert.equal(btn.disabled, true);

  // Type "delete"
  input.value = "delete";
  input.dispatchEvent(new (doc.defaultView!.Event)("input"));
  assert.equal(btn.disabled, false);

  // Click confirm
  btn.click();
  await new Promise((resolve) => setTimeout(resolve, 10));
  assert.equal(cleared, true);
});

test("SettingsPaneView renders MCP listing with textContent only", () => {
  const { doc, host } = fixture();
  const callbacks = {
    onClose: () => {},
    onResetAll: async () => {},
    onClearSessions: async () => {},
  };
  const view = new SettingsPaneView(doc, host, callbacks);

  const listing: McpListing = {
    loads_servers: true,
    why: "MCP active",
    warning: null,
    servers: [
      {
        name: "cloudflare-docs",
        transport: "stdio",
        command: ["npx", "-y", "mcp-server"],
        args: ["--param", "val"],
        url: null,
        env_keys: ["CF_ACCOUNT_ID"],
        header_keys: [],
        loaded: true,
      },
    ],
    sources: [".zorp/mcp.toml"],
  };

  (view as unknown as { renderMcp(l: McpListing): void }).renderMcp(listing);

  const card = host.querySelector(".mcp-card");
  assert.ok(card);
  assert.equal(card.textContent?.includes("cloudflare-docs"), true);
  assert.equal(card.textContent?.includes("CF_ACCOUNT_ID"), true);
  assert.equal(card.textContent?.includes("npx -y mcp-server --param val"), true);
});

test("SettingsPaneView renders doctor report and handles probe button", () => {
  const { doc, host } = fixture();
  const callbacks = {
    onClose: () => {},
    onResetAll: async () => {},
    onClearSessions: async () => {},
  };
  const view = new SettingsPaneView(doc, host, callbacks);

  const report: DoctorReport = {
    healthy: true,
    version: "0.1.0",
    checks: [
      {
        label: "Model endpoint",
        health: "ok",
        detail: "http://localhost:11434/v1 reachable",
      },
      {
        label: "Recall search",
        health: "off",
        detail: "recall feature not compiled in",
      },
    ],
  };

  (view as unknown as { renderDoctor(r: DoctorReport): void }).renderDoctor(report);

  const doctorView = host.querySelector(".doctor-view");
  assert.ok(doctorView);
  assert.equal(doctorView.textContent?.includes("Healthy"), true);
  assert.equal(doctorView.textContent?.includes("v0.1.0"), true);
  assert.equal(doctorView.textContent?.includes("Model endpoint"), true);

  const probeBtn = doctorView.querySelector<HTMLButtonElement>(".doctor-probe-btn")!;
  assert.ok(probeBtn);
  probeBtn.click();
  assert.equal(probeBtn.disabled, true);
  assert.equal(probeBtn.textContent, "Probing...");
});
