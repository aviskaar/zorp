---
status: accepted
date: 2026-08-30
---

# the model listing carries the key, and a candidate key travels in a body

**Decision:** `GET/POST /api/settings/models` sends an API key to the
upstream as a bearer token: a candidate key from a POST body when the
panel sends one, else the stored key, else nothing. The settings panel
gains an oMLX preset (`http://localhost:8000/v1`, OpenAI wire format,
key field shown) and passes the key typed in the form along with both
the model listing and the connection test, so a protected endpoint can
be listed and tested before the first save. A candidate key travels
only in a request body, never a query string, because a secret in a
query string is a secret in URLs.

**Why:** oMLX is OpenAI-compatible but locally protected with
`--api-key`, and the listing used to go out anonymous, so a running,
correctly configured server answered its `/models` with a 401 and the
panel read as unable to connect. The connection test already sent the
key (2026-08-17 area); the listing was the one probe left
authenticating differently from the thing it probes.

**Ruled out:** a key in the query string of the existing GET (logs and
URLs), and a separate header-injection setting per provider; the wire
format is still just OpenAI-compatible plus bearer auth, which is the
same shape the real turn already sends.
