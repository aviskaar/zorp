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

import type { SkillAvailability, SkillListing, SkillSummary } from "./api.ts";

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
 * Fill the popover from a listing.
 *
 * Grouped by scope, in the order the scopes are searched, so a reader can
 * tell a skill that came with the repository they opened from one they
 * installed themselves. An empty listing says so; a listing that is only
 * warnings says that too, because a skill that failed to parse is the
 * case a person most needs to hear about.
 */
export function renderSkillsPanel(doc: Document, panel: HTMLElement, listing: SkillListing): void {
  panel.replaceChildren();

  panel.append(
    textNode(
      doc,
      "p",
      "skills-note",
      "Installed skills. The agent loads one when the task matches its description. " +
        "A skill adds guidance only: it grants no tool, widens no approval, and bypasses no denylist entry.",
    ),
  );

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
