/**
 * Tests for the folder view of the workspace listing.
 *
 * The injection case comes first, as in `activity-line.test.ts`: a folder
 * name is part of a path a model wrote, so it reaches the page the same way
 * every other model-derived string does, through `textContent`.
 *
 * The rest pins the grouping itself, which is the part a reader depends on:
 * that the order the server chose survives, that a folder says how much is
 * inside it, and that a folder is shut unless it holds something worth
 * revealing.
 */

import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";

import { artifactTree, countLabel, groupByFolder, leafName } from "../src/artifact-tree.ts";

const dom = new JSDOM("<!doctype html><body></body>");
const doc = dom.window.document as unknown as Document;

interface File {
  path: string;
}

function files(...paths: string[]): File[] {
  return paths.map((path) => ({ path }));
}

/** The row `main.ts` supplies, reduced to the part these tests read. */
function row(file: File, label: string): HTMLElement {
  const button = doc.createElement("button");
  button.className = "artifact-item";
  button.dataset.path = file.path;
  button.textContent = label;
  return button;
}

function build(list: File[], reveal?: Set<string>): HTMLElement {
  const holder = doc.createElement("ul");
  holder.append(...artifactTree(doc, list, { row, reveal }));
  return holder;
}

test("a folder name reaches the page as text and never as markup", () => {
  const holder = build(files("<img src=x onerror=alert(1)>/a.txt"));
  const name = holder.querySelector(".artifact-folder-name");
  assert.ok(name);
  assert.equal(name?.textContent, "<img src=x onerror=alert(1)>");
  assert.equal(holder.querySelectorAll("img").length, 0);
});

test("a leaf name is everything after the last separator", () => {
  assert.equal(leafName("a/b/c.txt"), "c.txt");
  assert.equal(leafName("c.txt"), "c.txt");
  assert.equal(leafName("a/b/"), "b");
});

test("files at the root stay at the root", () => {
  const root = groupByFolder(files("a.txt", "b.txt"));
  assert.equal(root.folders.length, 0);
  assert.deepEqual(
    root.files.map((f) => f.path),
    ["a.txt", "b.txt"],
  );
});

test("a shared prefix becomes one folder holding the rest", () => {
  const root = groupByFolder(files("out/a.png", "out/b.png", "out/deep/c.png"));
  assert.equal(root.folders.length, 1);
  const out = root.folders[0];
  assert.equal(out.name, "out");
  assert.equal(out.path, "out");
  assert.equal(out.count, 3, "a folder counts everything at or under it");
  assert.deepEqual(
    out.files.map((f) => f.path),
    ["out/a.png", "out/b.png"],
  );
  assert.equal(out.folders[0].name, "deep");
  assert.equal(out.folders[0].path, "out/deep");
  assert.equal(out.folders[0].count, 1);
});

test("the order the server chose survives the grouping", () => {
  // The server sorts by path and this does not re-sort, so a caller who
  // changes the server's mind about ordering changes only the server.
  const root = groupByFolder(files("z/2.txt", "z/1.txt", "z/3.txt"));
  assert.deepEqual(
    root.folders[0].files.map((f) => f.path),
    ["z/2.txt", "z/1.txt", "z/3.txt"],
  );
});

test("two spellings of one directory are one folder", () => {
  const root = groupByFolder(files("./out/a.txt", "out/b.txt", "/out/c.txt"));
  assert.equal(root.folders.length, 1);
  assert.equal(root.folders[0].count, 3);
});

test("a path that names no file is dropped rather than given a folder", () => {
  const root = groupByFolder(files("///", ""));
  assert.equal(root.folders.length, 0);
  assert.equal(root.files.length, 0);
});

test("the summary says how much is inside", () => {
  assert.equal(countLabel(1), "1 file");
  assert.equal(countLabel(2), "2 files");
  const holder = build(files("out/a.png", "out/b.png"));
  assert.equal(holder.querySelector(".artifact-folder-count")?.textContent, "2 files");
});

test("folders come before the files beside them", () => {
  const holder = build(files("a.txt", "out/b.png"));
  const kinds = Array.from(holder.children).map((li) =>
    li.querySelector(".artifact-folder") ? "folder" : "file",
  );
  assert.deepEqual(kinds, ["folder", "file"]);
});

test("the top level is open and the level under it is not", () => {
  // A workspace with one directory would otherwise open to a single row
  // naming that directory, and the folders holding hundreds of files are
  // one step down, which is the step worth folding.
  const holder = build(files("out/deep/c.png", "out/b.png"));
  const top = holder.querySelector<HTMLElement>("details");
  assert.equal(top?.hasAttribute("open"), true);
  const deep = holder.querySelectorAll<HTMLElement>("details")[1];
  assert.equal(deep.hasAttribute("open"), false);
});

test("a folder below the top is shut unless it holds something worth revealing", () => {
  const shut = build(files("out/deep/a.png", "out/deep/b.png"));
  assert.equal(shut.querySelectorAll("details")[1].hasAttribute("open"), false);

  const open = build(files("out/deep/a.png", "out/deep/b.png"), new Set(["out/deep/b.png"]));
  assert.equal(open.querySelectorAll("details")[1].hasAttribute("open"), true);
});

test("revealing a nested file opens every folder above it and nothing else", () => {
  const holder = build(
    files("out/deep/c.png", "out/other/d.png", "elsewhere/e.png"),
    new Set(["out/deep/c.png"]),
  );
  const boxes = Array.from(holder.querySelectorAll<HTMLElement>("details"));
  const open = boxes.filter((box) => box.hasAttribute("open"));
  // `out` and `elsewhere` are open for being at the top, `deep` for holding
  // the revealed file. `other` is the one that stays shut.
  assert.equal(boxes.length, 4);
  assert.equal(open.length, 3);
  const shut = boxes.filter((box) => !box.hasAttribute("open"));
  assert.equal(shut[0].querySelector(".artifact-folder-name")?.textContent, "other");
});

test("a row still carries the whole path, and shows only the leaf", () => {
  const holder = build(files("out/deep/c.png"), new Set(["out/deep/c.png"]));
  const button = holder.querySelector<HTMLElement>(".artifact-item");
  assert.equal(button?.dataset.path, "out/deep/c.png");
  assert.equal(button?.textContent, "c.png");
});
