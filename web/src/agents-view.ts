/**
 * The agents pane: named profiles a person picks a conversation to run
 * under.
 *
 * An agent is a flavor with a description. Picking one is a person's
 * decision, before the conversation starts, and it carries the model, the
 * system prompt, the tool allow-list and the approval preset for the whole
 * of it. Nothing here is chosen by a model and there is no control that
 * could be; `agent.rs` has `no_tool_picks_an_agent_or_trusts_one` saying so
 * from the other side.
 *
 * **Everything goes through `textContent`.** A name and a description come
 * out of a `.toml` that may have arrived by `git clone`. The server scrubs
 * them of control and bidirectional characters before sending, because an
 * override inside a name reorders every row after it and that is how one
 * agent impersonates another in a list somebody is picking from. This
 * module builds no HTML strings and must never start to.
 *
 * `doc` is passed in rather than taken from the global, the same as
 * `skills-view`, so a test can render into a jsdom document and read back
 * what actually landed.
 */

import type { Agent, AgentListing } from "./api.ts";

/** What a scope is called on the page. */
export const SCOPE_LABELS: Record<Agent["scope"], string> = {
  user: "yours, everywhere",
  workspace: "this workspace",
};

/**
 * The three states a card can be in, and the sentence for each.
 *
 * The middle one is the whole reason the trust gate exists. The model can
 * write a file into `<workspace>/.zorp/flavors/`, so an agent that arrived
 * that way runs with its safe fields and without the ones that carry shell
 * commands, until a person reads what it wants and clicks.
 */
export function agentState(agent: Agent): {
  key: "broken" | "untrusted" | "ready";
  note: string;
} {
  if (agent.broken) {
    return { key: "broken", note: `This file could not be read: ${agent.broken}` };
  }
  if (agent.wants_privilege && !agent.trusted) {
    return {
      key: "untrusted",
      note: "Not trusted yet, so the settings below are not applied. Read them and trust it to turn them on.",
    };
  }
  return { key: "ready", note: "" };
}

/** What an agent changes about a run, for the line under its description. */
export function agentSummary(agent: Agent): string {
  const parts: string[] = [];
  if (agent.model) {
    parts.push(`model ${agent.model}`);
  }
  if (agent.tools) {
    // An agent can only narrow. `register_builtins_filtered` picks from what
    // the build has and cannot add a tool it lacks.
    parts.push(`${agent.tools.length} tool${agent.tools.length === 1 ? "" : "s"}`);
  }
  if (agent.approval_preset) {
    parts.push(`approval ${agent.approval_preset}`);
  }
  return parts.length ? parts.join(", ") : "the defaults, with its own instructions";
}

function el(doc: Document, tag: string, className: string): HTMLElement {
  const node = doc.createElement(tag);
  node.className = className;
  return node;
}

function textNode(doc: Document, tag: string, className: string, value: string): HTMLElement {
  const node = el(doc, tag, className);
  node.textContent = value;
  return node;
}

export interface AgentsHandlers {
  /** A person picked this agent for the conversation. `null` is the default. */
  onPick: (name: string | null) => void;
  /** A person read what a workspace agent wants and trusted it. */
  onTrust: (agent: Agent) => void;
}

/**
 * Draw the cards.
 *
 * `chosen` is the conversation's current agent, or `null` for the default.
 * `locked` is whether the conversation has already answered: an agent
 * carries the prompt and the tool set for a whole conversation, and a
 * transcript whose halves ran under different ones is one nobody can read
 * back honestly. Branching is the way to change it, and a branch copies the
 * agent, so the message says that rather than just refusing.
 */
export function renderAgents(
  doc: Document,
  host: HTMLElement,
  listing: AgentListing,
  chosen: string | null,
  locked: boolean,
  handlers: AgentsHandlers,
): void {
  host.replaceChildren();

  if (locked) {
    host.append(
      textNode(
        doc,
        "p",
        "settings-note",
        "This conversation has already answered, so its agent is fixed. Branch it to carry on under a different one.",
      ),
    );
  }

  // The default first, because it is what every conversation runs as and a
  // list that only showed the alternatives would not say what you have now.
  host.append(
    card(
      doc,
      {
        name: listing.default,
        scope: "user",
        description: "zorp as it comes: every tool this build has, and the default instructions.",
        model: null,
        tools: null,
        approval_preset: null,
        wants_privilege: false,
        privilege_summary: [],
        trusted: true,
        fully_applied: true,
        broken: null,
      },
      chosen === null,
      locked,
      handlers,
      true,
    ),
  );

  for (const scope of ["user", "workspace"] as Agent["scope"][]) {
    const inScope = listing.agents.filter((a) => a.scope === scope);
    if (!inScope.length) {
      continue;
    }
    host.append(textNode(doc, "div", "skills-scope", SCOPE_LABELS[scope]));
    for (const agent of inScope) {
      host.append(card(doc, agent, chosen === agent.name, locked, handlers, false));
    }
  }

  if (!listing.agents.length) {
    host.append(
      textNode(
        doc,
        "p",
        "skills-empty",
        "No agents yet. An agent is a flavor with a description: put one in " +
          "~/.config/zorp/flavors/<name>.toml, or in .zorp/flavors/<name>.toml in this workspace.",
      ),
    );
  }
}

function card(
  doc: Document,
  agent: Agent,
  active: boolean,
  locked: boolean,
  handlers: AgentsHandlers,
  isDefault: boolean,
): HTMLElement {
  const state = agentState(agent);
  const node = el(doc, `div`, `agent-card agent-${state.key}`);
  node.dataset.agent = agent.name;
  if (active) {
    node.dataset.active = "yes";
  }

  node.append(
    textNode(doc, "div", "skills-name", agent.name),
    textNode(
      doc,
      "div",
      "skills-description",
      agent.description ?? "(no description)",
    ),
  );

  if (!isDefault && !agent.broken) {
    node.append(textNode(doc, "div", "skills-presence", agentSummary(agent)));
  }

  if (state.note) {
    node.append(textNode(doc, "div", "skills-declared", state.note));
  }

  // What trusting would grant, in the same words the CLI prompts with, so a
  // person sees one sentence rather than two descriptions of one thing.
  if (state.key === "untrusted") {
    const list = el(doc, "ul", "agent-privileges");
    for (const line of agent.privilege_summary) {
      list.append(textNode(doc, "li", "skills-description", line));
    }
    node.append(list);
  }

  const actions = el(doc, "div", "settings-field-row");

  if (!agent.broken) {
    const pick = doc.createElement("button");
    pick.type = "button";
    pick.className = active ? "btn btn-allow" : "btn btn-deny";
    pick.textContent = active ? "Running under this" : "Use this agent";
    // Locked, or already the one in use. Either way there is nothing this
    // click could do, and a button that does nothing is worse than none.
    pick.disabled = locked || active;
    pick.addEventListener("click", () => handlers.onPick(isDefault ? null : agent.name));
    actions.append(pick);
  }

  // The only control that turns a workspace agent's command-bearing fields
  // on, and it is a person's click. Never offered at user scope, where the
  // person already put the file there.
  if (state.key === "untrusted" && agent.scope === "workspace") {
    const trust = doc.createElement("button");
    trust.type = "button";
    trust.className = "btn btn-deny";
    trust.textContent = "Trust this agent";
    trust.addEventListener("click", () => handlers.onTrust(agent));
    actions.append(trust);
  }

  if (actions.childNodes.length) {
    node.append(actions);
  }
  return node;
}
