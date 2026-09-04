/**
 * One tool call in the transcript.
 *
 * The server names a call as `run_command(<command>)` and follows it with a
 * short result such as `exited 0`. This draws that as a bullet, the tool
 * name, a one line brief of what the command does, and the result, with the
 * full command one click away under the line.
 *
 * The brief is computed here, in code, from the command text. No model is
 * asked to describe the call, because the description would then be one more
 * model authored line in a transcript that is meant to be a record of what
 * ran. The full command stays on the page for the same reason: a person must
 * always be able to see exactly what ran, not a summary of it.
 *
 * Everything here goes through `textContent`. The command came from a model
 * that has been reading tool results and web pages, and a brief derived from
 * it is the same untrusted text in a shorter form. Neither is markup and
 * neither can become markup.
 */

/** Longest brief drawn on the line, in characters. */
export const BRIEF_MAX = 80;

/** The command the model asked for, when the server put it in the name. */
export interface ToolCall {
  tool: string;
  command: string | null;
}

/**
 * Split `run_command(pandoc a.html -o a.pdf)` into the tool and the
 * command. A bare tool name has no command.
 *
 * The command is whatever sits between the first `(` and the final `)`, so
 * a command that itself ends in a parenthesis survives intact.
 */
export function splitCall(name: string): ToolCall {
  const open = name.indexOf("(");
  if (open > 0 && name.endsWith(")") && /^[A-Za-z0-9_.:-]+$/.test(name.slice(0, open))) {
    return { tool: name.slice(0, open), command: name.slice(open + 1, -1) };
  }
  return { tool: name, command: null };
}

/**
 * A one line brief of a shell command: the first program and its first few
 * positional arguments, which is where the file paths and package names
 * are, with everything else elided.
 *
 * A leading `cd dir &&` says where the command ran rather than what it did,
 * so it is dropped. Flags and redirections are dropped too. Anything dropped
 * or cut leaves a trailing ellipsis so the line never reads as the whole
 * command.
 */
export function briefCommand(command: string): string {
  let rest = command.trim();
  rest = rest.replace(/^(?:cd\s+(?:"[^"]*"|'[^']*'|\S+)\s*(?:&&|;)\s*)+/, "");
  const { tokens, more } = firstSegment(rest);
  let elided = more;

  // `FOO=bar cmd` sets the environment for the command. Not what it does.
  while (tokens.length && /^[A-Za-z_][A-Za-z0-9_]*=/.test(tokens[0])) {
    tokens.shift();
    elided = true;
  }
  const program = tokens.shift();
  if (!program) {
    return cap(rest);
  }

  const kept: string[] = [program];
  let skipNext = false;
  for (const token of tokens) {
    if (skipNext) {
      skipNext = false;
      elided = true;
      continue;
    }
    // A bare redirection operator takes the next token as its target.
    if (/^\d*(>>?|<|&>)$/.test(token)) {
      skipNext = true;
      elided = true;
      continue;
    }
    if (token.startsWith("-") || /^\d*[<>]/.test(token) || token.startsWith("&")) {
      elided = true;
      continue;
    }
    if (kept.length > 3) {
      elided = true;
      continue;
    }
    kept.push(token);
  }
  return cap(kept.join(" ") + (elided ? " …" : ""));
}

function cap(text: string): string {
  if (text.length <= BRIEF_MAX) {
    return text;
  }
  return `${text.slice(0, BRIEF_MAX - 1).trimEnd()}…`;
}

/**
 * The tokens of the first simple command, split on whitespace outside
 * quotes, and whether a pipeline or list continues after it. Quotes stay in
 * the tokens so a quoted argument reads the way it was written.
 */
function firstSegment(command: string): { tokens: string[]; more: boolean } {
  const tokens: string[] = [];
  let current = "";
  let quote: string | null = null;
  let more = false;
  const push = (): void => {
    if (current) {
      tokens.push(current);
    }
    current = "";
  };
  for (let index = 0; index < command.length; index += 1) {
    const char = command[index];
    if (quote) {
      current += char;
      if (char === quote) {
        quote = null;
      }
      continue;
    }
    if (char === '"' || char === "'") {
      quote = char;
      current += char;
      continue;
    }
    if (char === "\\" && index + 1 < command.length) {
      current += char + command[index + 1];
      index += 1;
      continue;
    }
    if (char === "|" || char === ";" || char === "\n" || (char === "&" && command[index + 1] === "&")) {
      more = true;
      break;
    }
    if (/\s/.test(char)) {
      push();
      continue;
    }
    current += char;
  }
  push();
  return { tokens, more };
}

function el(doc: Document, tag: string, className = ""): HTMLElement {
  const node = doc.createElement(tag);
  if (className) {
    node.className = className;
  }
  return node;
}

function text(doc: Document, tag: string, className: string, value: string): HTMLElement {
  const node = el(doc, tag, className);
  node.textContent = value;
  return node;
}

/**
 * The line for a tool event: `name` as the server sent it, `status` as the
 * result text that follows it.
 */
export function toolLine(doc: Document, name: string, status: string): HTMLElement {
  const { tool, command } = splitCall(name);
  return callLine(doc, tool, command, status);
}

/**
 * A bullet, the tool name, a brief of the command when there is one, and
 * the status as one piece that never breaks in the middle.
 *
 * With a command the line is the summary of a closed `details` element
 * whose body is the full command, verbatim. Without one there is nothing
 * more to show, so the line is a plain block and no empty container is
 * left under it.
 */
export function callLine(
  doc: Document,
  tool: string,
  command: string | null,
  status: string,
  statusClass = "",
): HTMLElement {
  const column = el(doc, "span", "activity-text");
  column.append(text(doc, "span", "activity-name", tool));
  if (command !== null) {
    column.append(" ", text(doc, "span", "activity-brief", briefCommand(command)));
  }
  if (status) {
    const className = statusClass ? `activity-status ${statusClass}` : "activity-status";
    column.append(" ", text(doc, "span", className, status));
  }

  const line = el(doc, command === null ? "div" : "summary", "activity-line");
  line.append(text(doc, "span", "activity-bullet", "●"), column);
  if (command === null) {
    return line;
  }

  line.title = "Show the full command";
  const details = el(doc, "details", "activity-call");
  const full = el(doc, "pre", "activity-full");
  const code = doc.createElement("code");
  code.textContent = command;
  full.append(code);
  details.append(line, full);
  return details;
}
