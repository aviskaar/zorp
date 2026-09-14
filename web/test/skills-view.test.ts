/**
 * The skills pill and the list behind it.
 *
 * These exist because of where the strings come from. A skill's name,
 * description and path are read out of a `SKILL.md` on disk, which is a
 * file zorp did not write and which the person may have installed from
 * anywhere. The description is the one field a skill author controls that a
 * reader is shown, so it is the field an injection would arrive in, and it
 * gets the same treatment as model output: it lands as text or it does not
 * land.
 *
 * The second thing pinned here is that this is a report and not a switch.
 * There is no control that loads a skill, and the body never reaches the
 * page at all.
 */

import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";

import {
  SCOPE_LABELS,
  renderSkillsIndicator,
  renderSkillsPanel,
  skillsView,
  type SkillsView,
} from "../src/skills-view.ts";
import type { ActiveSkill, ActiveSkillListing, SkillListing, SkillSummary } from "../src/api.ts";

const MARKUP = `
<!doctype html><body>
  <div class="skills-indicator" id="skills-indicator" hidden>
    <button id="skills-btn" type="button" aria-expanded="false">
      <span>Skills</span><span id="skills-count"></span>
    </button>
    <div id="skills-panel" hidden></div>
  </div>
</body>`;

function fixture(): { doc: Document; view: SkillsView } {
  const dom = new JSDOM(MARKUP);
  const doc = dom.window.document as unknown as Document;
  return { doc, view: skillsView(doc) };
}

function skill(over: Partial<SkillSummary> = {}): SkillSummary {
  return {
    name: "tidy-notes",
    description: "Turn rough notes into a short ordered summary.",
    path: "/home/a/.claude/skills/tidy-notes/SKILL.md",
    scope: "user",
    declared_tools: [],
    ...over,
  };
}

function listing(over: Partial<SkillListing> = {}): SkillListing {
  return { skills: [skill()], warnings: [], ...over };
}

/* ------------------------------------------------------------------ */
/* the pill                                                            */
/* ------------------------------------------------------------------ */

test("the pill carries the count and says what a skill can and cannot do", () => {
  const { view } = fixture();
  renderSkillsIndicator(view, { available: true, count: 3 });

  assert.equal(view.root.hidden, false);
  assert.equal(view.count.textContent, "3");
  assert.match(view.button.title, /3 skills installed/);
  assert.match(view.button.title, /never permissions/);
});

test("one skill is not one skills", () => {
  const { view } = fixture();
  renderSkillsIndicator(view, { available: true, count: 1 });

  assert.match(view.button.title, /1 skill installed/);
});

/** An answer that has not arrived, an older server that reports nothing,
 * and no skills at all are the same thing on the page: no pill. One that
 * appeared while the answer was in flight would be a guess. */
test("nothing reported and nothing installed both draw no pill", () => {
  const { view } = fixture();

  renderSkillsIndicator(view, null);
  assert.equal(view.root.hidden, true);

  renderSkillsIndicator(view, undefined);
  assert.equal(view.root.hidden, true);

  renderSkillsIndicator(view, { available: false, count: 0 });
  assert.equal(view.root.hidden, true);

  // And a count of zero that somehow claims to be available is still
  // nothing to show.
  renderSkillsIndicator(view, { available: true, count: 0 });
  assert.equal(view.root.hidden, true);
});

test("hiding the pill closes the panel with it", () => {
  const { view } = fixture();
  view.panel.hidden = false;
  view.button.setAttribute("aria-expanded", "true");

  renderSkillsIndicator(view, null);

  assert.equal(view.panel.hidden, true);
  assert.equal(view.button.getAttribute("aria-expanded"), "false");
});

/* ------------------------------------------------------------------ */
/* a SKILL.md is a file zorp did not write                             */
/* ------------------------------------------------------------------ */

test("a description that looks like markup lands as text", () => {
  const { doc, view } = fixture();
  renderSkillsPanel(
    doc,
    view.panel,
    listing({ skills: [skill({ description: "<img src=x onerror=alert(1)>" })] }),
  );

  assert.equal(view.panel.querySelectorAll("img").length, 0);
  assert.equal(
    view.panel.querySelector(".skills-description")!.textContent,
    "<img src=x onerror=alert(1)>",
  );
});

test("a name that looks like markup lands as text", () => {
  const { doc, view } = fixture();
  renderSkillsPanel(
    doc,
    view.panel,
    listing({ skills: [skill({ name: "<script>alert(1)</script>" })] }),
  );

  assert.equal(view.panel.querySelectorAll("script").length, 0);
  assert.equal(
    view.panel.querySelector(".skills-name")!.textContent,
    "<script>alert(1)</script>",
  );
});

test("a warning that looks like markup lands as text", () => {
  const { doc, view } = fixture();
  renderSkillsPanel(
    doc,
    view.panel,
    listing({ skills: [], warnings: ["<img src=x onerror=alert(1)> could not be parsed"] }),
  );

  assert.equal(view.panel.querySelectorAll("img").length, 0);
  assert.match(view.panel.querySelector(".skills-warning")!.textContent!, /could not be parsed/);
});

/* ------------------------------------------------------------------ */
/* what the list says                                                  */
/* ------------------------------------------------------------------ */

test("skills are grouped by where they came from, in precedence order", () => {
  const { doc, view } = fixture();
  renderSkillsPanel(
    doc,
    view.panel,
    listing({
      skills: [
        skill({ name: "from-env", scope: "env" }),
        skill({ name: "mine", scope: "user" }),
        skill({ name: "repo", scope: "workspace" }),
      ],
    }),
  );

  const scopes = Array.from(view.panel.querySelectorAll(".skills-scope")).map(
    (n) => n.textContent,
  );
  assert.deepEqual(scopes, [
    SCOPE_LABELS.user,
    SCOPE_LABELS.workspace,
    SCOPE_LABELS.env,
  ]);
});

/** A skill asking for tools and not getting them is a thing a reader should
 * be able to see rather than discover. */
test("a skill that asks for tools says it does not get them", () => {
  const { doc, view } = fixture();
  renderSkillsPanel(
    doc,
    view.panel,
    listing({ skills: [skill({ declared_tools: ["Read", "Bash"] })] }),
  );

  const declared = view.panel.querySelector(".skills-declared")!.textContent!;
  assert.match(declared, /Read, Bash/);
  assert.match(declared, /does not grant/);
});

test("a skill that asks for nothing says nothing", () => {
  const { doc, view } = fixture();
  renderSkillsPanel(doc, view.panel, listing());

  assert.equal(view.panel.querySelector(".skills-declared"), null);
});

test("an empty listing says where to put one", () => {
  const { doc, view } = fixture();
  renderSkillsPanel(doc, view.panel, listing({ skills: [] }));

  const empty = view.panel.querySelector(".skills-empty")!.textContent!;
  assert.match(empty, /\.claude\/skills/);
  assert.match(empty, /ZORP_SKILLS_DIR/);
});

/** The panel is a report. Nothing in it loads anything, and the note above
 * the list says what a skill is and is not. */
test("the panel offers no way to load a skill and says a skill grants nothing", () => {
  const { doc, view } = fixture();
  renderSkillsPanel(doc, view.panel, listing());

  assert.equal(view.panel.querySelectorAll("button").length, 0);
  assert.equal(view.panel.querySelectorAll("a").length, 0);
  assert.match(view.panel.querySelector(".skills-note")!.textContent!, /grants no tool/);
});

test("redrawing the panel replaces it rather than appending to it", () => {
  const { doc, view } = fixture();
  renderSkillsPanel(doc, view.panel, listing());
  renderSkillsPanel(doc, view.panel, listing());

  assert.equal(view.panel.querySelectorAll(".skills-item").length, 1);
});

/* ------------------------------------------------------------------ */
/* what is in the conversation, as opposed to what is installed        */
/* ------------------------------------------------------------------ */

function active(over: Partial<ActiveSkill> = {}): ActiveSkill {
  return {
    name: "landing-page",
    scope: "workspace",
    presence: "present",
    active: true,
    seq: 4,
    loads: 1,
    bytes_in_window: 1832,
    ...over,
  };
}

function live(over: Partial<ActiveSkillListing> = {}): ActiveSkillListing {
  const skills = over.skills ?? [active()];
  return {
    skills,
    loaded: skills.length,
    active: skills.filter((s) => s.active).length,
    ...over,
  };
}

/**
 * The distinction the whole feature is for. A panel that could not tell
 * these apart would reproduce the bug it exists to fix, since the activity
 * line already shows that both were loaded.
 */
test("a skill still in the window and one compaction took do not read the same", () => {
  const { doc, view } = fixture();
  renderSkillsPanel(
    doc,
    view.panel,
    listing(),
    live({
      skills: [
        active({ name: "here", presence: "present", active: true }),
        active({ name: "gone", presence: "elided", active: false, bytes_in_window: 0 }),
      ],
    }),
  );

  const items = [...view.panel.querySelectorAll(".skills-item")];
  const here = items.find((i) => i.textContent?.includes("here"))!;
  const gone = items.find((i) => i.textContent?.includes("gone"))!;

  assert.ok(here.classList.contains("skills-present"));
  assert.ok(gone.classList.contains("skills-elided"));
  assert.match(here.textContent!, /in the context now/);
  assert.match(gone.textContent!, /no longer read it/);
});

test("the count says how many of the loaded ones are still live", () => {
  const { doc, view } = fixture();
  renderSkillsPanel(
    doc,
    view.panel,
    listing(),
    live({
      skills: [
        active({ name: "a", presence: "present", active: true }),
        active({ name: "b", presence: "dropped", active: false }),
        active({ name: "c", presence: "dropped", active: false }),
      ],
    }),
  );

  assert.match(view.panel.textContent!, /1 of 3 still in the context/);
});

test("a conversation that loaded nothing says so rather than showing an empty list", () => {
  const { doc, view } = fixture();
  renderSkillsPanel(doc, view.panel, listing(), live({ skills: [], loaded: 0, active: 0 }));

  assert.match(view.panel.textContent!, /No skill has been loaded/);
});

/**
 * The interesting row. Instructions from a file that is no longer on disk
 * are exactly what a person needs to be able to see, so the row is kept and
 * labelled rather than dropped for want of a scope.
 */
test("a skill that has since been uninstalled keeps its row and says so", () => {
  const { doc, view } = fixture();
  renderSkillsPanel(doc, view.panel, listing(), live({ skills: [active({ scope: null })] }));

  assert.match(view.panel.textContent!, /no longer installed/);
});

test("what a present body is costing is shown, and a gone one has no cost to show", () => {
  const { doc, view } = fixture();
  renderSkillsPanel(
    doc,
    view.panel,
    listing(),
    live({
      skills: [
        active({ name: "here", bytes_in_window: 1832 }),
        active({ name: "gone", presence: "elided", active: false, bytes_in_window: 0 }),
      ],
    }),
  );

  const items = [...view.panel.querySelectorAll(".skills-item")];
  assert.match(items.find((i) => i.textContent?.includes("here"))!.textContent!, /1,832 bytes/);
  assert.doesNotMatch(items.find((i) => i.textContent?.includes("gone"))!.textContent!, /bytes/);
});

test("a repeated load is visible rather than silently collapsed", () => {
  const { doc, view } = fixture();
  renderSkillsPanel(doc, view.panel, listing(), live({ skills: [active({ loads: 3 })] }));

  assert.match(view.panel.textContent!, /loaded 3 times/);
});

/**
 * A name comes out of a header zorp wrote, but it still lands as text. The
 * rest of this file exists for the same reason and this section is not an
 * exception to it.
 */
test("nothing in the active section becomes markup", () => {
  const { doc, view } = fixture();
  renderSkillsPanel(
    doc,
    view.panel,
    listing(),
    live({ skills: [active({ name: "<img src=x onerror=alert(1)>" })] }),
  );

  assert.equal(view.panel.querySelectorAll("img").length, 0);
  assert.ok(view.panel.textContent!.includes("<img src=x onerror=alert(1)>"));
});

/** Still a report. The new section adds no control either. */
test("the active section offers no way to load or unload a skill", () => {
  const { doc, view } = fixture();
  renderSkillsPanel(doc, view.panel, listing(), live());

  assert.equal(view.panel.querySelectorAll("button").length, 0);
  assert.equal(view.panel.querySelectorAll("input").length, 0);
  assert.equal(view.panel.querySelectorAll("a").length, 0);
});

/**
 * An older server has no such route and a page with no session has no
 * conversation to ask about. Both have to leave the installed listing
 * standing, because half a panel beats none.
 */
test("with no active listing the panel is exactly what it was before", () => {
  const { doc, view } = fixture();
  renderSkillsPanel(doc, view.panel, listing(), null);

  assert.doesNotMatch(view.panel.textContent!, /in this conversation/);
  assert.equal(view.panel.querySelectorAll(".skills-item").length, 1);
  assert.match(view.panel.textContent!, /grants no tool/);
});
