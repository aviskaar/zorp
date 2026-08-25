/**
 * A small markdown renderer that builds DOM nodes and never assembles an
 * HTML string.
 *
 * That constraint is the whole reason this file exists instead of a
 * dependency. Every markdown library worth using returns HTML and hands you
 * the `innerHTML` call, and the text being rendered here is model output:
 * the model has been reading tool results, web pages and files, so treating
 * it as trusted markup is a cross-site scripting hole with extra steps. Text
 * only ever reaches the page through `textContent`, exactly as the old
 * code-block-only renderer did.
 *
 * The supported subset is in
 * `docs/superpowers/specs/2026-08-17-artifact-pane-design.md`. Raw HTML is
 * deliberately not supported and renders as visible text, which is both the
 * safe direction and the honest one.
 */

// Spelled with its extension because the tests load this module in node,
// which resolves ESM specifiers literally and will not guess at ".ts".
import { parseFinding, type Finding, type VerifiedFinding } from "./finding.ts";

/** Schemes a link is allowed to have. Anything else renders as plain text. */
const SAFE_SCHEMES = ["http://", "https://", "mailto:"];

export interface MarkdownOptions {
  /**
   * Decides whether a `finding` block has earned its marker, and returns the
   * evidence to show behind it.
   *
   * Absent means nothing can be marked. That is the default because most
   * callers here cannot see what the run actually did: the artifact pane is
   * rendering a file, and a replayed transcript has lost the tool activity
   * that a finding would have to cite. A renderer that badged those would be
   * asserting something it has no way to check.
   */
  markFinding?: (finding: Finding) => VerifiedFinding | null;
}

export function renderMarkdown(
  target: HTMLElement,
  source: string,
  options: MarkdownOptions = {},
): void {
  const lines = (source ?? "").replace(/\r\n?/g, "\n").split("\n");
  let index = 0;

  while (index < lines.length) {
    const line = lines[index];

    if (!line.trim()) {
      index += 1;
      continue;
    }

    // Fenced code first, so nothing inside a fence is ever parsed as
    // markdown. A fence containing `# heading` is code, not a heading.
    const fence = line.match(/^\s*```(.*)$/);
    if (fence) {
      const lang = fence[1].trim();
      const body: string[] = [];
      index += 1;
      while (index < lines.length && !/^\s*```/.test(lines[index])) {
        body.push(lines[index]);
        index += 1;
      }
      index += 1; // the closing fence, or the end of the input
      // A finding rides inside a fence for one reason: nothing inside a fence
      // is ever parsed as markdown, so the block is inert before anyone looks
      // at it, and a surface that has never heard of findings shows it as a
      // code block rather than swallowing it.
      target.append(
        lang === FINDING_LANG
          ? findingBlock(body.join("\n"), options)
          : codeBlock(body.join("\n"), lang),
      );
      continue;
    }

    const heading = line.match(/^(#{1,6})\s+(.*)$/);
    if (heading) {
      const node = document.createElement(`h${heading[1].length}`);
      node.className = "md-heading";
      renderInline(node, heading[2].trim());
      target.append(node);
      index += 1;
      continue;
    }

    if (/^\s*(---+|\*\*\*+|___+)\s*$/.test(line)) {
      target.append(document.createElement("hr"));
      index += 1;
      continue;
    }

    if (/^\s*>\s?/.test(line)) {
      const body: string[] = [];
      while (index < lines.length && /^\s*>\s?/.test(lines[index])) {
        body.push(lines[index].replace(/^\s*>\s?/, ""));
        index += 1;
      }
      const quote = document.createElement("blockquote");
      quote.className = "md-quote";
      renderMarkdown(quote, body.join("\n"), options);
      target.append(quote);
      continue;
    }

    // A table needs its separator row to be a table at all, otherwise a line
    // that merely contains a pipe would start one.
    if (line.includes("|") && isTableSeparator(lines[index + 1] ?? "")) {
      const rows: string[] = [];
      while (index < lines.length && lines[index].includes("|")) {
        rows.push(lines[index]);
        index += 1;
      }
      target.append(table(rows));
      continue;
    }

    if (isListItem(line)) {
      const consumed = renderList(target, lines, index);
      index = consumed;
      continue;
    }

    // Everything else is a paragraph, running until a blank line or the
    // start of some other block.
    const body: string[] = [];
    while (
      index < lines.length &&
      lines[index].trim() &&
      !isListItem(lines[index]) &&
      !/^\s*(#{1,6}\s|>|```)/.test(lines[index]) &&
      !/^\s*(---+|\*\*\*+|___+)\s*$/.test(lines[index])
    ) {
      body.push(lines[index]);
      index += 1;
    }
    const para = document.createElement("p");
    para.className = "para";
    renderInline(para, body.join(" "));
    target.append(para);
  }

  if (!target.childNodes.length) {
    const para = document.createElement("p");
    para.className = "para";
    target.append(para);
  }
}

/** The fence language that asks for a marker. */
const FINDING_LANG = "finding";

/**
 * Render a `finding` block in one of its three shapes.
 *
 * Only the first shape gets a badge, and it needs a caller willing to vouch
 * for it. The other two exist so that refusing to mark something never loses
 * what the model wrote: an unverified finding is still prose worth reading,
 * it just does not get to look like a verdict.
 */
function findingBlock(body: string, options: MarkdownOptions): HTMLElement {
  const parsed = parseFinding(body);
  if (!parsed) {
    // Malformed. Show it verbatim rather than guessing at what was meant.
    return codeBlock(body, FINDING_LANG);
  }
  const verified = options.markFinding?.(parsed) ?? null;
  return verified ? markedFinding(verified) : plainFinding(parsed);
}

/** An unmarked finding: the model's words, with none of zorp's authority. */
function plainFinding(finding: Finding): HTMLElement {
  const wrap = document.createElement("div");
  wrap.className = "finding-plain";
  for (const text of [finding.claim, finding.reason]) {
    const para = document.createElement("p");
    para.className = "para";
    renderInline(para, text);
    wrap.append(para);
  }
  if (finding.sources.length) {
    const list = document.createElement("ul");
    list.className = "md-list";
    for (const source of finding.sources) {
      const item = document.createElement("li");
      // Verbatim. These are strings that were supposed to match something the
      // run touched, and formatting them would hide why they did not.
      item.textContent = source;
      list.append(item);
    }
    wrap.append(list);
  }
  return wrap;
}

/**
 * A marked finding.
 *
 * Every claim this card makes has to be one the mechanism can back. It says
 * the sources were used in this run, because that was checked. It says
 * nothing was checked about whether the claim is true, because nothing was.
 */
function markedFinding(finding: VerifiedFinding): HTMLElement {
  const card = document.createElement("section");
  card.className = "card card-finding";
  // A named region rather than a coloured box. The icon is decorative and the
  // word "Finding" is on the page, so nothing here depends on seeing colour.
  card.setAttribute("role", "note");
  card.setAttribute(
    "aria-label",
    `Finding, corroborated by ${finding.evidence.length} sources this run used`,
  );

  const head = document.createElement("div");
  head.className = "card-head";
  head.append(bulb(), inlineText("span", "card-title", "Finding"));
  const tag = inlineText("span", "card-tag", `${finding.evidence.length} sources`);
  head.append(tag);
  card.append(head);

  const claim = document.createElement("p");
  claim.className = "finding-claim";
  renderInline(claim, finding.claim);
  card.append(claim);

  const why = document.createElement("details");
  why.className = "finding-why";
  const summary = document.createElement("summary");
  summary.textContent = "Why this is marked";
  why.append(summary);

  const reason = document.createElement("p");
  reason.className = "card-body";
  renderInline(reason, finding.reason);
  why.append(reason);

  const list = document.createElement("ul");
  list.className = "finding-evidence";
  for (const entry of finding.evidence) {
    const item = document.createElement("li");
    item.append(inlineText("code", "finding-source-name", entry.name));
    item.append(inlineText("span", "finding-source-summary", entry.summary));
    list.append(item);
  }
  why.append(list);

  const limits = document.createElement("p");
  limits.className = "card-note";
  limits.textContent =
    "zorp checked that these sources are things this run actually used. It did not check that the claim is true, or that it is new.";
  why.append(limits);

  card.append(why);
  return card;
}

function inlineText(tag: string, className: string, text: string): HTMLElement {
  const node = document.createElement(tag);
  node.className = className;
  node.textContent = text;
  return node;
}

/** The marker's icon. Drawn here so it needs no font and no network. */
function bulb(): SVGSVGElement {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("class", "glyph");
  svg.setAttribute("viewBox", "0 0 24 24");
  // Decorative: the card is already named and labelled in text.
  svg.setAttribute("aria-hidden", "true");
  svg.setAttribute("fill", "none");
  svg.setAttribute("stroke", "currentColor");
  svg.setAttribute("stroke-width", "1.7");
  svg.setAttribute("stroke-linecap", "round");
  svg.setAttribute("stroke-linejoin", "round");

  const glass = document.createElementNS("http://www.w3.org/2000/svg", "path");
  glass.setAttribute("d", "M9 17a6 6 0 116 0c-.8.6-1.2 1.3-1.3 2.2H10.3C10.2 18.3 9.8 17.6 9 17z");
  svg.append(glass);

  const base = document.createElementNS("http://www.w3.org/2000/svg", "path");
  base.setAttribute("d", "M10.3 21.4h3.4");
  svg.append(base);

  return svg;
}

function codeBlock(body: string, lang: string): HTMLElement {
  const block = document.createElement("pre");
  block.className = "code-block";
  const code = document.createElement("code");
  code.textContent = body.replace(/\n+$/, "");
  if (lang) {
    block.dataset.lang = lang;
  }
  block.append(code);
  return block;
}

const UNORDERED = /^(\s*)[-*+]\s+(.*)$/;
const ORDERED = /^(\s*)\d+[.)]\s+(.*)$/;

function isListItem(line: string): boolean {
  return UNORDERED.test(line) || ORDERED.test(line);
}

/**
 * Render one list, including nested ones, and return the index of the first
 * line that was not part of it.
 *
 * Nesting is by leading whitespace: an item indented further than the item
 * above it starts a sublist. That is the rule the common cases follow and it
 * needs no lookahead beyond the current line.
 */
function renderList(target: HTMLElement, lines: string[], start: number): number {
  const first = lines[start];
  const ordered = ORDERED.test(first);
  const baseIndent = indentOf(first);
  const list = document.createElement(ordered ? "ol" : "ul");
  list.className = "md-list";

  let index = start;
  let item: HTMLElement | null = null;

  while (index < lines.length) {
    const line = lines[index];
    if (!line.trim()) {
      // A blank line ends the list unless what follows is more of the same
      // list. "Same" includes the kind: a bullet list followed by a blank
      // line and then a numbered list is two lists, and treating it as one
      // silently rendered the numbered items as bullets.
      const next = lines[index + 1] ?? "";
      if (!isListItem(next) || ORDERED.test(next) !== ordered) {
        break;
      }
      index += 1;
      continue;
    }
    if (!isListItem(line)) {
      break;
    }
    const indent = indentOf(line);
    if (indent > baseIndent && item) {
      index = renderList(item, lines, index);
      continue;
    }
    if (indent < baseIndent) {
      break;
    }
    // Strictly this list's kind. A line of the other kind at the same indent
    // starts a new list rather than joining this one.
    const match = line.match(ordered ? ORDERED : UNORDERED);
    if (!match) {
      break;
    }
    item = document.createElement("li");
    renderInline(item, match[2]);
    list.append(item);
    index += 1;
  }

  target.append(list);
  return index;
}

function indentOf(line: string): number {
  return (line.match(/^\s*/)?.[0] ?? "").replace(/\t/g, "    ").length;
}

function isTableSeparator(line: string): boolean {
  return /^\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?\s*$/.test(line);
}

function table(rows: string[]): HTMLElement {
  const wrap = document.createElement("div");
  wrap.className = "md-table-wrap";
  const node = document.createElement("table");
  node.className = "md-table";
  const cells = rows.map(splitRow);

  const head = document.createElement("thead");
  const headRow = document.createElement("tr");
  for (const cell of cells[0] ?? []) {
    const th = document.createElement("th");
    renderInline(th, cell);
    headRow.append(th);
  }
  head.append(headRow);
  node.append(head);

  const body = document.createElement("tbody");
  // Row 0 is the header and row 1 is the `---` separator, so data starts
  // at 2.
  for (const row of cells.slice(2)) {
    const tr = document.createElement("tr");
    for (const cell of row) {
      const td = document.createElement("td");
      renderInline(td, cell);
      tr.append(td);
    }
    body.append(tr);
  }
  node.append(body);
  wrap.append(node);
  return wrap;
}

function splitRow(row: string): string[] {
  return row
    .trim()
    .replace(/^\|/, "")
    .replace(/\|$/, "")
    .split("|")
    .map((c) => c.trim());
}

/**
 * Inline markup: code, links, bold, italic. Code is handled first and its
 * contents are never scanned again, so backticks around `**text**` show the
 * asterisks rather than emphasising anything.
 */
export function renderInline(target: HTMLElement, text: string): void {
  const parts = (text ?? "").split("`");
  parts.forEach((part, index) => {
    if (index % 2 === 1) {
      const code = document.createElement("code");
      code.className = "inline-code";
      code.textContent = part;
      target.append(code);
      return;
    }
    if (part) {
      renderLinksAndEmphasis(target, part);
    }
  });
}

/**
 * A link, but not the `[...](...)` half of an image.
 *
 * The leading `(^|[^!])` is load bearing. Without it this matched inside
 * `![alt](url)` and left the `!` behind as text, so every image the model
 * emitted became a clickable link pointing at whatever URL it chose. Nothing
 * was fetched, since no `img` was ever created, but "not a beacon" is not the
 * same as "not a link".
 */
const LINK = /(^|[^!])\[([^\]]*)\]\(([^)\s]+)\)/;

function renderLinksAndEmphasis(target: HTMLElement, text: string): void {
  let rest = text;
  for (;;) {
    const match = rest.match(LINK);
    if (!match || match.index === undefined) {
      break;
    }
    const [whole, before, label, href] = match;
    // `before` is the character the pattern had to consume to prove this is
    // not an image. It belongs to the text, not to the link.
    renderEmphasis(target, rest.slice(0, match.index) + before);
    if (isSafeHref(href)) {
      const anchor = document.createElement("a");
      anchor.href = href;
      anchor.target = "_blank";
      // Without noopener the opened page gets a handle on this one through
      // window.opener. noreferrer keeps the chat's URL out of its logs.
      anchor.rel = "noopener noreferrer";
      renderEmphasis(anchor, label || href);
      target.append(anchor);
    } else {
      // Not silently dropped. A link the renderer will not make clickable
      // still shows its text and its URL, so nothing disappears. `before` is
      // already on the page, so only the link part is repeated here.
      renderEmphasis(target, whole.slice(before.length));
    }
    rest = rest.slice(match.index + whole.length);
  }
  renderEmphasis(target, rest);
}

function isSafeHref(href: string): boolean {
  const lowered = href.trim().toLowerCase();
  // A relative link cannot carry a scheme, so it cannot be `javascript:`.
  if (lowered.startsWith("/") || lowered.startsWith("#")) {
    return true;
  }
  return SAFE_SCHEMES.some((scheme) => lowered.startsWith(scheme));
}

const EMPHASIS = /(\*\*|__)(.+?)\1|(\*|_)(.+?)\3/;

function renderEmphasis(target: Node, text: string): void {
  let rest = text;
  for (;;) {
    const match = rest.match(EMPHASIS);
    if (!match || match.index === undefined) {
      break;
    }
    if (match.index > 0) {
      target.appendChild(document.createTextNode(rest.slice(0, match.index)));
    }
    const strong = Boolean(match[1]);
    const node = document.createElement(strong ? "strong" : "em");
    node.textContent = strong ? match[2] : match[4];
    target.appendChild(node);
    rest = rest.slice(match.index + match[0].length);
  }
  if (rest) {
    target.appendChild(document.createTextNode(rest));
  }
}
