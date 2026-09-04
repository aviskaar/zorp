/**
 * Tests for the tool call line.
 *
 * The injection cases come first, for the reason `markdown.test.ts` gives:
 * the command on the line is model output, and a line that turned any of it
 * into markup would be a cross-site scripting hole. The brief is derived
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

import { BRIEF_MAX, briefCommand, callLine, splitCall, toolLine } from "../src/activity-line.ts";

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

test("a script tag in the command is text on the line and in the details", () => {
  const command = 'echo "<script>alert(1)</script>" > out.html';
  const node = line(`run_command(${command})`);
  assert.equal(node.querySelectorAll("script").length, 0);
  assert.equal(node.querySelector(".activity-full code")?.textContent, command);
  assert.ok(node.querySelector(".activity-brief")?.textContent?.includes("<script>"));
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

test("the name, the brief and the status are separate spans in one text column", () => {
  const node = line("run_command(pandoc in.html -o out.pdf)");
  const column = node.querySelector(".activity-line > .activity-text");
  assert.ok(column, "the text sits in one column beside the bullet");
  assert.equal(column?.querySelector(".activity-name")?.textContent, "run_command");
  assert.equal(column?.querySelector(".activity-brief")?.textContent, "pandoc in.html out.pdf …");
  assert.equal(column?.querySelectorAll(".activity-status").length, 1);
  assert.equal(column?.querySelector(".activity-status")?.textContent, "exited 0");
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
  assert.equal(node.querySelector("pre.activity-full code")?.textContent, "pandoc in.html -o out.pdf");
});

test("a call without a command is a plain line with no container under it", () => {
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
  assert.equal(node.textContent?.trim(), "●read_file");
});

test("a verify line carries its verdict class on the status", () => {
  const node = callLine(doc as unknown as Document, "verify", "cargo test", "failed", "activity-fail");
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

/* the brief */

test("a leading cd is dropped", () => {
  assert.equal(briefCommand("cd /repo && cargo build"), "cargo build");
  assert.equal(briefCommand("cd '/my repo'; cd web && npm test"), "npm test");
});

test("the program and its first positional arguments survive, the rest is elided", () => {
  assert.equal(briefCommand("pip3 install weasyprint"), "pip3 install weasyprint");
  assert.equal(
    briefCommand("pandoc k8s.html -o k8s.pdf --pdf-engine=weasyprint 2>&1 || pandoc k8s.html"),
    "pandoc k8s.html k8s.pdf …",
  );
  assert.equal(briefCommand("ls -lh a.pdf && file a.pdf"), "ls a.pdf …");
  assert.equal(briefCommand("git add a b c d e"), "git add a b …");
});

test("redirections and environment assignments are not what the command does", () => {
  assert.equal(briefCommand("FOO=1 make > build.log 2>&1"), "make …");
  assert.equal(briefCommand("cat <<'EOF' > notes.txt\nhello\nEOF"), "cat …");
});

test("a quoted argument stays one argument", () => {
  assert.equal(briefCommand('git commit -m "fix: wrap lines"'), 'git commit "fix: wrap lines" …');
});

test("the brief is capped", () => {
  const long = `python3 -c "${"x".repeat(200)}"`;
  const brief = briefCommand(long);
  assert.equal(brief.length, BRIEF_MAX);
  assert.ok(brief.endsWith("…"));
});

test("an empty command is an empty brief", () => {
  assert.equal(briefCommand(""), "");
  assert.equal(briefCommand("   "), "");
});
