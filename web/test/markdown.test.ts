/**
 * Tests for the markdown renderer.
 *
 * The first block is the one that matters. Everything rendered here is model
 * output, and the model has been reading tool results, web pages and files.
 * A renderer that turns any of that into markup is a cross-site scripting
 * hole, so the injection cases are the reason this file exists and the
 * formatting cases are the reason the renderer is worth having.
 */

import { strict as assert } from "node:assert";
import { test } from "node:test";
import { JSDOM } from "jsdom";
import { renderMarkdown, type MarkdownOptions } from "../src/markdown.ts";
import type { Finding, VerifiedFinding } from "../src/finding.ts";

const dom = new JSDOM("<!doctype html><body></body>");
// The renderer calls document.createElement, so it needs a document. Setting
// the globals is what lets the real source run unmodified rather than being
// refactored to take a document argument purely for the tests.
(globalThis as Record<string, unknown>).document = dom.window.document;
(globalThis as Record<string, unknown>).Node = dom.window.Node;

function render(source: string, options?: MarkdownOptions): HTMLElement {
  const host = dom.window.document.createElement("div");
  renderMarkdown(host as unknown as HTMLElement, source, options);
  return host as unknown as HTMLElement;
}

test("a script tag in the source becomes text, not a script", () => {
  const host = render("Look: <script>alert(1)</script> done");
  assert.equal(host.querySelectorAll("script").length, 0);
  assert.ok(
    host.textContent?.includes("<script>alert(1)</script>"),
    `the tag should still be visible as text, got: ${host.textContent}`,
  );
});

test("an img with an onerror handler never becomes an element", () => {
  const host = render('<img src=x onerror="alert(1)">');
  assert.equal(host.querySelectorAll("img").length, 0);
  assert.equal(host.querySelectorAll("*[onerror]").length, 0);
});

test("a javascript: link is not clickable", () => {
  const host = render("[click me](javascript:alert(1))");
  assert.equal(
    host.querySelectorAll("a").length,
    0,
    "a javascript: URL was turned into a link",
  );
  // Not silently dropped either. The user should see that something was
  // there and what it pointed at.
  assert.ok(host.textContent?.includes("click me"), host.textContent ?? "");
  assert.ok(host.textContent?.includes("javascript:"), host.textContent ?? "");
});

test("data: and vbscript: links are refused the same way", () => {
  for (const href of ["data:text/html,<script>alert(1)</script>", "vbscript:msgbox"]) {
    const host = render(`[x](${href})`);
    assert.equal(host.querySelectorAll("a").length, 0, `${href} became a link`);
  }
});

test("an http link is clickable and cannot reach back through window.opener", () => {
  const host = render("see [the paper](https://example.com/paper.pdf)");
  const anchor = host.querySelector("a");
  assert.ok(anchor, "an https link should be clickable");
  assert.equal(anchor?.getAttribute("href"), "https://example.com/paper.pdf");
  assert.equal(anchor?.getAttribute("rel"), "noopener noreferrer");
  assert.equal(anchor?.textContent, "the paper");
});

test("markdown images do not fetch anything and are not links either", () => {
  const host = render("![alt](https://tracker.example.com/beacon.png)");
  assert.equal(
    host.querySelectorAll("img").length,
    0,
    "an image tag would fetch an attacker-chosen URL on render",
  );
  // The link regex used to match the `[alt](url)` inside `![alt](url)` and
  // leave the `!` behind as text, which turned every image into something
  // clickable pointing at whatever URL the model chose.
  assert.equal(
    host.querySelectorAll("a").length,
    0,
    "an image became a clickable link to its source",
  );
  assert.ok(
    host.textContent?.includes("tracker.example.com"),
    `the URL should still be visible as text: ${host.textContent}`,
  );
});

test("headings become real heading elements", () => {
  const host = render("# One\n\n## Two\n\n###### Six");
  assert.equal(host.querySelector("h1")?.textContent, "One");
  assert.equal(host.querySelector("h2")?.textContent, "Two");
  assert.equal(host.querySelector("h6")?.textContent, "Six");
});

test("a seven hash line is not a heading", () => {
  const host = render("####### nope");
  assert.equal(host.querySelectorAll("h1,h2,h3,h4,h5,h6").length, 0);
});

test("unordered and ordered lists render as lists", () => {
  const host = render("- one\n- two\n\n1. first\n2. second");
  assert.equal(host.querySelectorAll("ul > li").length, 2);
  assert.equal(host.querySelectorAll("ol > li").length, 2);
  assert.equal(host.querySelector("ul > li")?.textContent, "one");
});

test("an indented list item nests inside the item above it", () => {
  const host = render("- outer\n  - inner\n- outer again");
  const nested = host.querySelector("ul > li > ul > li");
  assert.ok(nested, "the indented item should be a sublist");
  assert.equal(nested?.textContent, "inner");
});

test("a pipe table renders as a table with a header row", () => {
  const host = render("| a | b |\n| --- | --- |\n| 1 | 2 |\n| 3 | 4 |");
  assert.equal(host.querySelectorAll("thead th").length, 2);
  assert.equal(host.querySelectorAll("tbody tr").length, 2);
  assert.equal(host.querySelectorAll("tbody td")[0]?.textContent, "1");
});

test("a line with a pipe but no separator row stays a paragraph", () => {
  const host = render("this | that");
  assert.equal(host.querySelectorAll("table").length, 0);
  assert.equal(host.querySelector("p")?.textContent, "this | that");
});

test("bold and italic render as strong and em", () => {
  const host = render("**bold** and *italic* and __also bold__");
  assert.equal(host.querySelectorAll("strong").length, 2);
  assert.equal(host.querySelectorAll("em").length, 1);
  assert.equal(host.querySelector("strong")?.textContent, "bold");
});

test("markdown inside a fenced block is code, not markdown", () => {
  const host = render("```\n# not a heading\n**not bold**\n```");
  assert.equal(host.querySelectorAll("h1").length, 0);
  assert.equal(host.querySelectorAll("strong").length, 0);
  assert.equal(
    host.querySelector("pre.code-block code")?.textContent,
    "# not a heading\n**not bold**",
  );
});

test("a fence keeps its language tag for later highlighting", () => {
  const host = render("```rust\nfn main() {}\n```");
  assert.equal(
    (host.querySelector("pre.code-block") as HTMLElement | null)?.dataset.lang,
    "rust",
  );
});

test("backticks protect their contents from emphasis", () => {
  const host = render("use `**not bold**` here");
  assert.equal(host.querySelectorAll("strong").length, 0);
  assert.equal(host.querySelector("code.inline-code")?.textContent, "**not bold**");
});

test("blockquotes nest their content as markdown", () => {
  const host = render("> ## quoted heading\n> and text");
  assert.ok(host.querySelector("blockquote h2"), "a heading inside a quote");
});

test("empty input still produces a node rather than nothing", () => {
  const host = render("");
  assert.ok(host.childNodes.length > 0, "an empty answer should not vanish");
});

test("a horizontal rule renders", () => {
  const host = render("above\n\n---\n\nbelow");
  assert.equal(host.querySelectorAll("hr").length, 1);
});

/*
 * Finding markers.
 *
 * A `finding` fence is the only block that can put a badge on the page, and
 * the renderer never decides that on its own: it asks the caller, and a caller
 * with no way to check refuses by not being there at all. Everything below is
 * either "no badge" or "a badge whose contents are still text".
 */

const FINDING = [
  "```finding",
  "claim: the two series disagree for 2019",
  "because: the filed figure and the published one differ by 1.4 points",
  "source: docs/rates.md",
  "source: ons.gov.uk/inflation-2019",
  "```",
].join("\n");

function marker(finding: Finding): VerifiedFinding {
  return {
    claim: finding.claim,
    reason: finding.reason,
    evidence: [
      { name: "read_file", summary: "docs/rates.md (120 lines)" },
      { name: "web_search", summary: "ons.gov.uk/inflation-2019" },
    ],
  };
}

const marking: MarkdownOptions = { markFinding: marker };

test("an ordinary answer carries no marker at all", () => {
  const host = render("# Result\n\nNothing surprising here.\n\n- a\n- b", marking);
  assert.equal(host.querySelectorAll(".card-finding").length, 0);
});

// The default is refusal. Every surface that cannot see what the run actually
// did gets this one, which is the artifact pane and any replayed transcript.
test("without a way to check, a finding block shows its text and no badge", () => {
  const host = render(FINDING);
  assert.equal(host.querySelectorAll(".card-finding").length, 0, "a badge appeared unchecked");
  assert.ok(
    host.textContent?.includes("the two series disagree for 2019"),
    `the claim vanished: ${host.textContent}`,
  );
});

test("a caller that refuses the finding gets text and no badge", () => {
  const host = render(FINDING, { markFinding: () => null });
  assert.equal(host.querySelectorAll(".card-finding").length, 0);
  assert.ok(host.textContent?.includes("the filed figure"), "the reason vanished");
});

test("a verified finding renders exactly one marker", () => {
  const host = render(FINDING, marking);
  assert.equal(host.querySelectorAll(".card-finding").length, 1);
});

// Not colour alone and not a bare icon: the word is on the page and the
// region has a name a screen reader will read.
test("the marker says what it is in text, not only in colour", () => {
  const host = render(FINDING, marking);
  const card = host.querySelector(".card-finding") as HTMLElement;
  assert.ok(card.textContent?.includes("Finding"), `no visible label: ${card.textContent}`);
  assert.equal(card.getAttribute("role"), "note");
  assert.ok((card.getAttribute("aria-label") ?? "").length > 0, "the region has no name");
  for (const svg of card.querySelectorAll("svg")) {
    assert.equal(
      svg.getAttribute("aria-hidden"),
      "true",
      "a decorative icon is exposed to assistive technology",
    );
  }
});

// A bulb with nothing behind it is decoration.
test("the marker carries its reason and the evidence behind it", () => {
  const host = render(FINDING, marking);
  const card = host.querySelector(".card-finding") as HTMLElement;
  assert.ok(card.querySelector("details"), "the reason is not reachable by clicking");
  assert.ok(card.textContent?.includes("differ by 1.4 points"), "the reason is missing");
  assert.ok(card.textContent?.includes("docs/rates.md (120 lines)"), "the evidence is missing");
  assert.ok(card.textContent?.includes("ons.gov.uk/inflation-2019"), "the evidence is missing");
});

// The UI must not imply more confidence than the mechanism earns, so the card
// says in words what was and was not checked.
test("the marker says what it did not check", () => {
  const host = render(FINDING, marking);
  const text = (host.querySelector(".card-finding") as HTMLElement).textContent ?? "";
  assert.ok(/did not check|not check/i.test(text), `no limits stated: ${text}`);
});

test("the caller is handed the parsed finding, not the raw block", () => {
  let seen: Finding | null = null;
  render(FINDING, {
    markFinding: (finding) => {
      seen = finding;
      return null;
    },
  });
  assert.deepEqual(seen, {
    claim: "the two series disagree for 2019",
    reason: "the filed figure and the published one differ by 1.4 points",
    sources: ["docs/rates.md", "ons.gov.uk/inflation-2019"],
  });
});

test("a finding block that does not parse is never offered for marking", () => {
  let asked = 0;
  const host = render("```finding\njust vibes\n```", {
    markFinding: () => {
      asked += 1;
      return marker({ claim: "", reason: "", sources: [] });
    },
  });
  assert.equal(asked, 0, "a malformed block was offered a badge");
  assert.equal(host.querySelectorAll(".card-finding").length, 0);
  assert.ok(host.textContent?.includes("just vibes"), "the text was dropped");
});

/*
 * Injection through the marker.
 *
 * Everything in a finding block is model output, exactly like the prose
 * around it, and the card is a second path onto the page.
 */

test("a script tag in a claim is text, not a script", () => {
  const host = render(
    ["```finding", "claim: <script>alert(1)</script>", "because: r", "source: a", "```"].join("\n"),
    marking,
  );
  assert.equal(host.querySelectorAll("script").length, 0);
  assert.ok(
    host.textContent?.includes("<script>alert(1)</script>"),
    `the tag should be visible as text: ${host.textContent}`,
  );
});

test("an img with an onerror handler in a reason never becomes an element", () => {
  const host = render(
    ["```finding", "claim: c", 'because: <img src=x onerror="alert(1)">', "source: a", "```"].join(
      "\n",
    ),
    marking,
  );
  assert.equal(host.querySelectorAll("img").length, 0);
  assert.equal(host.querySelectorAll("*[onerror]").length, 0);
});

test("a javascript: link inside a marked claim is not clickable", () => {
  const host = render(
    ["```finding", "claim: [click me](javascript:alert(1))", "because: r", "source: a", "```"].join(
      "\n",
    ),
    marking,
  );
  assert.equal(host.querySelectorAll("a").length, 0, "a javascript: URL became a link");
  assert.ok(host.textContent?.includes("click me"), host.textContent ?? "");
});

// The evidence labels come from tool summaries, which are built from paths,
// URLs and command output the model has been reading.
test("markup in an evidence label is text, not markup", () => {
  const host = render(FINDING, {
    markFinding: (finding) => ({
      claim: finding.claim,
      reason: finding.reason,
      evidence: [
        { name: "<script>alert(1)</script>", summary: '<img src=x onerror="alert(2)">' },
        { name: "web_search", summary: "ons.gov.uk" },
      ],
    }),
  });
  assert.equal(host.querySelectorAll("script").length, 0);
  assert.equal(host.querySelectorAll("img").length, 0);
  assert.ok(host.textContent?.includes("<script>alert(1)</script>"), host.textContent ?? "");
});

test("a marked claim never renders an image beacon", () => {
  const host = render(
    [
      "```finding",
      "claim: ![alt](https://tracker.example.com/beacon.png)",
      "because: r",
      "source: a",
      "```",
    ].join("\n"),
    marking,
  );
  assert.equal(host.querySelectorAll("img").length, 0);
  assert.equal(host.querySelectorAll("a").length, 0);
});
