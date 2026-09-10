/**
 * The sidebar, grouped by project.
 *
 * A project is a name a person typed and a set of conversations filed
 * under it. This turns a flat list of sessions into one collapsible group
 * per project, with everything nobody filed drawn after them. A sidebar
 * with no projects is drawn exactly as it was before projects existed: no
 * headings, no groups, just the rows.
 *
 * Its own module rather than a block inside `main.ts` for the reason every
 * other piece of this sidebar has one: `main.ts` runs the whole app on
 * import and cannot be loaded from a test, and this is a thing worth
 * testing. A project name is text a person typed into a box, it lands
 * beside titles a model wrote, and it goes on the page through
 * `textContent` like everything else in here. Nothing in this file
 * assembles markup and nothing in it may start to.
 *
 * `doc` is passed in rather than taken from the global, the same as
 * `session-row.ts` and for the same reason: it is what lets a test render
 * into a jsdom document and read back what actually landed.
 */

import type { ProjectSummary, SessionSummary } from "./api.ts";

/** The heading over the conversations nobody filed, when there are any. */
export const UNFILED_LABEL = "Not in a project";

/**
 * Which groups a person has folded shut, for the life of the page.
 *
 * Module scope and never persisted. A collapsed group is a thing somebody
 * did a second ago to see past it, not a preference worth carrying to the
 * next machine, and the sidebar is redrawn on every refresh so without
 * this every open group would spring back on each poll.
 */
const collapsed = new Set<string>();

/** Builds one row. `session-row.ts`'s `sessionRow`, bound to its options. */
export type RowFactory = (session: SessionSummary) => HTMLElement;

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

export interface ProjectGroupOptions {
  /**
   * Remove a project. Drawn on each group's summary with no confirmation
   * dialog, because deleting a project deletes no conversation: the
   * control's own `title` says so, and that is the thing a dialog would
   * have been there to ask about.
   */
  onDeleteProject?: (project: ProjectSummary) => void;
}

/**
 * The nodes for a whole session list, grouped.
 *
 * Order is the projects as the server listed them, oldest first, then the
 * unfiled conversations. Within a group the sessions keep the order they
 * arrived in, which is the server's own newest-first ordering.
 *
 * A project with nothing in it is still drawn, with a count of zero. It is
 * a thing somebody just made and an empty sidebar is how they would know
 * it did not work.
 */
export function projectGroups(
  doc: Document,
  projects: ProjectSummary[],
  sessions: SessionSummary[],
  row: RowFactory,
  options: ProjectGroupOptions = {},
): Node[] {
  const filed = new Map<string, SessionSummary[]>();
  for (const project of projects) {
    filed.set(project.id, []);
  }
  const unfiled: SessionSummary[] = [];
  for (const session of sessions) {
    const group = session.project_id ? filed.get(session.project_id) : undefined;
    // A conversation whose project the server no longer lists is not lost:
    // it is drawn with the unfiled ones rather than vanishing into a group
    // that is not on the page.
    if (group) {
      group.push(session);
    } else {
      unfiled.push(session);
    }
  }

  const nodes: Node[] = [];
  for (const project of projects) {
    nodes.push(group(doc, project, filed.get(project.id) ?? [], row, options));
  }

  // No projects at all means no headings at all: the sidebar reads exactly
  // as it did before this feature existed.
  if (projects.length && unfiled.length) {
    nodes.push(textNode(doc, "div", "sidebar-label project-unfiled-label", UNFILED_LABEL));
  }
  if (unfiled.length) {
    nodes.push(list(doc, unfiled, row));
  }
  return nodes;
}

function group(
  doc: Document,
  project: ProjectSummary,
  sessions: SessionSummary[],
  row: RowFactory,
  options: ProjectGroupOptions,
): HTMLElement {
  const details = doc.createElement("details");
  details.className = "project-group";
  details.dataset.projectId = project.id;
  details.open = !collapsed.has(project.id);
  details.addEventListener("toggle", () => {
    if (details.open) {
      collapsed.delete(project.id);
    } else {
      collapsed.add(project.id);
    }
  });

  const summary = doc.createElement("summary");
  summary.className = "project-summary";
  summary.append(
    textNode(doc, "span", "project-name", project.name),
    textNode(doc, "span", "project-count", String(sessions.length)),
  );

  if (options.onDeleteProject) {
    const remove = el(doc, "button", "icon-btn project-delete") as HTMLButtonElement;
    remove.type = "button";
    remove.textContent = "Delete project";
    // The reassurance goes here rather than into a dialog, because there
    // is nothing to confirm: unfiling is not deleting.
    remove.title = `Delete the project "${project.name}". The conversations in it are kept.`;
    remove.setAttribute("aria-label", remove.title);
    // A summary toggles the details on click. This control is inside one
    // and is not a toggle.
    remove.addEventListener("click", (event) => {
      event.preventDefault();
      event.stopPropagation();
      options.onDeleteProject?.(project);
    });
    summary.append(remove);
  }

  details.append(summary, list(doc, sessions, row));
  return details;
}

function list(doc: Document, sessions: SessionSummary[], row: RowFactory): HTMLElement {
  const ul = el(doc, "ul", "session-list");
  for (const session of sessions) {
    ul.append(row(session));
  }
  return ul;
}

/** Forget which groups were folded shut. For tests, which share a module. */
export function resetCollapsed(): void {
  collapsed.clear();
}
