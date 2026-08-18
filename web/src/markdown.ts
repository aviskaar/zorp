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

/** Schemes a link is allowed to have. Anything else renders as plain text. */
const SAFE_SCHEMES = ["http://", "https://", "mailto:"];

export function renderMarkdown(target: HTMLElement, source: string): void {
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
      target.append(codeBlock(body.join("\n"), lang));
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
      renderMarkdown(quote, body.join("\n"));
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

/**
 * Split a table row on its cell boundaries.
 *
 * A pipe preceded by a backslash is text, not a boundary. That is what GFM
 * says, and it is what keeps a cell whose contents contain a pipe from adding
 * a column: the server escapes them that way when it turns a Word or
 * OpenDocument table into markdown.
 */
function splitRow(row: string): string[] {
  const trimmed = row.trim().replace(/^\|/, "").replace(/\|$/, "");
  const cells: string[] = [];
  let current = "";
  for (let index = 0; index < trimmed.length; index += 1) {
    const char = trimmed[index];
    if (char === "\\" && trimmed[index + 1] === "|") {
      current += "|";
      index += 1;
      continue;
    }
    if (char === "|") {
      cells.push(current.trim());
      current = "";
      continue;
    }
    current += char;
  }
  cells.push(current.trim());
  return cells;
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
