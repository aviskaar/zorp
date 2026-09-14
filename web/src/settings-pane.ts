/**
 * The settings pane: model, workspace, skills, MCP servers, data, and this build.
 *
 * Sits in the right column in place of the old model and workspace modal overlays.
 * A pane rather than a dialog because it holds seven sections that a person reads,
 * scrolls, and returns to.
 *
 * Everything reaching the page goes through textContent.
 * MCP env and headers values never leave the server.
 * The API key is never rendered or output.
 * Destructive actions require typing "delete" into a confirmation input.
 */

import {
  deleteRecallIndex,
  fetchActiveSkills,
  fetchDataState,
  fetchDoctor,
  fetchMcp,
  fetchSkills,
  resetSettings,
  type ActiveSkillListing,
  type DataState,
  type DoctorReport,
  type McpListing,
} from "./api.ts";
import { renderSkillsPanel } from "./skills-view.ts";

export function formatBytes(bytes: number | null | undefined): string {
  if (bytes === null || bytes === undefined || bytes < 0) {
    return "empty";
  }
  if (bytes === 0) {
    return "0 B";
  }
  const units = ["B", "KB", "MB", "GB"];
  let i = 0;
  let val = bytes;
  while (val >= 1024 && i < units.length - 1) {
    val /= 1024;
    i++;
  }
  return `${val.toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
}

export interface SettingsPaneCallbacks {
  onClose: () => void;
  onResetAll: () => Promise<void>;
  onClearSessions: () => Promise<void>;
}

export class SettingsPaneView {
  private readonly doc: Document;
  private readonly host: HTMLElement;
  private readonly callbacks: SettingsPaneCallbacks;

  // Section containers
  private readonly skillsContainer: HTMLElement;
  private readonly mcpContainer: HTMLElement;
  private readonly dataContainer: HTMLElement;
  private readonly doctorContainer: HTMLElement;

  constructor(doc: Document, host: HTMLElement, callbacks: SettingsPaneCallbacks) {
    this.doc = doc;
    this.host = host;
    this.callbacks = callbacks;

    this.skillsContainer = this.el("div", "settings-skills-container");
    this.mcpContainer = this.el("div", "settings-mcp-container");
    this.dataContainer = this.el("div", "settings-data-container");
    this.doctorContainer = this.el("div", "settings-doctor-container");

    const skillsHost = this.host.querySelector("#settings-skills-host");
    if (skillsHost) {
      skillsHost.replaceChildren(this.skillsContainer);
    } else {
      this.host.append(this.skillsContainer);
    }

    const mcpHost = this.host.querySelector("#settings-mcp-host");
    if (mcpHost) {
      mcpHost.replaceChildren(this.mcpContainer);
    } else {
      this.host.append(this.mcpContainer);
    }

    const dataHost = this.host.querySelector("#settings-data-host");
    if (dataHost) {
      dataHost.replaceChildren(this.dataContainer);
    } else {
      this.host.append(this.dataContainer);
    }

    const doctorHost = this.host.querySelector("#settings-doctor-host");
    if (doctorHost) {
      doctorHost.replaceChildren(this.doctorContainer);
    } else {
      this.host.append(this.doctorContainer);
    }
  }

  private el<K extends keyof HTMLElementTagNameMap>(
    tag: K,
    className = "",
  ): HTMLElementTagNameMap[K] {
    const node = this.doc.createElement(tag);
    if (className) {
      node.className = className;
    }
    return node;
  }

  private text<K extends keyof HTMLElementTagNameMap>(
    tag: K,
    className: string,
    value: string,
  ): HTMLElementTagNameMap[K] {
    const node = this.el(tag, className);
    node.textContent = value;
    return node;
  }

  /**
   * Scroll to a specific section by its identifier: "model", "workspace",
   * "skills", "mcp", "data", "doctor", "approval".
   */
  scrollTo(sectionId: string): void {
    const target = this.host.querySelector(`#settings-section-${sectionId}`);
    if (target) {
      target.scrollIntoView({ behavior: "smooth", block: "start" });
    }
  }

  /** Refresh dynamic sections when opening or returning to the pane. */
  async refresh(sessionId: string | null): Promise<void> {
    await Promise.allSettled([
      this.refreshSkills(sessionId),
      this.refreshMcp(),
      this.refreshData(),
      this.refreshDoctor(false),
    ]);
  }

  async refreshSkills(sessionId: string | null): Promise<void> {
    try {
      const installed = await fetchSkills();
      let live: ActiveSkillListing | null = null;
      if (sessionId) {
        try {
          live = await fetchActiveSkills(sessionId);
        } catch {
          live = null;
        }
      }
      this.skillsContainer.replaceChildren();
      renderSkillsPanel(this.doc, this.skillsContainer, installed, live);
    } catch (err) {
      this.skillsContainer.replaceChildren(
        this.text("p", "settings-error", String(err)),
      );
    }
  }

  async refreshMcp(): Promise<void> {
    try {
      const listing = await fetchMcp();
      this.renderMcp(listing);
    } catch (err) {
      this.mcpContainer.replaceChildren(
        this.text("p", "settings-error", String(err)),
      );
    }
  }

  private renderMcp(listing: McpListing): void {
    const container = this.el("div", "mcp-view");

    if (!listing.loads_servers) {
      const notice = this.el("div", "settings-note settings-callout-warn");
      notice.textContent = listing.why;
      container.append(notice);
    }

    if (listing.warning) {
      const warn = this.el("div", "settings-note settings-callout-danger");
      warn.textContent = listing.warning;
      container.append(warn);
    }

    if (listing.servers.length === 0) {
      container.append(
        this.text(
          "p",
          "settings-empty",
          "No MCP servers configured in .zorp/mcp.toml or ZORP_MCP_SERVERS.",
        ),
      );
    } else {
      const list = this.el("div", "mcp-list");
      for (const server of listing.servers) {
        const card = this.el("div", "mcp-card");

        const head = this.el("div", "mcp-card-head");
        head.append(this.text("span", "mcp-name", server.name));
        const transportBadge = this.text("span", "badge badge-note", server.transport);
        head.append(transportBadge);
        if (!server.loaded) {
          head.append(this.text("span", "badge badge-default", "not loaded in browser"));
        }
        card.append(head);

        if (server.command && server.command.length > 0) {
          const cmdLine = server.args
            ? `${server.command.join(" ")} ${server.args.join(" ")}`
            : server.command.join(" ");
          const cmdBox = this.el("div", "mcp-cmd");
          cmdBox.append(this.text("span", "mcp-cmd-label", "Command: "));
          cmdBox.append(this.text("code", "mcp-cmd-code", cmdLine));
          card.append(cmdBox);
        } else if (server.url) {
          const urlBox = this.el("div", "mcp-cmd");
          urlBox.append(this.text("span", "mcp-cmd-label", "URL: "));
          urlBox.append(this.text("code", "mcp-cmd-code", server.url));
          card.append(urlBox);
        }

        if (server.env_keys.length > 0) {
          const envRow = this.el("div", "mcp-keys");
          envRow.append(this.text("span", "mcp-keys-label", "Environment keys: "));
          envRow.append(this.text("span", "mcp-keys-list", server.env_keys.join(", ")));
          card.append(envRow);
        }

        if (server.header_keys.length > 0) {
          const hdrRow = this.el("div", "mcp-keys");
          hdrRow.append(this.text("span", "mcp-keys-label", "Header keys: "));
          hdrRow.append(this.text("span", "mcp-keys-list", server.header_keys.join(", ")));
          card.append(hdrRow);
        }

        list.append(card);
      }
      container.append(list);
    }

    if (listing.sources.length > 0) {
      const srcBox = this.el("div", "mcp-sources");
      srcBox.append(this.text("span", "mcp-sources-label", "Read from: "));
      srcBox.append(this.text("span", "mcp-sources-list", listing.sources.join(", ")));
      container.append(srcBox);
    }

    this.mcpContainer.replaceChildren(container);
  }

  async refreshData(): Promise<void> {
    try {
      const state = await fetchDataState();
      this.renderData(state);
    } catch (err) {
      this.dataContainer.replaceChildren(
        this.text("p", "settings-error", String(err)),
      );
    }
  }

  private renderData(state: DataState): void {
    const container = this.el("div", "data-view");

    // Files table
    const table = this.el("table", "data-table");
    const thead = this.el("thead");
    const hrow = this.el("tr");
    hrow.append(
      this.text("th", "", "What"),
      this.text("th", "", "Path"),
      this.text("th", "", "Size"),
    );
    thead.append(hrow);
    table.append(thead);

    const tbody = this.el("tbody");
    for (const file of state.files) {
      const row = this.el("tr");
      const whatCell = this.el("td");
      whatCell.append(this.text("strong", "data-label", file.label));
      whatCell.append(this.text("div", "data-desc", file.what));

      const pathCell = this.el("td");
      pathCell.append(this.text("div", "data-path", file.path));
      if (file.env_var) {
        pathCell.append(this.text("div", "data-env", `override: ${file.env_var}`));
      }

      const sizeCell = this.text(
        "td",
        "data-size",
        file.exists ? formatBytes(file.bytes) : "empty",
      );

      row.append(whatCell, pathCell, sizeCell);
      tbody.append(row);
    }
    table.append(tbody);
    container.append(table);

    // Reset note
    const note = this.el("p", "settings-note");
    note.textContent = state.reset_note;
    container.append(note);

    // Destructive actions section
    const actionsGroup = this.el("div", "data-actions-group");
    actionsGroup.append(this.text("h4", "data-actions-title", "Clear data"));

    // 1. Clear conversations
    actionsGroup.append(
      this.createActionRow({
        title: "Clear conversations",
        description: "Deletes all conversation transcripts, turns, and project labels.",
        buttonText: "Clear conversations",
        btnClass: "btn-danger",
        onConfirm: async (statusEl) => {
          try {
            await this.callbacks.onClearSessions();
            statusEl.textContent = "All conversations cleared.";
            statusEl.className = "settings-result settings-result-ok";
            await this.refreshData();
          } catch (e) {
            statusEl.textContent = String(e);
            statusEl.className = "settings-result settings-result-err";
          }
        },
      }),
    );

    // 2. Delete search index
    actionsGroup.append(
      this.createActionRow({
        title: "Delete search index",
        description: "Deletes the vector database used for conversation recall. Recomputed on next sweep.",
        buttonText: "Delete search index",
        btnClass: "btn-danger",
        onConfirm: async (statusEl) => {
          try {
            await deleteRecallIndex();
            statusEl.textContent = "Search index deleted.";
            statusEl.className = "settings-result settings-result-ok";
            await this.refreshData();
          } catch (e) {
            statusEl.textContent = String(e);
            statusEl.className = "settings-result settings-result-err";
          }
        },
      }),
    );

    // 3. Reset settings
    actionsGroup.append(
      this.createActionRow({
        title: "Reset settings",
        description:
          "Removes zorp.toml and trust file, resetting settings to environment defaults. Leaves API key intact.",
        buttonText: "Reset settings",
        btnClass: "btn-danger",
        onConfirm: async (statusEl) => {
          try {
            await resetSettings();
            statusEl.textContent = "Settings reset to defaults. API key preserved in environment.";
            statusEl.className = "settings-result settings-result-ok";
            await this.refreshData();
          } catch (e) {
            statusEl.textContent = String(e);
            statusEl.className = "settings-result settings-result-err";
          }
        },
      }),
    );

    // 4. Reset everything
    actionsGroup.append(
      this.createActionRow({
        title: "Reset everything",
        description:
          "Clears conversations, deletes the search index, and resets settings sequentially.",
        buttonText: "Reset everything",
        btnClass: "btn-danger",
        onConfirm: async (statusEl) => {
          try {
            await this.callbacks.onResetAll();
            statusEl.textContent = "Everything reset.";
            statusEl.className = "settings-result settings-result-ok";
            await this.refreshData();
          } catch (e) {
            statusEl.textContent = String(e);
            statusEl.className = "settings-result settings-result-err";
          }
        },
      }),
    );

    container.append(actionsGroup);
    this.dataContainer.replaceChildren(container);
  }

  private createActionRow(opt: {
    title: string;
    description: string;
    buttonText: string;
    btnClass: string;
    onConfirm: (statusEl: HTMLElement) => Promise<void>;
  }): HTMLElement {
    const row = this.el("div", "data-action-row");
    row.append(this.text("div", "data-action-title", opt.title));
    row.append(this.text("div", "data-action-desc", opt.description));

    const form = this.el("div", "data-action-confirm");
    const input = this.el("input", "data-confirm-input");
    input.type = "text";
    input.placeholder = 'type "delete" to confirm';
    input.autocomplete = "off";
    input.spellcheck = false;

    const btn = this.el("button", `btn ${opt.btnClass}`);
    btn.type = "button";
    btn.textContent = opt.buttonText;
    btn.disabled = true;

    const statusEl = this.el("div", "settings-result");

    input.addEventListener("input", () => {
      btn.disabled = input.value.trim().toLowerCase() !== "delete";
    });

    btn.addEventListener("click", async () => {
      if (input.value.trim().toLowerCase() !== "delete") {
        return;
      }
      btn.disabled = true;
      input.value = "";
      statusEl.textContent = "Working...";
      await opt.onConfirm(statusEl);
    });

    form.append(input, btn);
    row.append(form, statusEl);
    return row;
  }

  async refreshDoctor(probe: boolean): Promise<void> {
    try {
      const report = await fetchDoctor(probe);
      this.renderDoctor(report);
    } catch (err) {
      this.doctorContainer.replaceChildren(
        this.text("p", "settings-error", String(err)),
      );
    }
  }

  private renderDoctor(report: DoctorReport): void {
    const container = this.el("div", "doctor-view");

    const summaryRow = this.el("div", "doctor-summary");
    const healthBadge = this.text(
      "span",
      `badge ${report.healthy ? "badge-ok" : "badge-warn"}`,
      report.healthy ? "Healthy" : "Needs attention",
    );
    summaryRow.append(healthBadge);
    summaryRow.append(this.text("span", "doctor-version", `v${report.version}`));

    const probeBtn = this.el("button", "btn btn-deny doctor-probe-btn");
    probeBtn.type = "button";
    probeBtn.textContent = "Probe endpoint";
    probeBtn.addEventListener("click", () => {
      probeBtn.disabled = true;
      probeBtn.textContent = "Probing...";
      void this.refreshDoctor(true);
    });
    summaryRow.append(probeBtn);
    container.append(summaryRow);

    const checksList = this.el("div", "doctor-checks");
    for (const check of report.checks) {
      const row = this.el("div", "doctor-check-row");

      const badgeClass =
        check.health === "ok"
          ? "badge-ok"
          : check.health === "bad"
            ? "badge-bad"
            : check.health === "off"
              ? "badge-default"
              : "badge-note";

      row.append(this.text("span", `badge ${badgeClass}`, check.health));
      row.append(this.text("span", "doctor-check-label", check.label));
      row.append(this.text("span", "doctor-check-detail", check.detail));

      checksList.append(row);
    }
    container.append(checksList);

    this.doctorContainer.replaceChildren(container);
  }

  getSkillsHost(): HTMLElement {
    return this.skillsContainer;
  }

  getMcpHost(): HTMLElement {
    return this.mcpContainer;
  }

  getDataHost(): HTMLElement {
    return this.dataContainer;
  }

  getDoctorHost(): HTMLElement {
    return this.doctorContainer;
  }
}
