/**
 * Tests for the workspace picker.
 *
 * Three things matter here and none of them is cosmetic.
 *
 * A directory name is somebody else's string. A filesystem will hold a
 * directory called `<img src=x onerror=...>` quite happily, and the picker
 * lists whatever the browse endpoint hands it, so the injection case comes
 * first for the reason `markdown.test.ts` gives.
 *
 * A refusal has to be readable and survivable. The server answers a bad
 * path with a sentence written for a person; showing anything else, or
 * closing the picker on a refusal, leaves them with no way to try again.
 *
 * And a turn refused for want of a workspace must not read as a busy
 * session. Both are 409 and only the body tells them apart, so the api
 * layer is pinned here as well as the picker.
 */

import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { JSDOM } from "jsdom";

import { ApiError, NO_WORKSPACE, TurnBusyError, sendTurn, type Workspace } from "../src/api.ts";
import {
  WORKSPACE_REASON,
  WorkspacePicker,
  lastSegment,
  needsWorkspace,
  scratchLine,
  workspaceBar,
} from "../src/workspace.ts";

interface Call {
  method: string;
  url: string;
  body: unknown;
}

const realFetch = globalThis.fetch;

/** Replace `fetch` for one test and record what went through it. */
function stubFetch(reply: (url: string, call: Call) => Response): Call[] {
  const calls: Call[] = [];
  globalThis.fetch = (async (input: unknown, init: RequestInit = {}) => {
    const url = String(input);
    const call: Call = {
      method: init.method ?? "GET",
      url,
      body: init.body === undefined ? undefined : JSON.parse(String(init.body)),
    };
    calls.push(call);
    return reply(url, call);
  }) as typeof globalThis.fetch;
  return calls;
}

test.afterEach(() => {
  globalThis.fetch = realFetch;
});

/**
 * Let a click's request run to the end.
 *
 * A click handler starts a fetch and returns, so the assertions have to
 * wait for a real turn of the loop rather than for a microtask: reading a
 * response body is more than one tick away.
 */
function settle(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

function json(value: unknown): Response {
  return new Response(JSON.stringify(value), { status: 200 });
}

function workspace(over: Partial<Workspace> = {}): Workspace {
  return {
    path: "/home/me/work",
    scratch: "/home/me/work/scratch",
    source: "saved",
    configured: true,
    ...over,
  };
}

const HOME = {
  path: "/home/me",
  parent: "/home",
  entries: [
    { name: "work", path: "/home/me/work" },
    { name: "<img src=x onerror=alert(1)>", path: "/home/me/odd" },
  ],
};

/** A picker in a fresh document, plus whatever it was told was saved. */
function page(): { picker: WorkspacePicker; host: HTMLElement; saved: Workspace[] } {
  const dom = new JSDOM("<!doctype html><div id='host'></div>");
  const doc = dom.window.document;
  const host = doc.getElementById("host") as HTMLElement;
  const saved: Workspace[] = [];
  const picker = new WorkspacePicker(doc as unknown as Document, host, (value) =>
    saved.push(value),
  );
  return { picker, host, saved };
}

function rows(host: HTMLElement): HTMLElement[] {
  return Array.from(host.querySelectorAll(".ws-row")) as HTMLElement[];
}

function here(host: HTMLElement): string {
  return host.querySelector(".ws-here")?.textContent ?? "";
}

function result(host: HTMLElement): string {
  return host.querySelector(".ws-result")?.textContent ?? "";
}

/** The listing endpoint answering for the home directory and one child. */
function browseServer(): Call[] {
  return stubFetch((url) => {
    if (url.includes("/api/workspace/browse")) {
      if (url.includes("path=")) {
        const wanted = decodeURIComponent(url.split("path=")[1]);
        if (wanted === "/home/me/work") {
          return json({
            path: "/home/me/work",
            parent: "/home/me",
            entries: [{ name: "notes", path: "/home/me/work/notes" }],
          });
        }
        if (wanted === "/home") {
          return json({ path: "/home", parent: null, entries: [] });
        }
      }
      return json(HOME);
    }
    if (url.includes("/api/workspace")) {
      return json(workspace({ path: null, scratch: null, source: "none", configured: false }));
    }
    throw new Error(`unexpected request to ${url}`);
  });
}

test("the picker lists what the browse endpoint returned", async () => {
  browseServer();
  const { picker, host } = page();
  await picker.open();

  assert.equal(here(host), "/home/me");
  const labels = rows(host).map((row) => row.textContent);
  assert.deepEqual(labels, ["Parent directory", "work", "<img src=x onerror=alert(1)>"]);
});

test("a directory named as markup lands as text and not as an element", async () => {
  browseServer();
  const { picker, host } = page();
  await picker.open();

  assert.equal(host.querySelector("img"), null);
  const odd = rows(host).find((row) => row.textContent?.startsWith("<img"));
  assert.ok(odd, "the odd directory is listed");
  assert.equal(odd.children.length, 0, "it has no child elements, so nothing was parsed");
  assert.equal(odd.title, "/home/me/odd");
});

test("clicking a directory navigates into it", async () => {
  const calls = browseServer();
  const { picker, host } = page();
  await picker.open();

  const work = rows(host).find((row) => row.textContent === "work");
  assert.ok(work);
  work.click();
  await settle();

  assert.equal(here(host), "/home/me/work");
  assert.deepEqual(
    rows(host).map((row) => row.textContent),
    ["Parent directory", "notes"],
  );
  assert.ok(calls.some((call) => call.url.includes(encodeURIComponent("/home/me/work"))));
});

test("clicking the parent row goes up", async () => {
  browseServer();
  const { picker, host } = page();
  await picker.open();

  const up = host.querySelector(".ws-up") as HTMLElement;
  assert.equal(up.title, "/home");
  up.click();
  await settle();

  assert.equal(here(host), "/home");
  // No parent row at the top of the tree, so nothing offers to go nowhere.
  assert.equal(host.querySelector(".ws-up"), null);
});

test("saving sends PUT with the shown path", async () => {
  const calls = stubFetch((url) => {
    if (url.includes("/api/workspace/browse")) {
      return json(HOME);
    }
    return json(workspace({ path: "/home/me" }));
  });
  const { picker, host, saved } = page();
  await picker.open();

  (host.querySelector(".ws-save") as HTMLElement).click();
  await settle();

  const put = calls.find((call) => call.method === "PUT");
  assert.ok(put, "a PUT went out");
  assert.ok(put.url.endsWith("/api/workspace"));
  assert.deepEqual(put.body, { path: "/home/me" });
  assert.equal(saved.length, 1);
  assert.equal(saved[0].path, "/home/me");
});

test("a typed path is what gets saved", async () => {
  const calls = stubFetch((url) => {
    if (url.includes("/api/workspace/browse")) {
      return json(HOME);
    }
    return json(workspace({ path: "/elsewhere" }));
  });
  const { picker, host } = page();
  await picker.open();

  const field = host.querySelector("input") as HTMLInputElement;
  field.value = "/elsewhere";
  field.dispatchEvent(new (field.ownerDocument.defaultView as Window).Event("input"));
  assert.equal(here(host), "/elsewhere", "the header follows the field");

  (host.querySelector(".ws-save") as HTMLElement).click();
  await settle();

  const put = calls.find((call) => call.method === "PUT");
  assert.deepEqual(put?.body, { path: "/elsewhere" });
});

test("a refused path shows the server's sentence and leaves the picker open", async () => {
  const refusal = "that path is a file, not a directory";
  stubFetch((url) => {
    if (url.includes("/api/workspace/browse")) {
      return json(HOME);
    }
    if (url.includes("/api/workspace")) {
      return new Response(refusal, { status: 400 });
    }
    throw new Error(`unexpected request to ${url}`);
  });
  const { picker, host, saved } = page();
  await picker.open();

  (host.querySelector(".ws-save") as HTMLElement).click();
  await settle();

  assert.equal(result(host), refusal, "the sentence is shown as the server wrote it");
  assert.equal(saved.length, 0, "nothing was reported as saved");
  assert.ok(rows(host).length > 0, "the listing is still there to try again from");
  assert.equal((host.querySelector(".ws-save") as HTMLButtonElement).disabled, false);
});

test("a browse that fails keeps the listing that is up", async () => {
  let first = true;
  stubFetch((url) => {
    if (url.includes("/api/workspace/browse")) {
      if (first) {
        first = false;
        return json(HOME);
      }
      return new Response("no such directory", { status: 404 });
    }
    return json(workspace({ path: null, configured: false }));
  });
  const { picker, host } = page();
  await picker.open();

  const work = rows(host).find((row) => row.textContent === "work") as HTMLElement;
  work.click();
  await settle();

  assert.equal(result(host), "no such directory");
  assert.deepEqual(
    rows(host).map((row) => row.textContent),
    ["Parent directory", "work", "<img src=x onerror=alert(1)>"],
  );
});

test("the picker opens with a sentence saying why, when it opened itself", async () => {
  browseServer();
  const { picker, host } = page();
  await picker.open(WORKSPACE_REASON);

  const reason = host.querySelector(".ws-reason") as HTMLElement;
  assert.equal(reason.hidden, false);
  assert.equal(reason.textContent, WORKSPACE_REASON);
});

test("the picker opened by hand explains nothing it does not have to", async () => {
  browseServer();
  const { picker, host } = page();
  await picker.open();

  assert.equal((host.querySelector(".ws-reason") as HTMLElement).hidden, true);
});

test("a turn refused for want of a workspace opens the picker", async () => {
  // What the error path in main.ts asks, and the answer that routes the
  // message to the picker rather than to an error card.
  assert.equal(needsWorkspace("no workspace chosen"), true);
  assert.equal(needsWorkspace("no workspace chosen (HTTP 409)"), true);
  assert.equal(needsWorkspace("a turn is already running on this session"), false);
  assert.equal(needsWorkspace("the model endpoint refused the request"), false);
});

test("a 409 for want of a workspace is not dressed up as a busy session", async () => {
  stubFetch(() => new Response(NO_WORKSPACE, { status: 409 }));
  await assert.rejects(sendTurn("s1", "hello"), (error: unknown) => {
    assert.ok(error instanceof ApiError);
    assert.ok(!(error instanceof TurnBusyError), "the body is what tells the two 409s apart");
    assert.equal((error as ApiError).message, NO_WORKSPACE);
    return true;
  });
});

test("a 409 for a running turn is still a busy session", async () => {
  stubFetch(() => new Response("a turn is already running", { status: 409 }));
  await assert.rejects(sendTurn("s1", "hello"), (error: unknown) => {
    assert.ok(error instanceof TurnBusyError);
    return true;
  });
});

test("the picker names the scratch directory the server reported", () => {
  assert.equal(
    scratchLine(workspace()),
    "Files the agent generates, PDFs included, go in /home/me/work/scratch.",
  );
  // Nothing is joined together here. With no answer from the server there is
  // no path to name, and the sentence says so without inventing one.
  assert.match(scratchLine(null), /a scratch directory inside the workspace\.$/);
  assert.match(scratchLine(workspace({ scratch: null })), /a scratch directory/);
});

test("the top bar shows the last segment with the full path in the title", () => {
  const bar = workspaceBar(workspace({ path: "/home/me/deep/nested/project" }));
  assert.equal(bar.label, "project");
  assert.equal(bar.title, "/home/me/deep/nested/project");
  assert.equal(bar.set, true);
});

test("the top bar reads No workspace when none is set", () => {
  for (const value of [null, workspace({ path: null, configured: false })]) {
    const bar = workspaceBar(value);
    assert.equal(bar.label, "No workspace");
    assert.equal(bar.set, false, "false is what makes the pill loud");
    assert.match(bar.title, /nowhere to work/);
  }
});

test("last segment handles the awkward paths", () => {
  assert.equal(lastSegment("/"), "/");
  assert.equal(lastSegment("/home"), "home");
  assert.equal(lastSegment("/home/me/work/"), "work");
  assert.equal(lastSegment("/home/me/work///"), "work");
});

test("the top bar pill borrows the model button's truncation", () => {
  // The label is a whole path segment and is never cut in code, so the bar
  // stays narrow only if the CSS says so.
  const css = readFileSync(new URL("../styles.css", import.meta.url), "utf8");
  const block = css.match(/\n\.model-btn\s*\{([^}]*)\}/)?.[1] ?? "";
  const span = css.match(/\n\.model-btn span\s*\{([^}]*)\}/)?.[1] ?? "";
  assert.match(block, /max-width/);
  assert.match(span, /text-overflow:\s*ellipsis/);
  assert.match(span, /white-space:\s*nowrap/);

  const html = readFileSync(new URL("../index.html", import.meta.url), "utf8");
  assert.ok(html.includes('class="model-btn workspace-btn"'));
  assert.ok(html.includes('id="workspace-btn"'));
  assert.ok(html.includes('id="workspace-btn-label"'));
});
