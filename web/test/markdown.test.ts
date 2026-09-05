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
import { renderMarkdown } from "../src/markdown.ts";

const dom = new JSDOM("<!doctype html><body></body>");
// The renderer calls document.createElement, so it needs a document. Setting
// the globals is what lets the real source run unmodified rather than being
// refactored to take a document argument purely for the tests.
(globalThis as Record<string, unknown>).document = dom.window.document;
(globalThis as Record<string, unknown>).Node = dom.window.Node;

function render(source: string): HTMLElement {
  const host = dom.window.document.createElement("div");
  renderMarkdown(host as unknown as HTMLElement, source);
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

/**
 * A backslash escaped pipe is one cell's contents, not a cell boundary. This
 * matters now that the server turns Word and OpenDocument tables into pipe
 * tables: a cell whose text contains a pipe would otherwise silently add a
 * column and reshape the table it came from.
 */
test("an escaped pipe stays inside its cell rather than splitting it", () => {
  const host = render("| a\\|b | c |\n| --- | --- |\n| 1 | 2 |");
  assert.equal(host.querySelectorAll("thead th").length, 2);
  assert.equal(host.querySelectorAll("thead th")[0]?.textContent, "a|b");
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

/**
 * Emphasis reaches across a code span. The split on backticks used to hand
 * each side to the emphasis pass on its own, so an opening `**` and its
 * closing `**` never met when a code span sat between them and the page
 * showed the asterisks.
 */
test("bold containing a code span is bold with the code inside it", () => {
  const host = render("**874 `.md` files** on disk");
  const strong = host.querySelector("strong");
  assert.ok(strong, "the span should be bold");
  assert.equal(strong?.textContent, "874 .md files");
  assert.equal(strong?.querySelector("code.inline-code")?.textContent, ".md");
  assert.equal(host.querySelector("p")?.textContent, "874 .md files on disk");
});

test("bold starting with a code span is bold", () => {
  const host = render("**`git ls-files '*.md'` = 172** and that's what's tracked.");
  const strong = host.querySelector("strong");
  assert.ok(strong, "the span should be bold");
  assert.equal(strong?.textContent, "git ls-files '*.md' = 172");
  assert.equal(
    strong?.querySelector("code.inline-code")?.textContent,
    "git ls-files '*.md'",
    "the asterisk inside the code span is code, not a delimiter",
  );
  assert.equal(host.querySelectorAll("em").length, 0);
});

test("italic containing a code span is italic with the code inside it", () => {
  const host = render("see *the `run` step* first");
  const em = host.querySelector("em");
  assert.equal(em?.textContent, "the run step");
  assert.equal(em?.querySelector("code.inline-code")?.textContent, "run");
});

test("asterisks inside a code span are literal even next to real emphasis", () => {
  const host = render("**a** `*b*` **c**");
  assert.equal(host.querySelectorAll("strong").length, 2);
  assert.equal(host.querySelectorAll("em").length, 0);
  assert.equal(host.querySelector("code.inline-code")?.textContent, "*b*");
});

test("an unmatched ** followed by a code span stays literal", () => {
  const host = render("**open `code` and no close");
  assert.equal(host.querySelectorAll("strong").length, 0);
  assert.equal(host.querySelector("code.inline-code")?.textContent, "code");
  assert.equal(host.querySelector("p")?.textContent, "**open code and no close");
});

test("a link inside bold still renders as a link", () => {
  const host = render("**see [the paper](https://example.com/p) now**");
  const anchor = host.querySelector("strong a");
  assert.ok(anchor, "the link should be inside the bold span");
  assert.equal(anchor?.getAttribute("href"), "https://example.com/p");
  assert.equal(anchor?.textContent, "the paper");
  assert.equal(host.querySelector("strong")?.textContent, "see the paper now");
});

test("an underscore in a link's URL is not an emphasis delimiter", () => {
  const host = render("read [docs](https://example.com/my_page_name) today");
  assert.equal(host.querySelectorAll("em").length, 0);
  assert.equal(
    host.querySelector("a")?.getAttribute("href"),
    "https://example.com/my_page_name",
  );
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
