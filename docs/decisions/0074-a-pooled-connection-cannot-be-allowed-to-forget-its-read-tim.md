---
status: accepted
date: 2026-08-23
---

# a pooled connection cannot be allowed to forget its read timeout

**Decision:** `zorp::http_agent` does not keep idle connections. Every model
request opens a fresh socket, and ureq arms `timeout_read` on that socket before
it waits for response headers. `ZORP_HTTP_TIMEOUT_SECS` remains the one bound.
It is still a per-read timeout, so it bounds silence and never the total length
of a streamed answer.

**Why: a second long calibration run passed the configured bound and never
returned.** The run stopped at attempt 203 of 250. Twenty-five minutes after
its last log line the process was alive at 0.0% CPU, one worker was blocked in
`__recvfrom`, and one established OpenRouter socket had received no bytes over
a five second sample. The default 900 second timeout had been exceeded by
about ten minutes. This is the same signature that first put a timeout on the
streaming path, but the binary already contained that fix.

The missing bound was in ureq 2.12.1's pool. A new connection gets
`timeout_read` in `stream.rs`. Returning it to the pool calls `Stream::reset`
and clears both socket timeouts. `pool.try_get_connection` then gives that
socket to the next request without restoring them. The response body reader
does restore `timeout_read`, which hid the bug after headers arrived. The wait
for those headers on request two and later had no bound at all.

**Why no pooling.** A model call is much longer than a TCP and TLS handshake,
and one agent attempt can make 40 calls. Paying for those handshakes is small
next to losing the whole attempt, or a thirteen hour run, to one silent reused
socket. A two-request regression server finishes the first response so its
connection can be pooled, then accepts the second request and sends no response
bytes. The test fails against the pooled agent and requires the second call to
end near a short configured read timeout.

**Ruled out: a whole-request timeout.** It would cap the length of an honest
answer. The read timeout deliberately restarts with each read so an answer may
stream for as long as it keeps talking.

**Deferred: ureq 3.x.** Its timeout implementation was rewritten and may make
pooling safe again, but that upgrade is larger than this fix and brings a new
API and dependency tree under the workspace's Rust 1.95 floor. It should be
evaluated on its own. Reapplying the timeout to a pooled ureq 2.x socket is not
available through its public API.
