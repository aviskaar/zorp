/**
 * Tests for the conversation id in the address bar.
 *
 * The bug these come from: selecting a conversation changed the page and
 * not the URL, so a reload went back to a new chat and the conversation
 * you were reading was two clicks away again. The fix is to keep the id
 * in the query string, which means the URL is now an input, and an input
 * is something to validate rather than trust.
 */
import { strict as assert } from "node:assert";
import { test } from "node:test";
import { SESSION_PARAM, sessionFromSearch, searchForSession } from "../src/session-url.ts";

test("a session id in the query string is found", () => {
  assert.equal(sessionFromSearch("?session=18cdefd3142f21b0-9dc2"), "18cdefd3142f21b0-9dc2");
});

test("no session in the query string is null, not an empty string", () => {
  assert.equal(sessionFromSearch(""), null);
  assert.equal(sessionFromSearch("?"), null);
  assert.equal(sessionFromSearch("?other=1"), null);
});

test("an empty or whitespace session parameter is null", () => {
  assert.equal(sessionFromSearch("?session="), null);
  assert.equal(sessionFromSearch("?session=%20%20"), null);
});

test("the session parameter is found next to other parameters", () => {
  assert.equal(sessionFromSearch("?token=abc&session=s1"), "s1");
  assert.equal(sessionFromSearch("?session=s1&token=abc"), "s1");
});

/**
 * The id is taken from the address bar, so it is whatever someone typed
 * or linked. It goes on to build a request path, and while `api.ts`
 * encodes the segment, a value that cannot be a real id should not
 * produce a request at all. Anything outside the shape the server issues
 * is refused here rather than sent and denied.
 */
test("an id that is not the shape the server issues is refused", () => {
  assert.equal(sessionFromSearch("?session=../../etc/passwd"), null);
  assert.equal(sessionFromSearch("?session=" + encodeURIComponent("../secrets")), null);
  assert.equal(sessionFromSearch("?session=" + encodeURIComponent("a/b")), null);
  assert.equal(sessionFromSearch("?session=" + encodeURIComponent("<script>")), null);
  assert.equal(sessionFromSearch("?session=" + encodeURIComponent("a b")), null);
});

test("a very long id is refused rather than sent", () => {
  assert.equal(sessionFromSearch("?session=" + "a".repeat(200)), null);
});

test("the ids the server actually issues are accepted", () => {
  for (const id of ["18cdefd3142f21b0-9dc2", "abc123", "a-b_c", "A1"]) {
    assert.equal(sessionFromSearch(`?session=${id}`), id, `rejected a real id: ${id}`);
  }
});

test("selecting a session writes it into the query string", () => {
  assert.equal(searchForSession("", "s1"), "?session=s1");
  assert.equal(searchForSession("?session=old", "s1"), "?session=s1");
});

/**
 * Other parameters are somebody else's. `token` in particular is how a
 * non-loopback server is reached at all, so dropping it on a click would
 * log the page out.
 */
test("other query parameters survive a session change", () => {
  assert.equal(searchForSession("?token=abc", "s1"), "?token=abc&session=s1");
  assert.equal(searchForSession("?token=abc&session=old", "s1"), "?token=abc&session=s1");
});

test("starting a new chat clears the session but keeps the rest", () => {
  assert.equal(searchForSession("?session=s1", null), "");
  assert.equal(searchForSession("?token=abc&session=s1", null), "?token=abc");
});

test("the parameter name is one constant, not spelled twice", () => {
  assert.equal(SESSION_PARAM, "session");
  assert.equal(searchForSession("", "s1"), `?${SESSION_PARAM}=s1`);
});
