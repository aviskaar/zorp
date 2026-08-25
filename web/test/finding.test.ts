/**
 * Tests for the finding marker's gate.
 *
 * This is the part of the feature that has to be right. A marker the model
 * can award itself is worth nothing, so the only question these tests ask is
 * whether a block earns its badge, and almost all of them are cases where it
 * must not.
 */

import test from "node:test";
import assert from "node:assert/strict";

import {
  MIN_CITATION_CHARS,
  MIN_SOURCES,
  parseFinding,
  verifyFinding,
  type ActivityEntry,
} from "../src/finding.ts";

const RUN: ActivityEntry[] = [
  { name: "read_file", summary: "docs/rates.md (120 lines)" },
  { name: "web_search", summary: "ons.gov.uk/inflation-2019" },
  { name: "run_command", summary: "exited 0" },
];

function block(...lines: string[]): string {
  return lines.join("\n");
}

const GOOD = block(
  "claim: the two series disagree for 2019",
  "because: the filed figure and the published one differ by 1.4 points",
  "source: docs/rates.md",
  "source: ons.gov.uk/inflation-2019",
);

/* ---------------------------------------------------------------- parsing */

test("a well formed block parses into a claim, a reason and its sources", () => {
  const finding = parseFinding(GOOD);
  assert.ok(finding, "a complete block did not parse");
  assert.equal(finding.claim, "the two series disagree for 2019");
  assert.equal(
    finding.reason,
    "the filed figure and the published one differ by 1.4 points",
  );
  assert.deepEqual(finding.sources, ["docs/rates.md", "ons.gov.uk/inflation-2019"]);
});

// A bulb with nothing behind it is decoration, so a block with no reason is
// not a finding at all.
test("a block with no reason does not parse", () => {
  assert.equal(parseFinding(block("claim: something", "source: docs/rates.md")), null);
});

test("a block with no claim does not parse", () => {
  assert.equal(parseFinding(block("because: it just is", "source: docs/rates.md")), null);
});

test("an empty claim is the same as no claim", () => {
  assert.equal(parseFinding(block("claim:   ", "because: x", "source: y")), null);
});

test("keys are recognised whatever case the model used", () => {
  const finding = parseFinding(block("Claim: c", "BECAUSE: r", "Source: docs/rates.md"));
  assert.equal(finding?.claim, "c");
  assert.equal(finding?.reason, "r");
});

test("prose that is not key and value at all does not parse", () => {
  assert.equal(parseFinding("I found something really interesting!"), null);
});

/* ----------------------------------------------------------- verification */

test("a finding whose sources are all real, distinct activity is marked", () => {
  const verified = verifyFinding(parseFinding(GOOD)!, RUN);
  assert.ok(verified, "a fully corroborated finding was refused");
  assert.equal(verified.evidence.length, 2);
  assert.deepEqual(
    verified.evidence.map((e) => e.name),
    ["read_file", "web_search"],
  );
  assert.equal(verified.claim, "the two series disagree for 2019");
});

// The whole point. A source the run never touched is the cheapest way for a
// model to manufacture significance, and it must cost the marker.
test("a source that appears in no activity kills the marker", () => {
  const finding = parseFinding(
    block(
      "claim: c",
      "because: r",
      "source: docs/rates.md",
      "source: imf.org/never-fetched",
    ),
  )!;
  assert.equal(verifyFinding(finding, RUN), null);
});

test("one fabricated source among real ones still kills the marker", () => {
  const finding = parseFinding(
    block(
      "claim: c",
      "because: r",
      "source: docs/rates.md",
      "source: ons.gov.uk/inflation-2019",
      "source: bis.org/imagined",
    ),
  )!;
  assert.equal(
    verifyFinding(finding, RUN),
    null,
    "padding two real citations with a made up one was allowed",
  );
});

test("a run that used no tools can never produce a marker", () => {
  assert.equal(verifyFinding(parseFinding(GOOD)!, []), null);
});

test("one source is not corroboration", () => {
  const finding = parseFinding(block("claim: c", "because: r", "source: docs/rates.md"))!;
  assert.equal(verifyFinding(finding, RUN), null);
});

test("no sources at all is not corroboration either", () => {
  const finding = parseFinding(block("claim: c", "because: r"))!;
  assert.equal(verifyFinding(finding, RUN), null);
});

// Two citations that land on the same tool call are one source wearing two
// hats, which is exactly the shape a model reaches for when it has only one.
test("two citations resolving to the same activity count once", () => {
  const finding = parseFinding(
    block("claim: c", "because: r", "source: docs/rates.md", "source: rates.md (120"),
  )!;
  assert.equal(verifyFinding(finding, RUN), null);
});

// A short citation matches half the run by accident, so it is not a citation.
test("a citation too short to identify anything is refused", () => {
  const short = "md".slice(0, MIN_CITATION_CHARS - 1);
  const finding = parseFinding(
    block("claim: c", "because: r", `source: ${short}`, "source: ons.gov.uk/inflation-2019"),
  )!;
  assert.equal(verifyFinding(finding, RUN), null);
});

test("matching a source ignores case but still has to match", () => {
  const finding = parseFinding(
    block("claim: c", "because: r", "source: DOCS/RATES.MD", "source: ONS.gov.uk/inflation-2019"),
  )!;
  assert.ok(verifyFinding(finding, RUN), "a case difference lost a real citation");
});

// The tool name is not a citation. "read" would resolve against every read in
// the run and turn the gate into a formality.
test("citing a tool name rather than what it touched does not resolve", () => {
  const finding = parseFinding(
    block("claim: c", "because: r", "source: read_file", "source: web_search"),
  )!;
  assert.equal(verifyFinding(finding, RUN), null);
});

test("the thresholds are the ones the design argued for", () => {
  assert.equal(MIN_SOURCES, 2);
  assert.ok(MIN_CITATION_CHARS >= 4);
});
