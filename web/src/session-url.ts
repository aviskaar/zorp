/**
 * The selected conversation, kept in the address bar.
 *
 * Selecting a conversation used to change the page and not the URL, so a
 * reload landed on a new chat and the conversation you were reading was
 * two clicks away again. Anything you would reasonably bookmark, send to
 * yourself, or get back to with the back button was unreachable.
 *
 * A query parameter rather than a path segment, because a path would need
 * the server to serve `index.html` for `/s/<id>` and a reload on an
 * unknown path is exactly the bug being fixed. A query parameter rides
 * along on the existing static route and needs no server change at all.
 *
 * Its own module because the interesting part is that the URL is now an
 * *input*. Everything else in the page gets its session id from the
 * server; this one comes from whatever is in the address bar, so it is
 * parsed and refused here rather than trusted.
 */

export const SESSION_PARAM = "session";

/**
 * The shape the server issues: hex and a suffix, joined by a dash. Kept
 * deliberately narrow. `api.ts` already encodes the value into the request
 * path, so this is not the only thing standing between a hostile URL and a
 * bad request, but a value that cannot be a real id should not produce a
 * request at all.
 */
const ID_SHAPE = /^[A-Za-z0-9_-]{1,128}$/;

/**
 * The session id in a query string, or null when there is not a usable one.
 *
 * Null rather than an empty string for every failure, so a caller has one
 * thing to check. An id that is absent, blank, or not the right shape all
 * mean the same thing to the page: open a new chat.
 */
export function sessionFromSearch(search: string): string | null {
  let params: URLSearchParams;
  try {
    params = new URLSearchParams(search);
  } catch {
    return null;
  }
  const raw = params.get(SESSION_PARAM);
  if (raw === null) {
    return null;
  }
  const trimmed = raw.trim();
  if (!trimmed || !ID_SHAPE.test(trimmed)) {
    return null;
  }
  return trimmed;
}

/**
 * The query string that selects `id`, built from the one the page already
 * has. Pass null to clear the selection, which is what starting a new chat
 * does.
 *
 * Other parameters are somebody else's and survive. `token` in particular
 * is how a non-loopback server is reached at all, so dropping it when
 * someone clicks a conversation would log the page out.
 */
export function searchForSession(search: string, id: string | null): string {
  const params = new URLSearchParams(search);
  if (id === null) {
    params.delete(SESSION_PARAM);
  } else {
    params.set(SESSION_PARAM, id);
  }
  const next = params.toString();
  return next ? `?${next}` : "";
}
