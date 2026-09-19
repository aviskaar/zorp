/**
 * What skills this server can see, and where they came from.
 *
 * A pill in the toolbar carrying a count, and a popover listing the
 * installed skills with their descriptions. Like `search-indicator.ts` this
 * is a report and not a switch: there is no control here that loads a
 * skill, installs one, or turns one off, and there should not be. Loading a
 * skill is the agent's `skill` tool, which the model calls when the task
 * matches, gated exactly as every other tool call is.
 *
 * **Everything here goes through `textContent`.** A skill's name,
 * description and path come out of a `SKILL.md` on disk, which is a file
 * zorp did not write and which the person may have installed from anywhere.
 * A description is the one field a skill author controls that is shown to a
 * reader, so it is the field an injection would arrive in. This module
 * builds no HTML strings and must never start to.
 *
 * `doc` is passed in rather than taken from the global, the same as
 * `memory-note` and for the same reason: it is what lets a test render into
 * a jsdom document and read back what actually landed.
 */

import type {
  ActiveSkill,
  ActiveSkillListing,
  SkillAvailability,
  SkillListing,
  SkillOffer,
  SkillSummary,
} from "./api.ts";

/** The word on the pill. The count goes beside it. */
export const SKILLS_LABEL = "Skills";

/** What a scope is called on the page. */
export const SCOPE_LABELS: Record<SkillSummary["scope"], string> = {
  user: "your skills",
  workspace: "this workspace",
  env: "ZORP_SKILLS_DIR",
  other: "elsewhere",
};

/** The elements the indicator and the popover write into. */
export interface SkillsView {
  root: HTMLElement;
  button: HTMLButtonElement;
  count: HTMLElement;
  panel: HTMLElement;
}

export function skillsView(doc: Document): SkillsView {
  const byId = (id: string): HTMLElement => {
    const node = doc.getElementById(id);
    if (!node) {
      throw new Error(`index.html is missing #${id}`);
    }
    return node;
  };
  return {
    root: byId("skills-indicator"),
    button: byId("skills-btn") as HTMLButtonElement,
    count: byId("skills-count"),
    panel: byId("skills-panel"),
  };
}

/**
 * Draw the pill from what the server reported.
 *
 * `null` means nothing has been reported yet, and an older server reports
 * nothing at all. Both are drawn the same way as no skills: as nothing. A
 * pill that appeared while the answer was in flight would be a guess, and
 * this exists to replace guessing.
 */
export function renderSkillsIndicator(
  view: SkillsView,
  capability: SkillAvailability | null | undefined,
): void {
  if (!capability?.available || capability.count <= 0) {
    view.root.hidden = true;
    view.count.textContent = "";
    view.panel.hidden = true;
    view.button.setAttribute("aria-expanded", "false");
    return;
  }
  view.root.hidden = false;
  view.count.textContent = String(capability.count);
  const plural = capability.count === 1 ? "skill" : "skills";
  const label = `${capability.count} ${plural} installed. Loading one adds guidance only, never permissions.`;
  view.button.title = label;
  view.button.setAttribute("aria-label", label);
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

/**
 * What each presence means, in a sentence a reader can act on.
 *
 * The words are the server's; these are the explanations. "Elided" is the
 * one that has to be unambiguous, because it is the state the activity line
 * cannot show and the reason this listing exists.
 */
export const PRESENCE_NOTES: Record<ActiveSkill["presence"], string> = {
  present: "in the context now",
  elided: "loaded, then dropped by compaction. The model can no longer read it",
  dropped: "no longer in the request. The exchange it belonged to was dropped",
  unrecorded: "its result was never recorded, so nothing was sent",
};

/**
 * The skills whose instructions are in this conversation, above the ones
 * that are merely installed.
 *
 * Both lists have to be here and they have to be labelled differently. A
 * panel that showed only what was loaded would reproduce the bug it exists
 * to fix, since "loaded" and "still in the window" stop being the same
 * thing the moment a conversation is long enough to compact.
 */
function renderActiveSection(
  doc: Document,
  panel: HTMLElement,
  listing: ActiveSkillListing,
): void {
  panel.append(textNode(doc, "div", "skills-section", "in this conversation"));

  if (!listing.loaded) {
    panel.append(
      textNode(
        doc,
        "p",
        "skills-empty",
        "No skill has been loaded in this conversation. The agent loads one when a task matches its description.",
      ),
    );
    return;
  }

  panel.append(
    textNode(
      doc,
      "p",
      "skills-note",
      `${listing.active} of ${listing.loaded} still in the context. ` +
        "Instructions arrive as a tool result, and compaction takes the oldest of those first.",
    ),
  );

  const list = el(doc, "ul", "skills-list");
  for (const skill of listing.skills) {
    const item = el(doc, "li", `skills-item skills-${skill.presence}`);
    item.append(
      textNode(doc, "span", "skills-name", skill.name),
      textNode(doc, "span", "skills-presence", PRESENCE_NOTES[skill.presence]),
    );
    // Skill bodies are among the largest things in a window, so what one is
    // costing is worth a line. Only when it is actually costing something.
    if (skill.active && skill.bytes_in_window > 0) {
      item.append(
        textNode(
          doc,
          "span",
          "skills-bytes",
          `${skill.bytes_in_window.toLocaleString()} bytes`,
        ),
      );
    }
    if (skill.loads > 1) {
      item.append(textNode(doc, "span", "skills-loads", `loaded ${skill.loads} times`));
    }
    if (!skill.scope) {
      // The interesting case. Its instructions may still be in the window
      // while the file they came from is gone.
      item.append(
        textNode(doc, "span", "skills-declared", "no longer installed"),
      );
    }
    list.append(item);
  }
  panel.append(list);
}

/**
 * The rule that decided which skills the model is offered, printed beside
 * the list it applies to.
 *
 * Only when there is something to explain: a skill withheld, or a person's
 * override in force. A model offered everything by the rule has nothing
 * missing and needs no sentence about it. The same precedent as onboarding,
 * which prints its rule next to the model it picked, and for the same
 * reason: a skill that is installed and absent from a conversation with no
 * explanation looks like a bug.
 */
function renderOffer(doc: Document, panel: HTMLElement, offer: SkillOffer): void {
  if (!offer.withheld.length && offer.setting === "auto") {
    return;
  }
  const who = offer.model ?? "this model";
  const lead = offer.withheld.length
    ? `Not offered to ${who}: ${offer.withheld.join(", ")}. `
    : "";
  panel.append(textNode(doc, "p", "skills-note skills-offer", lead + offer.reason));
}

/**
 * Fill the popover from a listing.
 *
 * Grouped by scope, in the order the scopes are searched, so a reader can
 * tell a skill that came with the repository they opened from one they
 * installed themselves. An empty listing says so; a listing that is only
 * warnings says that too, because a skill that failed to parse is the
 * case a person most needs to hear about.
 */
export function renderSkillsPanel(
  doc: Document,
  panel: HTMLElement,
  listing: SkillListing,
  active?: ActiveSkillListing | null,
): void {
  panel.replaceChildren();

  // What is in the conversation goes first, because it is the question a
  // person opening this has. What is on disk is the reference underneath it.
  if (active) {
    renderActiveSection(doc, panel, active);
  }

  panel.append(textNode(doc, "div", "skills-section", "installed"));
  panel.append(
    textNode(
      doc,
      "p",
      "skills-note",
      "The agent loads one when the task matches its description. " +
        "A skill adds guidance only: it grants no tool, widens no approval, and bypasses no denylist entry.",
    ),
  );

  // The conversation's own answer when there is one, since an agent can
  // name its own model; the configured model's otherwise.
  const offer = active?.offer ?? listing.offer ?? null;
  if (offer) {
    renderOffer(doc, panel, offer);
  }

  if (!listing.skills.length) {
    panel.append(
      textNode(
        doc,
        "p",
        "skills-empty",
        "No skills found. Put a directory holding a SKILL.md under ~/.claude/skills, " +
          "under .claude/skills in this workspace, or wherever ZORP_SKILLS_DIR points.",
      ),
    );
  }

  // In the order the scopes are searched, so the grouping reads the way
  // precedence works rather than alphabetically.
  const order: SkillSummary["scope"][] = ["user", "workspace", "env", "other"];
  for (const scope of order) {
    const inScope = listing.skills.filter((skill) => skill.scope === scope);
    if (!inScope.length) {
      continue;
    }
    panel.append(textNode(doc, "div", "skills-scope", SCOPE_LABELS[scope]));
    const list = el(doc, "ul", "skills-list");
    for (const skill of inScope) {
      const item = el(doc, "li", "skills-item");
      item.append(
        textNode(doc, "span", "skills-name", skill.name),
        textNode(doc, "span", "skills-description", skill.description),
      );
      // The path is what a person needs to go and read or edit the thing.
      // A title rather than a line, because it is long and it is not what
      // the list is for.
      item.title = skill.path;
      if (offer?.withheld.includes(skill.name)) {
        // Still listed, because routing hides nothing from the person. It
        // is the model's index this is missing from, and the note above
        // says why.
        item.classList.add("skills-withheld");
        item.append(
          textNode(doc, "span", "skills-declared", "not offered to this model"),
        );
      }
      if (skill.declared_tools.length) {
        // Said plainly, because a skill asking for tools and not getting
        // them is a thing a reader should be able to see rather than
        // discover.
        item.append(
          textNode(
            doc,
            "span",
            "skills-declared",
            `asks for ${skill.declared_tools.join(", ")}, which zorp does not grant`,
          ),
        );
      }
      list.append(item);
    }
    panel.append(list);
  }

  for (const warning of listing.warnings) {
    panel.append(textNode(doc, "p", "skills-warning", warning));
  }
}
