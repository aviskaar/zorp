/**
 * Tests for the tool call line.
 *
 * The injection cases come first, for the reason `markdown.test.ts` gives:
 * the command on the line is model output, and a line that turned any of it
 * into markup would be a cross-site scripting hole. The phrase is derived
 * from the same text and gets the same treatment.
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

import { BRIEF_MAX, callLine, describeCommand, splitCall, toolLine } from "../src/activity-line.ts";

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

test("a status with markup in it is text", () => {
  const node = line("run_command(ls)", "<b>exited 0</b>");
  assert.equal(node.querySelectorAll("b").length, 0);
  assert.equal(node.querySelector(".activity-status")?.textContent, "<b>exited 0</b>");
});

/* structure */

test("the phrase and the status are separate spans in one text column, with no tool name", () => {
  const node = line("run_command(pandoc in.html -o out.pdf)");
  const column = node.querySelector(".activity-line > .activity-text");
  assert.ok(column, "the text sits in one column beside the bullet");
  assert.equal(column?.querySelectorAll(".activity-name").length, 0);
  assert.equal(column?.querySelector(".activity-brief")?.textContent, "Converting in.html");
  assert.equal(column?.querySelectorAll(".activity-status").length, 1);
  assert.equal(column?.querySelector(".activity-status")?.textContent, "exited 0");
  assert.equal(node.textContent, "●Converting in.html exited 0pandoc in.html -o out.pdf");
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
  assert.equal((node.firstElementChild as HTMLElement).title, "Show the full command");
  assert.equal(node.querySelector("pre.activity-full code")?.textContent, "pandoc in.html -o out.pdf");
});

test("a call without a command is a plain line that keeps its tool name", () => {
  const node = line("write_file", "created a.html (12 lines)");
  assert.equal(node.tagName, "DIV");
  assert.equal(node.querySelectorAll("details, pre").length, 0);
  assert.equal(node.querySelector(".activity-name")?.textContent, "write_file");
  assert.equal(node.querySelector(".activity-status")?.textContent, "created a.html (12 lines)");
});

test("an empty status puts no empty span on the page", () => {
  const node = line("read_file", "");
  assert.equal(node.querySelectorAll(".activity-status").length, 0);
  assert.equal(node.querySelectorAll("pre").length, 0);
  assert.equal(node.textContent, "●read_file");
});

test("a verify line keeps its name and carries its verdict class on the status", () => {
  const node = callLine(doc as unknown as Document, "verify", "cargo test", "failed", "activity-fail");
  assert.equal(node.querySelector(".activity-name")?.textContent, "verify");
  assert.equal(node.querySelector(".activity-brief")?.textContent, "Running tests");
  const status = node.querySelector(".activity-status");
  assert.ok(status?.classList.contains("activity-fail"));
  assert.equal(status?.textContent, "failed");
  assert.equal(node.querySelector(".activity-full code")?.textContent, "cargo test");
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
