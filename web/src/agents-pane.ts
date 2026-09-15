/**
 * The agents pane: a side pane of named profiles a person picks a conversation to run under.
 *
 * An agent is a flavor with a description, picked before the conversation starts,
 * carrying the model, prompt, tool allow-list, and approval preset for the whole of it.
 *
 * Everything reaching the page goes through textContent.
 * Project scope agents that want privilege start untrusted until a person clicks "Trust this agent".
 * Locked once a conversation has answered; branching carries the agent.
 */

import {
  fetchAgent,
  fetchAgents,
  trustAgent,
  type AgentListing,
  type AgentSummary,
} from "./api.ts";

export interface AgentsPaneCallbacks {
  onClose: () => void;
  onSelectAgent: (name: string | null) => Promise<void>;
  getCurrentAgent: () => string | null;
}

export class AgentsPaneView {
  private readonly doc: Document;
  private readonly host: HTMLElement;
  private readonly callbacks: AgentsPaneCallbacks;
  private readonly listContainer: HTMLElement;
  private readonly statusNotice: HTMLElement;

  constructor(doc: Document, host: HTMLElement, callbacks: AgentsPaneCallbacks) {
    this.doc = doc;
    this.host = host;
    this.callbacks = callbacks;

    this.statusNotice = this.el("div", "agents-notice");
    this.statusNotice.hidden = true;
    this.listContainer = this.el("div", "agents-list");

    this.host.replaceChildren(this.statusNotice, this.listContainer);
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

  setNotice(message: string, isError = false): void {
    if (!message) {
      this.statusNotice.hidden = true;
      this.statusNotice.textContent = "";
      return;
    }
    this.statusNotice.hidden = false;
    this.statusNotice.className = isError
      ? "settings-note settings-callout-danger agents-notice"
      : "settings-note agents-notice";
    this.statusNotice.textContent = message;
  }

  async refresh(): Promise<void> {
    try {
      const listing = await fetchAgents();
      this.render(listing);
    } catch (err) {
      this.listContainer.replaceChildren(
        this.text("p", "settings-error", `Could not list agents: ${String(err)}`),
      );
    }
  }

  private render(listing: AgentListing): void {
    const container = this.el("div", "agents-cards");
    const currentAgent = this.callbacks.getCurrentAgent();

    // 1. Default card: "zorp"
    const isDefaultActive = !currentAgent || currentAgent === listing.default;
    const defaultCard = this.el("div", "agent-card");
    defaultCard.dataset.active = isDefaultActive ? "true" : "false";

    const defHead = this.el("div", "agent-card-head");
    defHead.append(this.text("h3", "agent-card-title", listing.default));
    defHead.append(this.text("span", "badge badge-default", "default"));
    if (isDefaultActive) {
      defHead.append(this.text("span", "badge badge-ok", "Active"));
    }
    defaultCard.append(defHead);

    defaultCard.append(
      this.text(
        "p",
        "agent-card-desc",
        "The default zorp assistant. Standard prompt and tool set, asks before modifying this machine.",
      ),
    );

    defaultCard.addEventListener("click", async () => {
      this.setNotice("");
      try {
        await this.callbacks.onSelectAgent(null);
        await this.refresh();
      } catch (err) {
        this.setNotice(String(err), true);
      }
    });
    container.append(defaultCard);

    // 2. Configured agents
    for (const agent of listing.agents) {
      const isCurrent = currentAgent === agent.name;
      const card = this.createAgentCard(agent, isCurrent);
      container.append(card);
    }

    this.listContainer.replaceChildren(container);
  }

  private createAgentCard(agent: AgentSummary, isActive: boolean): HTMLElement {
    const card = this.el("div", "agent-card");
    card.dataset.active = isActive ? "true" : "false";

    const head = this.el("div", "agent-card-head");
    head.append(this.text("h3", "agent-card-title", agent.name));

    const scopeBadge = this.text(
      "span",
      "badge badge-scope",
      agent.scope === "workspace" ? "workspace" : "user",
    );
    head.append(scopeBadge);

    if (agent.scope === "workspace") {
      if (agent.wants_privilege) {
        const trustBadge = this.text(
          "span",
          `badge ${agent.trusted ? "badge-trusted" : "badge-untrusted"}`,
          agent.trusted ? "trusted" : "untrusted",
        );
        head.append(trustBadge);
      }
    }

    if (isActive) {
      head.append(this.text("span", "badge badge-ok", "Active"));
    }
    card.append(head);

    if (agent.description) {
      card.append(this.text("p", "agent-card-desc", agent.description));
    }

    if (agent.broken) {
      const brokenBox = this.el("div", "settings-note settings-callout-danger");
      brokenBox.textContent = `Broken: ${agent.broken}`;
      card.append(brokenBox);
    }

    // Properties metadata
    const meta = this.el("div", "agent-card-meta");
    if (agent.model) {
      meta.append(this.text("span", "badge badge-default", `model: ${agent.model}`));
    }
    if (agent.tools && agent.tools.length > 0) {
      meta.append(this.text("span", "badge badge-default", `${agent.tools.length} tools`));
    }
    if (agent.approval_preset) {
      meta.append(
        this.text("span", "badge badge-default", `preset: ${agent.approval_preset}`),
      );
    }
    if (meta.children.length > 0) {
      card.append(meta);
    }

    // Untrusted workspace agent wanting privilege
    if (agent.scope === "workspace" && agent.wants_privilege && !agent.trusted) {
      const privBox = this.el("div", "agent-privilege-box");
      privBox.append(
        this.text(
          "span",
          "agent-priv-lead",
          "This workspace agent requests commands or looser approvals:",
        ),
      );

      const summaryList = this.el("ul", "agent-priv-list");
      for (const line of agent.privilege_summary) {
        summaryList.append(this.text("li", "", line));
      }
      privBox.append(summaryList);

      const trustBtn = this.el("button", "btn btn-allow agent-trust-btn");
      trustBtn.type = "button";
      trustBtn.textContent = "Trust this agent";
      trustBtn.addEventListener("click", async (ev) => {
        ev.stopPropagation();
        trustBtn.disabled = true;
        trustBtn.textContent = "Trusting...";
        try {
          await trustAgent(agent.scope, agent.name);
          await this.refresh();
        } catch (err) {
          this.setNotice(String(err), true);
        }
      });
      privBox.append(trustBtn);
      card.append(privBox);
    }

    // Expandable System Prompt details
    const promptDetails = this.el("details", "agent-prompt-details");
    const promptSummary = this.el("summary", "agent-prompt-summary");
    promptSummary.textContent = "View system prompt";
    promptDetails.append(promptSummary);

    const promptBody = this.el("pre", "agent-prompt-body");
    promptBody.textContent = "Loading prompt...";

    let loaded = false;
    promptDetails.addEventListener("toggle", () => {
      if (promptDetails.open && !loaded) {
        loaded = true;
        void fetchAgent(agent.scope, agent.name)
          .then((detail) => {
            promptBody.textContent = detail.system_prompt || "(No system prompt configured)";
          })
          .catch((e) => {
            promptBody.textContent = `Could not load prompt: ${String(e)}`;
          });
      }
    });

    promptDetails.addEventListener("click", (ev) => {
      ev.stopPropagation();
    });

    promptDetails.append(promptBody);
    card.append(promptDetails);

    // Click card to select agent
    card.addEventListener("click", async () => {
      this.setNotice("");
      try {
        await this.callbacks.onSelectAgent(agent.name);
        await this.refresh();
      } catch (err) {
        this.setNotice(String(err), true);
      }
    });

    return card;
  }
}
