/**
 * Grouping the sidebar by project.
 *
 * Two things are being checked here. That the grouping is right, which is
 * a question about where a row ends up and about what a sidebar with no
 * projects looks like, and that a project name lands as text.
 *
 * The second one is the reason this module exists at all. A project name
 * is typed by a person, into a box, in a sidebar full of titles a model
 * wrote. It goes on the page beside them and it goes on the page the same
 * way: through `textContent`, never through an assembled string.
 */

import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";

import { UNFILED_LABEL, projectGroups, resetCollapsed } from "../src/project-group.ts";
import type { ProjectSummary, SessionSummary } from "../src/api.ts";

function fixture(): Document {
  const dom = new JSDOM("<!doctype html><body><div id=list></div></body>");
  return dom.window.document as unknown as Document;
}

function project(id: string, name: string): ProjectSummary {
  return { id, name, created: 1 };
}

function session(id: string, projectId: string | null = null): SessionSummary {
  return {
    id,
    title: `Session ${id}`,
    updated_at: "2026-08-22T10:00:00Z",
    updated: 1_700_000_000_000,
    project_id: projectId,
  };
}

/** The simplest possible row: an `li` carrying the session id. */
function rowFactory(doc: Document) {
  return (s: SessionSummary) => {
    const li = doc.createElement("li");
    li.className = "session-item";
    li.dataset.id = s.id;
    return li;
  };
}

function render(
  doc: Document,
  projects: ProjectSummary[],
  sessions: SessionSummary[],
  onDeleteProject?: (project: ProjectSummary) => void,
): HTMLElement {
  resetCollapsed();
  const host = doc.body.querySelector("#list") as HTMLElement;
  host.replaceChildren(
    ...projectGroups(doc, projects, sessions, rowFactory(doc), { onDeleteProject }),
  );
  return host;
}

function idsUnder(node: ParentNode): string[] {
  return Array.from(node.querySelectorAll(".session-item")).map(
    (n) => (n as HTMLElement).dataset.id ?? "",
  );
}

/* ------------------------------------------------------------------ */
/* grouping                                                            */
/* ------------------------------------------------------------------ */

test("each project gets a group holding its own conversations", () => {
  const doc = fixture();
  const host = render(
    doc,
    [project("p1", "Kitchen"), project("p2", "Roof")],
    [session("a", "p1"), session("b", "p2"), session("c", "p1")],
  );

  const groups = Array.from(host.querySelectorAll(".project-group"));
  assert.equal(groups.length, 2);
  assert.deepEqual(idsUnder(groups[0]), ["a", "c"]);
  assert.deepEqual(idsUnder(groups[1]), ["b"]);
  assert.equal(groups[0].querySelector(".project-name")!.textContent, "Kitchen");
  assert.equal(groups[0].querySelector(".project-count")!.textContent, "2");
});

test("conversations in no project are drawn after the groups, under a heading", () => {
  const doc = fixture();
  const host = render(doc, [project("p1", "Kitchen")], [session("a", "p1"), session("b")]);

  const heading = host.querySelector(".project-unfiled-label");
  assert.equal(heading!.textContent, UNFILED_LABEL);
  // After the group, not before it.
  assert.ok(
    heading!.compareDocumentPosition(host.querySelector(".project-group")!) &
      doc.defaultView!.Node.DOCUMENT_POSITION_PRECEDING,
  );
  assert.deepEqual(idsUnder(host.querySelector(".project-unfiled-label")!.nextSibling!), ["b"]);
});

/** The sidebar of somebody who has never made a project reads exactly as
 * it did before projects existed: rows, and nothing above them. */
test("with no projects there are no headings and no groups", () => {
  const doc = fixture();
  const host = render(doc, [], [session("a"), session("b")]);

  assert.equal(host.querySelectorAll(".project-group").length, 0);
  assert.equal(host.querySelector(".project-unfiled-label"), null);
  assert.deepEqual(idsUnder(host), ["a", "b"]);
});

/** A project somebody just made is empty, and an empty sidebar is how
 * they would conclude it did not work. */
test("a project with nothing in it is still drawn", () => {
  const doc = fixture();
  const host = render(doc, [project("p1", "Kitchen")], []);

  const group = host.querySelector(".project-group")!;
  assert.equal(group.querySelector(".project-count")!.textContent, "0");
  assert.deepEqual(idsUnder(group), []);
});

/** A label pointing at a project the server no longer lists must not take
 * the conversation off the page with it. */
test("a conversation filed under a project that is gone is still drawn", () => {
  const doc = fixture();
  const host = render(doc, [project("p1", "Kitchen")], [session("a", "vanished")]);

  assert.deepEqual(idsUnder(host), ["a"]);
});

/* ------------------------------------------------------------------ */
/* the delete control                                                  */
/* ------------------------------------------------------------------ */

test("deleting a project names it, and says the conversations are kept", () => {
  const doc = fixture();
  const deleted: string[] = [];
  const host = render(doc, [project("p1", "Kitchen")], [session("a", "p1")], (p) =>
    deleted.push(p.id),
  );

  const control = host.querySelector(".project-delete") as HTMLButtonElement;
  assert.match(control.title, /Kitchen/);
  assert.match(control.title, /kept/);
  control.click();
  assert.deepEqual(deleted, ["p1"]);
});

/** The control sits inside a `summary`, whose click toggles the group.
 * Deleting a project is not folding it away. */
test("the delete control does not fold the group", () => {
  const doc = fixture();
  const host = render(doc, [project("p1", "Kitchen")], [session("a", "p1")], () => {});

  const group = host.querySelector(".project-group") as HTMLDetailsElement;
  assert.equal(group.open, true);
  (host.querySelector(".project-delete") as HTMLButtonElement).click();
  assert.equal(group.open, true);
});

test("a group with no delete handler draws no delete control", () => {
  const doc = fixture();
  const host = render(doc, [project("p1", "Kitchen")], [session("a", "p1")]);

  assert.equal(host.querySelector(".project-delete"), null);
});

/* ------------------------------------------------------------------ */
/* a project name is typed by a person                                 */
/* ------------------------------------------------------------------ */

test("a project name that looks like markup lands as text in the heading", () => {
  const doc = fixture();
  const host = render(doc, [project("p1", "<img src=x onerror=alert(1)>")], []);

  assert.equal(host.querySelectorAll("img").length, 0);
  assert.equal(
    host.querySelector(".project-name")!.textContent,
    "<img src=x onerror=alert(1)>",
  );
});

test("a project name that looks like markup lands as text in the delete title", () => {
  const doc = fixture();
  const host = render(doc, [project("p1", "<script>alert(1)</script>")], [], () => {});

  assert.equal(host.querySelectorAll("script").length, 0);
  assert.match(
    (host.querySelector(".project-delete") as HTMLButtonElement).title,
    /<script>alert\(1\)<\/script>/,
  );
});
