/**
 * One tool call in the transcript.
 *
 * The server names a call as `run_command(<command>)` and follows it with a
 * short result such as `exited 0`. This draws that as a bullet and a phrase
 * saying what the command does, with the full command one click away under
 * the line. A call that carries no command, `write_file` or `read_file`,
 * keeps its tool name on the line, and so does a `verify` line, since there
 * the name is the point.
 *
 * The result is not written on the line. It is the bullet's colour, from
 * exactly one of the `LINE_STATES` classes on the line: green for a call
 * that succeeded, red for one that failed, and a pulsing in-progress colour
 * for a call that has started and not yet reported. The result text stays
 * reachable: it is the line's `title`, and for a shell call it sits under
 * the verbatim command in the details. `stateForStatus` is the one mapping
 * from the server's status word to a colour.
 *
 * The phrase on the line is the model's own description of its call, given
 * in the call's `description` argument next to the command, drawn through
 * `textContent` and labelled as model text. When the model gave none, the
 * phrase is computed here, in code, from the command: a lookup on the
 * program name and its first subcommand, plus the first positional
 * argument, in the table below. Either way the verbatim command stays one
 * click under the line, because a person must always be able to see
 * exactly what ran, and a description can be wrong.
 *
 * Everything here goes through `textContent`. The command came from a model
 * that has been reading tool results and web pages, the description is that
 * model's words, and a phrase derived from the command is the same untrusted
 * text in a shorter form. None of it is markup and none of it can become
 * markup.
 */

/** Longest phrase drawn on the line, in characters. */
export const BRIEF_MAX = 80;

/** What hovering the model's phrase says it is. */
const MODEL_PHRASE_TITLE = "The model's own description of this call. Click to see the command that ran.";

/** What hovering a shell line says, after its status, when it has one. */
const COMMAND_HINT = "Click to see the command that ran.";

/** The title a shell line has while it is running. */
const RUNNING_SHELL_TITLE = "Running. Click to see the command.";

/**
 * The state a line is in, carried as exactly one of these classes on the
 * `.activity-line` element. The bullet takes its colour from them, and the
 * activity group reads them off its lines, so they are a contract.
 */
export const LINE_STATES = ["activity-ok", "activity-fail", "activity-running"] as const;
export type LineState = (typeof LINE_STATES)[number];

/**
 * The status words that mean a call failed. This is the vocabulary the
 * agent's tools produce as `ToolOutput::summary`, and what the agent loop
 * itself counts as not succeeded (`denied`, `error`, `unknown tool`), plus
 * the shell's own outcomes and the subagent tool's stopped states. `verify`
 * says `failed`.
 */
const FAILED = new Set([
  "timed out",
  "cancelled",
  "failed",
  "error",
  "denied",
  "unknown tool",
  "blocked",
  "step limit",
  "repeated action",
  "verification failed",
]);

/**
 * The colour for a finished call's status word. `exited 0` is ok and any
 * other exit code is a failure; the words in `FAILED`, and a status that
 * starts as an error, a denial or a withheld result, are failures too. A
 * patch of which nothing applied failed as well.
 *
 * Everything else is ok, including an empty status, because the other tools
 * summarise a success in their own words: `a.txt (12 lines)`,
 * `created b.md (3 lines)`, `'query' (4 matches)`, `started PID 512`,
 * `loaded skill x`. A rule that read those as failures would paint a
 * working session red. The failure vocabulary is the smaller and the more
 * stable of the two, and it is the one the agent loop uses.
 */
export function stateForStatus(status: string): "activity-ok" | "activity-fail" {
  const word = status.trim();
  const exited = /^exited (-?\d+)$/.exec(word);
  if (exited) {
    return exited[1] === "0" ? "activity-ok" : "activity-fail";
  }
  if (FAILED.has(word) || /^(error|denied|withheld)\b/.test(word) || /^0\/\d+ blocks applied$/.test(word)) {
    return "activity-fail";
  }
  return "activity-ok";
}

/** The command the model asked for, when the server put it in the name. */
export interface ToolCall {
  tool: string;
  command: string | null;
}

/** The tools whose command the server puts in the name. */
const SHELL_TOOLS = new Set(["run_command", "start_background_process"]);

/**
 * The verb phrase for a program, or for a program and its subcommands,
 * keyed by the first one to three positional words. `{}` is where the next
 * positional argument goes; a phrase without it takes no object.
 */
const PHRASES = new Map<string, string>();
for (const [programs, phrase] of [
  ["ls, tree, find, fd, exa, eza", "Listing files in {}"],
  ["cat, head, tail, less, more, bat", "Reading {}"],
  ["grep, rg, ag, ugrep", 'Searching for "{}"'],
  ["wc", "Counting lines in {}"],
  ["mkdir", "Creating directory {}"],
  ["touch", "Creating {}"],
  ["rm, rmdir", "Removing {}"],
  ["cp", "Copying {}"],
  ["mv", "Moving {}"],
  ["chmod, chown", "Changing permissions on {}"],
  ["sed, awk", "Editing text"],
  ["echo, printf", "Printing"],
  ["curl, wget", "Fetching {}"],
  ["pandoc", "Converting {}"],
  ["python, python3, node, bun, deno, ruby, perl, bash, sh, zsh", "Running a script"],
  ["make, cargo build, npm run build, pnpm run build, yarn run build, bun run build", "Building"],
  ["cargo check", "Checking"],
  ["cargo clippy", "Linting"],
  ["cargo fmt", "Formatting"],
  ["cargo run", "Running"],
  ["cargo add", "Adding a dependency"],
  ["cargo test, npm test, pnpm test, yarn test, bun test, pytest, jest, vitest, go test", "Running tests"],
  ["npm run, pnpm run, yarn run, bun run", "Running {}"],
  ["npm install, npm ci, npm add, pnpm install, pnpm add, yarn install, yarn add", "Installing packages"],
  ["bun install, bun add, pip install, pip3 install, uv pip install, uv add, uv sync", "Installing packages"],
  ["git status", "Checking git status"],
  ["git diff", "Showing changes"],
  ["git log", "Reading git history"],
  ["git add", "Staging changes"],
  ["git commit", "Committing"],
  ["git push", "Pushing"],
  ["git pull, git fetch", "Fetching from remote"],
  ["git checkout, git switch", "Switching branch"],
  ["git branch", "Listing branches"],
  ["git stash", "Stashing changes"],
  ["git clone", "Cloning"],
  ["docker build", "Building an image"],
  ["docker run", "Starting a container"],
  ["docker compose up", "Starting containers"],
  ["kill, pkill", "Stopping a process"],
  ["ps, top, lsof", "Listing processes"],
  ["which, whereis, type", "Locating a program"],
  ["env, printenv", "Reading the environment"],
  ["date, pwd, whoami, uname", "Checking the system"],
  ["cd", "Changing directory to {}"],
]) {
  for (const program of programs.split(", ")) {
    PHRASES.set(program, phrase);
  }
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
 * A plain phrase for what a shell command does: the verb for its program
 * and subcommand from the table above, and its first positional argument
 * when the verb takes one.
 *
 * A leading `cd dir &&` says where the command ran rather than what it did,
 * so it is dropped. Flags, redirections and environment assignments are
 * dropped too, except that `echo` and its kin into a redirection are
 * writing that file. A pipeline or list after the first command leaves a
 * trailing ellipsis so the line never reads as the whole command.
 */
export function describeCommand(command: string): string {
  let rest = command.trim();
  rest = rest.replace(/^(?:cd\s+(?:"[^"]*"|'[^']*'|\S+)\s*(?:&&|;)\s*)+/, "");
  const { tokens, more } = firstSegment(rest);

  // `FOO=bar cmd` sets the environment for the command. Not what it does.
  while (tokens.length && /^[A-Za-z_][A-Za-z0-9_]*=/.test(tokens[0])) {
    tokens.shift();
  }
  if (!tokens.length) {
    return cap(rest);
  }

  // The program by its basename, then its positional arguments.
  const words: string[] = [tokens[0].replace(/^.*\//, "")];
  let target: string | null = null;
  let operator: string | null = null;
  for (const token of tokens.slice(1)) {
    if (operator) {
      // A bare redirection operator took this token as its target.
      if (operator.includes(">")) {
        target = target ?? token;
      }
      operator = null;
      continue;
    }
    if (/^\d*(>>?|<|&>)$/.test(token)) {
      operator = token;
      continue;
    }
    if (token.startsWith("-") || /^\d*[<>]/.test(token) || token.startsWith("&")) {
      continue;
    }
    words.push(token);
  }

  let phrase = `Running ${words.slice(0, 2).map(unquote).join(" ")}`;
  for (let width = Math.min(3, words.length); width > 0; width -= 1) {
    const found = PHRASES.get(words.slice(0, width).join(" "));
    if (found) {
      phrase = fill(found, words[width]);
      break;
    }
  }
  if (target && ["echo", "printf", "cat"].includes(words[0])) {
    phrase = `Writing ${unquote(target)}`;
  }
  return cap(phrase + (more ? " …" : ""));
}

/**
 * The model's description of its call, made fit for one line: its first
 * line only, with control, invisible and bidirectional characters gone,
 * whitespace collapsed, wrapping quotes and markdown marks trimmed, and
 * capped at `BRIEF_MAX`. `null` when nothing is left, so the caller falls
 * back to the phrase computed from the command.
 */
export function clampPhrase(raw: string | null | undefined): string | null {
  if (!raw) {
    return null;
  }
  const phrase = raw
    .split(/[\n\r\u0085\u2028\u2029]/, 1)[0]
    .replace(/[\p{Cc}\u200B-\u200F\u202A-\u202E\u2060-\u2064\u2066-\u2069\uFEFF]/gu, "")
    .replace(/\s+/g, " ")
    .replace(/^["'`*#\s]+|["'`*#\s]+$/g, "");
  return phrase ? cap(phrase) : null;
}

/**
 * Put the object into the phrase. Without one, the slot goes, and so does
 * the joining word in front of it: "Listing files in {}" is "Listing files".
 */
function fill(phrase: string, object: string | undefined): string {
  if (object === undefined) {
    return phrase.replace(/(?: (?:in|for|on|to))? "?\{\}"?$/, "");
  }
  return phrase.replace("{}", unquote(object));
}

function unquote(token: string): string {
  return token.replace(/^(["'])(.*)\1$/s, "$2");
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
 * result text that follows it, `phrase` as the model's own description of
 * the call when the event carried one. Settled on the spot, since the
 * result is known. A reopened session draws every stored call this way.
 */
export function toolLine(doc: Document, name: string, status: string, phrase?: string | null): HTMLElement {
  const { tool, command } = splitCall(name);
  const node = callLine(doc, tool, command, phrase);
  settleLine(node, status);
  return node;
}

/**
 * The line for a `tool_started` event: the same call, drawn in progress.
 * The `tool` event that follows for it hands the node to `settleLine`.
 */
export function startedLine(doc: Document, name: string, phrase?: string | null): HTMLElement {
  const { tool, command } = splitCall(name);
  const node = callLine(doc, tool, command, phrase);
  const line = lineOf(node);
  line.classList.add("activity-running");
  line.title = node === line ? "Running" : RUNNING_SHELL_TITLE;
  return node;
}

/**
 * Bring a line to the state of its result, in place. `state` is read off
 * `status` unless the caller knows better: a line abandoned by the end of
 * the turn has no status word of its own and is failed with a sentence.
 *
 * The status goes into the line's `title`, and for a shell call under the
 * command in the details as well, through `textContent`; it is text the
 * server derived from a tool result and it is never markup. The line ends
 * up with exactly one of `LINE_STATES` on it whatever it had before.
 */
export function settleLine(node: HTMLElement, status: string, state: LineState = stateForStatus(status)): void {
  const line = lineOf(node);
  line.classList.remove(...LINE_STATES);
  line.classList.add(state);
  if (node === line) {
    line.title = status;
    return;
  }
  line.title = status ? `${status}. ${COMMAND_HINT}` : "Show the full command";
  let result = node.querySelector(".activity-result");
  if (!status) {
    result?.remove();
    return;
  }
  if (!result) {
    result = el(node.ownerDocument, "div", "activity-result");
    node.append(result);
  }
  result.textContent = status;
}

/** The `.activity-line` of a node `callLine` returned: the node itself, or the summary of its details. */
function lineOf(node: HTMLElement): HTMLElement {
  return node.classList.contains("activity-line") ? node : (node.querySelector(".activity-line") as HTMLElement);
}

/**
 * A bullet and the phrase for the command when there is one. The tool name
 * is drawn when there is no command to describe, and on a `verify` line,
 * where it says what the line is; beside a sentence, `run_command` is
 * noise. `word` is a status word written on the line, which only `verify`
 * does: a tool line carries its result as colour, through `settleLine`.
 *
 * The phrase is the model's own, clamped, when it gave one, and then the
 * span says so in its class and its title; otherwise it is computed from
 * the command. A call with no command has nothing to describe, and a
 * phrase given for one is ignored.
 *
 * With a command the line is the summary of a closed `details` element
 * whose body is the full command, verbatim. Without one there is nothing
 * more to show, so the line is a plain block and no empty container is
 * left under it. The node comes back in no state; `settleLine` and
 * `startedLine` put one on it.
 */
export function callLine(
  doc: Document,
  tool: string,
  command: string | null,
  phrase?: string | null,
  word?: string,
): HTMLElement {
  const spans: HTMLElement[] = [];
  if (command === null || !SHELL_TOOLS.has(tool)) {
    spans.push(text(doc, "span", "activity-name", tool));
  }
  if (command !== null) {
    const own = clampPhrase(phrase);
    if (own === null) {
      spans.push(text(doc, "span", "activity-brief", describeCommand(command)));
    } else {
      const brief = text(doc, "span", "activity-brief activity-brief-model", own);
      brief.title = MODEL_PHRASE_TITLE;
      spans.push(brief);
    }
  }
  if (word) {
    spans.push(text(doc, "span", "activity-status", word));
  }
  const column = el(doc, "span", "activity-text");
  column.append(...spans.flatMap((span, index) => (index ? [" ", span] : [span])));

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
