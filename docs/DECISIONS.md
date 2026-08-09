# Decision log

A running record of product and architecture decisions made while
building zorp. Newest entries at the top. Each entry is short: what was
decided, why, and what it ruled out. Full design writeups, when they
exist, live in `docs/superpowers/specs/` and are linked from here.

---

## 2026-08-09: validate's design: MCP-only search, two-dimension rubric, new embedding env var

**Decision:** `validate` searches through whatever search-capable MCP
servers the user has configured, no built-in search provider. Embeddings
for LanceDB come from a new `ZORP_EMBEDDING_MODEL` env var, hitting the
same `ZORP_BASE_URL` the chat model already uses. Scoring uses two
domain-agnostic dimensions, redundancy (has this already been answered)
and feasibility (can this be investigated), not Catalyst's four
academic-specific dimensions, each requiring a citation from retrieved
evidence or scoring 0.

**Why:** No built-in search provider keeps zorp vendor-neutral and
avoids owning an API key/provider zorp would have to maintain; MCP
already exists for exactly this. Reusing `ZORP_BASE_URL` for embeddings
matches how the rest of zorp is configured, one new env var instead of a
new provider abstraction. Two dimensions instead of four because
"novelty" and "prior-art distance" don't mean anything for a Kafka
migration question; redundancy and feasibility do, for any domain.

**Ruled out:** Shipping a default search provider (scope creep for a
first version, and a provider decision zorp shouldn't own). A structured
sources table (citations as free text for now; revisit only if
co-write's claim-check needs more).

**Full writeup:** `docs/superpowers/specs/2026-08-09-zorp-validate-design.md`

---

## 2026-08-09: research means investigation, not academia, zorp's scope broadens

**Decision:** zorp targets any question that can be turned into a
defensible answer using evidence, not just academic research. The
primitive: question, investigation, sources, evidence, conflicting
evidence, reasoning, validation, answer or artifact. This covers
technical decisions, product questions, competitive analysis, investment
theses, market sizing, scientific hypotheses, engineering choices, due
diligence, strategy, and ordinary high-stakes personal decisions.
Academic research is one instance of this, not the whole of it. The four
capabilities are renamed to match: validate (unchanged), experiment
becomes investigate, co-write (unchanged), find a venue becomes deliver.
Positioning drops "AI research agent" for "Zorp investigates hard
questions and delivers evidence-backed answers," with the tagline "LLMs
made intelligence cheap. Zorp makes validated intelligence cheap."

**Why:** Stated directly, with the reasoning that "research" read as
academic-paper-only undersells the product and caps the market. The
underlying architecture didn't need to change to support this: the
pre-registration discipline (commit a method and threshold before
evidence exists) was never actually academic-specific, "migrate off
Kafka if a spike test shows 20% latency improvement" is the same shape
as a scientific hypothesis. Only the language was narrow.

**Ruled out:** Rebuilding `zorp-track` or the architecture to support
this. Nothing about the foundation assumed an academic domain; broadening
the scope is a naming and product-language change, not a structural one.

**Full writeup:** `docs/superpowers/specs/2026-08-09-zorp-scope-and-positioning.md`

---

## 2026-08-09: eight decisions from an interview round on the open questions

A short interview to work through the open questions left in the
architecture proposal. Each is small enough to log together; none of
them has a full writeup beyond this entry.

**One binary, not two.** The four capabilities (validate, experiment,
co-write, find a venue) ship as new subcommands on `zorp-agent`, not a
separate `zorp-research` binary. Parallel experiment workers still run
as isolated subprocesses, but by having `zorp-agent` spawn more copies of
itself, not a second program. Keeps one thing to install and learn.

**Pre-registration is always required, not optional.** Every experiment
writes its hypothesis, metric, and a numeric kill threshold as its own
commit before any experiment code runs, the same discipline
lab-engine/Catalyst's idea triage already uses and for the same reason:
it stops the threshold from being quietly moved after seeing results.

**No hard experiment budget.** Catalyst caps experiments at 150 lines of
code, 10 minutes, no GPU. zorp ships sane default guidance but doesn't
hard-enforce it, since zorp is general-purpose and a cap tuned for
Catalyst's small validation experiments could be wrong for what zorp's
users actually run.

**Checkpoints are interactive by default.** The three research-loop
checkpoints (after validate, after experiment, before co-write finalizes)
default to asking a human, the same default `zorp-agent`'s existing
per-tool-call approval gate already uses. An explicit flag allows
unattended full-loop runs.

**Run record metrics are typed key-value pairs, not narrative logs.**
Every experiment attempt records explicit, named, typed values (for
example `accuracy: 0.87`) in DuckDB columns, alongside the free-form
logs. This is what lets the co-write claim-check compare a number the
draft cites against something structured, not an LLM's read of raw
stdout.

**Venue matching calls a live venue API, not a shipped catalog.** Confirmed
using huiban (the conference/journal database used to research zorp's own
venues earlier this session) as the model: query for current deadlines
and rankings rather than shipping and maintaining a dataset that goes
stale between releases.

**Multi-track from day one.** zorp supports multiple concurrent research
investigations from the start, closer to ORR's track model, rather than
one-at-a-time with multi-track added later. Chosen over the YAGNI
default specifically to avoid a data-model migration once real usage
exists.

**Venue matching runs on an abstract and contribution summary, not the
full paper.** Enough signal to match scope and contribution type, and it
can run as soon as co-write has a draft abstract, before the full paper
is finalized.

---

## 2026-08-09: two data stores, split by job, not one general-purpose one

**Decision:** DuckDB for the transactional and analytical record (the
run record: experiment status, stage transitions, metrics as structured
columns), LanceDB for multimodal, semantically searchable content
(literature embeddings, paper text and figures, plots, anything the
validate and find-a-venue capabilities need to search over by meaning
rather than by exact field).

**Why:** They're solving different problems. DuckDB's `duckdb-rs` is
synchronous, modeled on the same interface as `rusqlite` (which
`zorp-agent` already uses for session persistence), with full transaction
support, and it's also a real analytical engine, so aggregating metrics
across many experiment attempts is a native strength, not an afterthought
the way it would be on a plain OLTP store. LanceDB is embedded, built on
Arrow, and handles vector similarity, full-text, and multimodal data
(text, images) in one store, which is exactly what novelty checks and
venue matching need. Neither is a hard dependency on Aviskaar-private
infrastructure; both are embedded, no server, ship inside zorp itself.

**Ruled out:** One store trying to do both jobs. A vector database
forced to also be the transactional state machine, or a relational store
pressed into semantic search, would compromise on whichever job it does
second-best. This also supersedes an earlier verbal suggestion to reuse
`rusqlite` for the run record; DuckDB was chosen instead for the added
analytical capability.

**Async boundary:** LanceDB's Rust API is async (tokio). `duckdb-rs` is
synchronous. Both live above `zorp` core, same as `zorp-mcp` and the
`otel` feature already do, without touching the core's deliberately
synchronous design.

---

## 2026-08-09: zorp's own arXiv paper is about the harness, not a discovery it made

**Decision:** The paper zorp itself publishes to arXiv is a systems paper
describing zorp: its minimal-harness design, its lineage from quecto, and
its evals/benchmarks (including comparisons against heavier frameworks
like Sakana's AI-Scientist-v2 and Aviskaar's own lab-engine/Catalyst
pipeline). It is not a scientific-discovery paper produced by using zorp
to research some unrelated topic.

**Why:** Stated directly. This scopes `docs/paper/` and keeps it separate
from the product itself: what zorp offers users (validate an idea, run
experiments, co-write a paper, find a venue) is general-purpose and not
about zorp. What zorp publishes about itself is a tools paper, the same
genre as the papers describing AI-Scientist-v2 or other agent harnesses.

**Ruled out:** Treating "zorp writes a research paper" as meaning zorp
needs to autonomously produce a novel scientific result end to end before
it counts as done. That's a much larger, different bar than a systems
paper needs, and conflating the two would have quietly inflated scope.

---

## 2026-08-09: zorp's product is four standalone capabilities, human-authored papers only

**Decision:** zorp offers four capabilities that each work standalone,
chained by human checkpoints when used as a full loop: validate an idea
(literature/novelty check), run experiments (staged, sandboxed, every
attempt recorded), co-write a paper (zorp drafts from the run record, a
human edits and is the author of record, zorp never outputs a paper as
"done" on its own), and find a venue (match a finished paper against a
conference/journal catalog). zorp does not take a hard dependency on
Aviskaar-private infrastructure (ORR, lab-engine/Catalyst); an ORR
adapter can be optional and later, not the foundation.

**Why:** Most "AI scientist" agents assume the deliverable is a finished,
autonomously written paper. That's the wrong shape: AI-authored papers
are rejected outright at most venues, so the paper step has to be
collaborative, with the human as author, not a generator with a
claim-check pass bolted on. The standalone-capabilities framing also
matches how the harness will actually get used: someone validating one
idea, or just running an experiment, without wanting the full loop.

**Full writeup:** `docs/superpowers/specs/2026-08-09-zorp-architecture-design.md`.

---

## 2026-08-08: No em dashes or en dashes in repo prose

**Decision:** README, docs, commit messages, and comments in this repo
use plain punctuation (periods, commas, colons, plain hyphens) instead of
em dashes or en dashes, so the writing reads as plainly human as the code
itself.

**Why:** Requested directly, to keep the project's public-facing writing
from reading as AI-generated.

**Recorded in:** `CLAUDE.md` and `AGENTS.md` under "Writing style," so it
applies to all future writing in this repo, not just this pass.

---

## 2026-08-08: README rewritten as a full project front page

**Decision:** README expanded to badges, a why/architecture/getting-started/
status structure, with every command and env var in it checked against
the actual source rather than assumed.

**Why:** The repo will be public eventually and needs a front page that
holds up, not a stub.

---

## 2026-08-08: Harness renamed from quecto to zorp

**Decision:** All crates, binaries, env vars, and CLI/log strings renamed
from `quecto*` / `QUECTO_*` to `zorp*` / `ZORP_*`. quecto's own historical
docs (changelog, UAT reports, issue log) moved to `docs/upstream-quecto/`
and left untouched, since they document quecto's own history under its
own name.

**Why:** Deferred at bootstrap time on purpose (see the entry below), then
done once the base harness was in place and it was clear what was
actually being kept versus rebuilt.

---

## 2026-08-08: quecto vendored as the base harness, AI-Scientist-v2 kept local-only

**Decision:**
- [quecto](https://github.com/adityak74/quecto) (MIT) vendored as a
  source snapshot at the zorp repo root, fresh git history, crate names
  left as `quecto-*` initially (renamed later, see above).
- [AI-Scientist-v2](https://github.com/SakanaAI/AI-Scientist-v2) cloned
  into `reference/`, gitignored, used for design inspiration only and
  never committed.

**Why:** quecto is MIT licensed, so forking and modifying it directly is
safe. AI-Scientist-v2 uses a custom, restrictive "Responsible AI Source
Code License" that shouldn't travel with zorp's public repo, so it stays
local-only.

**Full writeup:** `docs/superpowers/specs/2026-08-08-zorp-bootstrap-design.md`
