---
status: accepted
date: 2026-11-02
---

# the vector library is deleted, because the retrieval future chose SQLite

**Decision:** delete the `library` feature and everything it carried: the
LanceDB store (`zorp-track/src/library.rs`), the embedding client
(`zorp-agent/src/embed.rs`) and its re-exports, the `#[cfg(feature =
"library")]` embed-and-insert step in `validate`, the `library` feature on
both `zorp-track` and `zorp-agent`, the `lancedb` and `tokio` optional
dependencies on `zorp-track`, the `ZORP_EMBEDDING_MODEL` environment
variable, and the `research-library` CI job. `validate` still records its
citations to the DuckDB `validations` row exactly as before; nothing else
changes at run time.

**Why:** this reverses 0022 (2026-08-14), which moved the library behind a
feature rather than deleting it "because a retrieval story is a plausible
future." That future arrived, and it did not use this. When recall and
memory needed a vector store (0062, 2026-08-22) they chose SQLite over the
LanceDB library in `zorp-track`, by name. So the one reason 0022 kept it has
already been answered somewhere else. It remained a write-only sink:
`validate` wrote cited sources into it, nothing read them back, and the
citations `co-write` uses come from DuckDB. 0122 (2026-09-11) then spent a
whole CI runner keeping the arrow tree compiling for that unread path. A
feature nothing reads, whose kept-for future went elsewhere, is cost without
return.

**What this rules out:** carrying an unread write path and a second heavy
dependency tree on the chance a reader appears. If a retrieval story does
come back to `zorp-track`, `git revert` restores this in one commit, and
the SQLite choice in recall and memory is the precedent for how such a
store would actually be built.

**Supersedes** 0022 and 0122.
