---
status: accepted
date: 2026-10-03
---

# a state-changing request another page sent is refused before any handler

**Decision:** `auth::refuse_cross_site` runs on every route, inside the
CORS layer. It passes GET, HEAD and OPTIONS. Any other method passes only
if `Sec-Fetch-Site` is `same-origin` or `none`, or, from a browser that
does not send that header, if `Origin` matches `Host`, or if `Origin` is
named with `--allow-origin`. A request with neither header passes,
because it did not come from a page: curl, the CLI and the Mac app.
Everything else gets 403 before a handler runs. First slice of #235.

**Why CORS was not enough.** The 2026-08-20 entry stopped a foreign page
reading this server's answers. A browser still sends a "simple" request,
a POST with a `text/plain` body or none, and the loopback install has no
token. A handler that takes no JSON body ran for any page the person
visited. The ones that take JSON were safe only because their extractor
refuses the content type, which is not a rule anybody wrote down.

**Why `Sec-Fetch-Site` and not `Origin` against `Host`.** The container's
nginx rewrites `Host` to the upstream and attaches the token for anyone
who reaches it, so neither `Host` nor the token can tell the UI from
another page there. A page cannot set `Sec-Fetch-Site`, and the UI's own
requests read `same-origin` through the proxy.

**Another loopback port is refused.** It reads `same-site`, because a site
ignores ports. A dev server the agent started is another program, and
#235 is about to put one in a frame next to this page.

**Ruled out:** a token for the loopback install, which is the 2026-08-20
entry's "larger change" and still is; checking `Origin` against `Host`
alone (breaks the container UI); and guarding only the routes found to
take no body, which is the per-handler accident this replaces.
