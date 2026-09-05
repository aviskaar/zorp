/**
 * Tests for the tool call line.
 *
 * The injection cases come first, for the reason `markdown.test.ts` gives:
 * the command on the line is model output, and a line that turned any of it
 * into markup would be a cross-site scripting hole. The phrase is either
 * the model's own words or derived from the same text, and gets the same
 * treatment.
 *
 * The layout cases are structural. jsdom does not lay anything out, so the
 * wrapping fix is pinned by the shape of the DOM and by reading the rule
 * from `styles.css` as text, the same arrangement `send-control.test.ts`
 * uses for a cascade question.
 */

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { JSDOM } from "jsdom";

import {
  BRIEF_MAX,
  LINE_STATES,
  callLine,
  clampPhrase,
  describeCommand,
  settleLine,
  splitCall,
  startedLine,
  stateForStatus,
  toolLine,
} from "../src/activity-line.ts";
import type { Message } from "../src/api.ts";

const dom = new JSDOM("<!doctype html><body></body>");
const doc = dom.window.document;
const css = readFileSync(new URL("../styles.css", import.meta.url), "utf8");

/** The CSS declarations for one selector, or "" when it has no rule. */
function rule(selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = css.match(new RegExp(`(?:^|\\n)${escaped}\\s*\\{([^}]*)\\}`));
  return match ? match[1] : "";
}

function line(name: string, status = "exited 0"): HTMLElement {
  const node = toolLine(doc as unknown as Document, name, status);
  doc.body.append(node);
  return node;
}

/** The `.activity-line` of what `toolLine` returned: the node, or the summary of its details. */
function lineOf(node: HTMLElement): HTMLElement {
  return node.classList.contains("activity-line") ? node : (node.querySelector(".activity-line") as HTMLElement);
}

/** The state classes on the line, which the contract says is exactly one. */
function states(node: HTMLElement): string[] {
  return LINE_STATES.filter((state) => lineOf(node).classList.contains(state));
}

/* injection */

test("a script tag as the command is text on the line and in the details", () => {
  const command = "<script>alert(1)</script>";
  const node = line(`run_command(${command})`);
  assert.equal(node.querySelectorAll("script").length, 0);
  assert.equal(node.querySelector(".activity-full code")?.textContent, command);
  assert.ok(node.querySelector(".activity-brief")?.textContent?.startsWith("Running "));
});

test("a tag in the phrase is text, not an element", () => {
  const command = "<img src=x onerror=alert(1)>";
  const node = line(`run_command(${command})`);
  assert.equal(node.querySelectorAll("img").length, 0);
  assert.equal(node.querySelector(".activity-brief")?.textContent, "Running <img src=x");
  assert.equal(node.querySelector(".activity-full code")?.textContent, command);
});

test("a closing details tag in the command cannot close the details", () => {
  const command = "echo </details><img src=x onerror=alert(1)>";
  const host = doc.createElement("div");
  host.append(line(`run_command(${command})`));
  assert.equal(host.querySelectorAll("details").length, 1);
  assert.equal(host.querySelectorAll("img").length, 0);
  assert.equal(host.querySelector(".activity-full code")?.textContent, command);
});

test("the full command is byte identical to the input", () => {
  const command = "cd /tmp && printf '%s\\n' \"a  b\"\t<tag> & && | ; $(date) `id`";
  const node = line(`run_command(${command})`);
  assert.equal(node.querySelector(".activity-full code")?.textContent, command);
});

test("a status with markup in it is text, in the title and under the command", () => {
  const node = line("run_command(ls)", "<b>exited 0</b>");
  assert.equal(node.querySelectorAll("b").length, 0);
  assert.equal(lineOf(node).title, "<b>exited 0</b>. Click to see the command that ran.");
  assert.equal(node.querySelector(".activity-result")?.textContent, "<b>exited 0</b>");
  const bare = line("write_file", "<img src=x onerror=alert(1)>");
  assert.equal(bare.querySelectorAll("img").length, 0);
  assert.equal(bare.title, "<img src=x onerror=alert(1)>");
});

/* structure */

test("the phrase is the only text on the line: no tool name and no status word", () => {
  const node = line("run_command(pandoc in.html -o out.pdf)");
  const column = node.querySelector(".activity-line > .activity-text");
  assert.ok(column, "the text sits in one column beside the bullet");
  assert.equal(column?.querySelectorAll(".activity-name").length, 0);
  assert.equal(column?.querySelector(".activity-brief")?.textContent, "Converting in.html");
  assert.equal(column?.querySelectorAll(".activity-status").length, 0);
  assert.equal(lineOf(node).textContent, "●Converting in.html");
  assert.equal(node.textContent, "●Converting in.htmlpandoc in.html -o out.pdfexited 0");
});

test("the status is absent from the line's text and present in its title", () => {
  const shell = line("run_command(cargo test)", "exited 101");
  assert.ok(!lineOf(shell).textContent?.includes("exited 101"));
  assert.equal(lineOf(shell).title, "exited 101. Click to see the command that ran.");
  assert.equal(shell.querySelector(".activity-result")?.textContent, "exited 101");
  const bare = line("read_file", "a.txt (12 lines)");
  assert.equal(bare.textContent, "●read_file");
  assert.equal(bare.title, "a.txt (12 lines)");
});

test("a background process reads as its phrase too", () => {
  const node = line("start_background_process(npm run dev)", "started");
  assert.equal(node.querySelectorAll(".activity-name").length, 0);
  assert.equal(node.querySelector(".activity-brief")?.textContent, "Running dev");
});

test("the status is styled as one piece that does not break", () => {
  assert.match(rule(".activity-status"), /white-space:\s*nowrap/);
  assert.match(rule(".activity-text"), /min-width:\s*0/);
  assert.doesNotMatch(rule(".activity-name"), /flex:\s*none/);
});

test("a call with a command is a closed details whose body is the full command", () => {
  const node = line("run_command(pandoc in.html -o out.pdf)");
  assert.equal(node.tagName, "DETAILS");
  assert.equal((node as HTMLDetailsElement).open, false);
  assert.equal(node.firstElementChild?.tagName, "SUMMARY");
  assert.equal((node.firstElementChild as HTMLElement).title, "exited 0. Click to see the command that ran.");
  assert.equal(node.querySelector("pre.activity-full code")?.textContent, "pandoc in.html -o out.pdf");
});

test("a call without a command is a plain line that keeps its tool name", () => {
  const node = line("write_file", "created a.html (12 lines)");
  assert.equal(node.tagName, "DIV");
  assert.equal(node.querySelectorAll("details, pre").length, 0);
  assert.equal(node.querySelector(".activity-name")?.textContent, "write_file");
  assert.equal(node.querySelectorAll(".activity-status").length, 0);
  assert.equal(node.title, "created a.html (12 lines)");
  assert.deepEqual(states(node), ["activity-ok"]);
});

test("an empty status leaves the line with no title, no result, and the ok colour", () => {
  const node = line("read_file", "");
  assert.equal(node.querySelectorAll(".activity-status, .activity-result").length, 0);
  assert.equal(node.querySelectorAll("pre").length, 0);
  assert.equal(node.textContent, "●read_file");
  assert.equal(node.title, "");
  assert.deepEqual(states(node), ["activity-ok"]);
});

test("a verify line keeps its name and its word, and carries the verdict as the line's colour", () => {
  const node = callLine(doc as unknown as Document, "verify", "cargo test", null, "failed");
  settleLine(node, "failed");
  assert.equal(node.querySelector(".activity-name")?.textContent, "verify");
  assert.equal(node.querySelector(".activity-brief")?.textContent, "Running tests");
  assert.equal(node.querySelector(".activity-status")?.textContent, "failed");
  assert.deepEqual(states(node), ["activity-fail"]);
  assert.equal(node.querySelector(".activity-full code")?.textContent, "cargo test");
  const passed = callLine(doc as unknown as Document, "verify", "cargo test", null, "passed");
  settleLine(passed, "passed");
  assert.deepEqual(states(passed), ["activity-ok"]);
});

/* the colour */

test("the status word maps to ok or fail, and the other tools' summaries are not failures", () => {
  const ok = [
    "exited 0",
    "passed",
    "",
    "finished",
    "a.txt (12 lines)",
    "created a.html (12 lines)",
    "web/src (9 entries)",
    "'briefCommand' (4 matches)",
    "'zorp' (3 results)",
    "started PID 512",
    "killed PID 512",
    "listed processes",
    "loaded skill pdf",
    "2 changed files",
    "diff (40 lines)",
    "3/3 blocks applied",
    "subagent finished",
    "mcp tool result",
    "ok",
  ];
  for (const status of ok) {
    assert.equal(stateForStatus(status), "activity-ok", JSON.stringify(status));
  }
  const fail = [
    "exited 1",
    "exited 101",
    "exited -1",
    "timed out",
    "cancelled",
    "failed",
    "error",
    "denied",
    "unknown tool",
    "withheld: turn tool output budget",
    "error: no such tool",
    "denied: approval required",
    "blocked",
    "step limit",
    "repeated action",
    "verification failed",
    "0/3 blocks applied",
  ];
  for (const status of fail) {
    assert.equal(stateForStatus(status), "activity-fail", JSON.stringify(status));
  }
});

test("a finished line carries exactly one state class, read off its status", () => {
  assert.deepEqual(states(line("run_command(ls)", "exited 0")), ["activity-ok"]);
  assert.deepEqual(states(line("run_command(ls)", "exited 2")), ["activity-fail"]);
  assert.deepEqual(states(line("run_command(sleep 99)", "timed out")), ["activity-fail"]);
  assert.deepEqual(states(line("write_file", "denied")), ["activity-fail"]);
});

test("the bullet takes its colour from the state, and the pulse stops for reduced motion", () => {
  assert.match(css, /\.activity-ok > \.activity-bullet[^{]*\{[^}]*var\(--ok\)/);
  assert.match(css, /\.activity-fail > \.activity-bullet[^{]*\{[^}]*var\(--danger\)/);
  assert.match(css, /\n\.activity-running > \.activity-bullet\s*\{[^}]*animation:\s*pulse/);
  assert.match(css, /prefers-reduced-motion: reduce\)\s*\{\s*\.activity-running > \.activity-bullet\s*\{\s*animation:\s*none/);
  assert.equal(rule(".activity-pass"), "", "the old word class is gone");
  assert.equal(rule(".activity-fail"), "", "the state class colours the bullet, not the whole line");
});

/* a call in progress */

test("a started line is in progress until the matching result settles it in place", () => {
  const node = startedLine(doc as unknown as Document, "run_command(cargo test)", "Running the tests");
  doc.body.append(node);
  const summary = lineOf(node);
  assert.deepEqual(states(node), ["activity-running"]);
  assert.equal(summary.title, "Running. Click to see the command.");
  assert.equal(summary.querySelector(".activity-brief")?.textContent, "Running the tests");
  assert.equal(node.querySelectorAll(".activity-status, .activity-result").length, 0);
  assert.equal(node.querySelector(".activity-full code")?.textContent, "cargo test");

  settleLine(node, "exited 101");
  assert.equal(lineOf(node), summary, "the same line, not a second one");
  assert.deepEqual(states(node), ["activity-fail"]);
  assert.equal(summary.title, "exited 101. Click to see the command that ran.");
  assert.equal(node.querySelector(".activity-result")?.textContent, "exited 101");
  assert.equal(node.querySelectorAll(".activity-result").length, 1);

  settleLine(node, "exited 0");
  assert.deepEqual(states(node), ["activity-ok"]);
  assert.equal(node.querySelectorAll(".activity-result").length, 1, "the result is updated, not appended");
  assert.equal(node.querySelector(".activity-result")?.textContent, "exited 0");
});

test("a started line without a command is in progress too, and settles to its status", () => {
  const node = startedLine(doc as unknown as Document, "write_file");
  assert.equal(node.tagName, "DIV");
  assert.deepEqual(states(node), ["activity-running"]);
  assert.equal(node.title, "Running");
  settleLine(node, "created a.html (12 lines)");
  assert.deepEqual(states(node), ["activity-ok"]);
  assert.equal(node.title, "created a.html (12 lines)");
  assert.equal(node.textContent, "●write_file");
});

test("a line the turn abandoned is failed, with the reason in its title", () => {
  const node = startedLine(doc as unknown as Document, "run_command(sleep 99)");
  settleLine(node, "The turn ended before this call reported", "activity-fail");
  assert.deepEqual(states(node), ["activity-fail"]);
  assert.equal(lineOf(node).title, "The turn ended before this call reported. Click to see the command that ran.");
  assert.equal(node.querySelector(".activity-result")?.textContent, "The turn ended before this call reported");
});

/* splitting the name */

test("the server's run_command(...) form splits into tool and command", () => {
  assert.deepEqual(splitCall("run_command(ls -la)"), { tool: "run_command", command: "ls -la" });
  assert.deepEqual(splitCall("start_background_process(npm run dev)"), {
    tool: "start_background_process",
    command: "npm run dev",
  });
});

test("a command ending in a parenthesis keeps it", () => {
  assert.deepEqual(splitCall("run_command(echo $(date))"), {
    tool: "run_command",
    command: "echo $(date)",
  });
});

test("a bare tool name has no command", () => {
  assert.deepEqual(splitCall("write_file"), { tool: "write_file", command: null });
  assert.deepEqual(splitCall("(odd)"), { tool: "(odd)", command: null });
});

/* the phrase */

test("a known program reads as its verb and first positional argument", () => {
  assert.equal(describeCommand("ls -la web/src | head -3"), "Listing files in web/src …");
  assert.equal(describeCommand("cat web/src/activity-line.ts"), "Reading web/src/activity-line.ts");
  assert.equal(describeCommand('grep -rn "briefCommand" web/src'), 'Searching for "briefCommand"');
  assert.equal(describeCommand("pandoc in.html -o out.pdf"), "Converting in.html");
  assert.equal(describeCommand("rm -rf dist"), "Removing dist");
  assert.equal(describeCommand("mkdir -p a/b"), "Creating directory a/b");
});

test("a verb that takes no object stands alone", () => {
  assert.equal(describeCommand("ls"), "Listing files");
  assert.equal(describeCommand("grep -r"), "Searching");
  assert.equal(describeCommand("cd"), "Changing directory");
  assert.equal(describeCommand("cd web"), "Changing directory to web");
  assert.equal(describeCommand("echo hello"), "Printing");
  assert.equal(describeCommand("sed -n '1,40p' web/src/main.ts"), "Editing text");
});

test("a subcommand picks the phrase", () => {
  assert.equal(describeCommand("cd web && npm test"), "Running tests");
  assert.equal(describeCommand("cargo test -p zorp-agent --features research"), "Running tests");
  assert.equal(describeCommand("git status"), "Checking git status");
  assert.equal(describeCommand('git commit -m "fix: wrap lines"'), "Committing");
  assert.equal(describeCommand("npm run build"), "Building");
  assert.equal(describeCommand("npm run check"), "Running check");
  assert.equal(describeCommand("docker compose up -d"), "Starting containers");
  assert.equal(describeCommand("uv pip install ruff"), "Installing packages");
  assert.equal(describeCommand("cd '/my repo'; cd web && npm test"), "Running tests");
});

test("an unknown program reads as Running it, by basename, with its first argument", () => {
  assert.equal(describeCommand("FOO=1 /usr/local/bin/frobnicate --x a b"), "Running frobnicate a");
  assert.equal(describeCommand("./scripts/release.sh"), "Running release.sh");
});

test("printing into a redirection is writing that file", () => {
  assert.equal(describeCommand('echo "<script>alert(1)</script>" > out.html'), "Writing out.html");
  assert.equal(describeCommand("cat <<'EOF' > notes.txt\nhello\nEOF"), "Writing notes.txt …");
  assert.equal(describeCommand("FOO=1 make > build.log 2>&1"), "Building");
  assert.equal(describeCommand("cat < notes.txt"), "Reading");
});

test("the phrase is capped", () => {
  const phrase = describeCommand(`cat ${"x".repeat(300)}`);
  assert.equal(phrase.length, BRIEF_MAX);
  assert.ok(phrase.endsWith("…"));
});

test("an empty command is an empty phrase", () => {
  assert.equal(describeCommand(""), "");
  assert.equal(describeCommand("   "), "");
});

/* the model's own phrase */

const MODEL_TITLE = "The model's own description of this call. Click to see the command that ran.";

function described(name: string, phrase: string | null | undefined, status = "exited 0"): HTMLElement {
  const node = toolLine(doc as unknown as Document, name, status, phrase);
  doc.body.append(node);
  return node;
}

test("a phrase the model gave is drawn as model text, with the command still whole under the line", () => {
  const command = "cd web && ls -la src | head -3; printf '%s' \"<b>\"";
  const node = described(`run_command(${command})`, "Looking at the web sources");
  const brief = node.querySelector(".activity-brief") as HTMLElement;
  assert.equal(brief.textContent, "Looking at the web sources");
  assert.ok(brief.classList.contains("activity-brief-model"));
  assert.equal(brief.title, MODEL_TITLE);
  assert.equal(node.querySelectorAll(".activity-name").length, 0);
  assert.equal(lineOf(node).title, "exited 0. Click to see the command that ran.");
  assert.equal(node.querySelector(".activity-full code")?.textContent, command);
  assert.match(rule(".activity-brief-model"), /font-style:\s*italic/);
});

test("markup in the phrase is text, not elements", () => {
  const phrase = "<b>x</b><script>alert(1)</script>";
  const node = described("run_command(ls)", phrase);
  assert.equal(node.querySelectorAll("b, script").length, 0);
  assert.equal(node.querySelector(".activity-brief")?.textContent, phrase);
});

test("a missing, empty or blank phrase falls back to the phrase from the command", () => {
  for (const phrase of [undefined, null, "", "   ", "\n\n", '""', "**", "\u200B"]) {
    const node = described("run_command(ls web/src)", phrase);
    const brief = node.querySelector(".activity-brief") as HTMLElement;
    assert.equal(brief.textContent, "Listing files in web/src", JSON.stringify(phrase));
    assert.ok(!brief.classList.contains("activity-brief-model"));
    assert.equal(brief.title, "");
  }
});

test("a phrase keeps only its first line", () => {
  assert.equal(clampPhrase("Listing files\nrm -rf /"), "Listing files");
  assert.equal(clampPhrase("Listing files\r\nsecond"), "Listing files");
  assert.equal(clampPhrase("Listing files\u2028second"), "Listing files");
  assert.equal(clampPhrase("Listing files\u0085second"), "Listing files");
  const node = described("run_command(ls)", "Listing files\nrm -rf /");
  assert.equal(node.querySelector(".activity-brief")?.textContent, "Listing files");
});

test("control, invisible and bidirectional characters are gone from the phrase", () => {
  assert.equal(clampPhrase("\u202EListing\u0000 files\u200B\u2066 here\u001B\uFEFF"), "Listing files here");
  const node = described("run_command(ls)", "\u202Eexited 0 sl\u202C");
  assert.equal(node.querySelector(".activity-brief")?.textContent, "exited 0 sl");
});

test("whitespace collapses and wrapping quotes and marks are trimmed", () => {
  assert.equal(clampPhrase('  "Listing   files"  '), "Listing files");
  assert.equal(clampPhrase("**Listing files**"), "Listing files");
  assert.equal(clampPhrase("`Listing files`"), "Listing files");
  assert.equal(clampPhrase("# Listing files"), "Listing files");
  assert.equal(clampPhrase("Listing 'web/src' files"), "Listing 'web/src' files");
});

test("a phrase over the cap ends in an ellipsis", () => {
  const phrase = clampPhrase("x".repeat(300));
  assert.equal(phrase?.length, BRIEF_MAX);
  assert.ok(phrase?.endsWith("…"));
  const node = described("run_command(ls)", "y".repeat(300));
  const drawn = node.querySelector(".activity-brief")?.textContent ?? "";
  assert.equal(drawn.length, BRIEF_MAX);
  assert.ok(drawn.endsWith("…"));
});

test("a call without a command ignores the phrase and renders as before", () => {
  const node = described("write_file", "Writing the report", "created a.html (12 lines)");
  assert.equal(node.tagName, "DIV");
  assert.equal(node.querySelectorAll(".activity-brief, details, pre").length, 0);
  assert.equal(node.querySelector(".activity-name")?.textContent, "write_file");
  assert.equal(node.textContent, "●write_file");
  assert.equal(node.title, "created a.html (12 lines)");
});

/* a reopened session */

/**
 * `GET /api/sessions/:id` replays a stored tool call as a `tool` entry, and
 * `openSession` hands it to `toolLine` exactly as it hands the live event:
 * the name, the status, and the phrase when the model gave one. The entry is
 * the `tool` member of the `Message` union in `src/api.ts`, so a change to
 * that shape fails to type check here.
 */
function stored(entry: Extract<Message, { role: "tool" }>): HTMLElement {
  const node = toolLine(doc as unknown as Document, entry.name, entry.summary, entry.phrase);
  doc.body.append(node);
  return node;
}

test("a stored call with a phrase draws the model's words, with the command under the line", () => {
  const node = stored({
    role: "tool",
    name: "run_command(ls web/src)",
    summary: "exited 0",
    phrase: "Listing files in web/src",
  });
  const brief = node.querySelector(".activity-brief") as HTMLElement;
  assert.equal(brief.textContent, "Listing files in web/src");
  assert.ok(brief.classList.contains("activity-brief-model"));
  assert.equal(brief.title, MODEL_TITLE);
  assert.equal(lineOf(node).title, "exited 0. Click to see the command that ran.");
  assert.deepEqual(states(node), ["activity-ok"], "a replayed line is never in progress");
  assert.equal(node.querySelector(".activity-full code")?.textContent, "ls web/src");
});

test("a stored call without a phrase gets the phrase computed from its command, and no status when none was derived", () => {
  const node = stored({ role: "tool", name: "run_command(ls web/src)", summary: "" });
  const brief = node.querySelector(".activity-brief") as HTMLElement;
  assert.equal(brief.textContent, "Listing files in web/src");
  assert.ok(!brief.classList.contains("activity-brief-model"));
  assert.equal(node.querySelectorAll(".activity-status, .activity-result").length, 0);
  assert.equal(lineOf(node).title, "Show the full command");
  assert.deepEqual(states(node), ["activity-ok"]);
  assert.equal(node.querySelector(".activity-full code")?.textContent, "ls web/src");
});
