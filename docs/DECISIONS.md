# Decision log

A running record of product and architecture decisions made while
building zorp. Newest entries at the top. Each entry is short: what was
decided, why, and what it ruled out. Full design writeups, when they
exist, live in `docs/superpowers/specs/` and are linked from here.

---

## 2026-08-14: kill thresholds carry a direction, and are enforced

**Decision:** a pre-registration now records a threshold direction
(`lower-is-better` or `higher-is-better`) alongside the metric and the
number, and `investigate` compares each recorded attempt against it. A
breach kills the track. `--threshold-direction` is required whenever a
threshold is set, the direction lives in `prereg.md` (so the existing
SHA-256 hash and git commit cover it), and it has its own column in the
`preregistrations` table.

**Why:** the threshold was only ever formatted into a prompt string and
never compared to anything, so a track that badly missed its own
threshold stayed Active. That is the one guarantee the whole product
rests on. A bare number could not be enforced even in principle, since
nothing said which side of it was failure.

**What it rules out:** guessing. A breach is exempt from
`AutoApprove`/`--yes`, because auto-approving the one decision that
exists to stop a run defeats the point. A legacy pre-registration with
no recorded direction is skipped with a loud warning rather than
enforced against an assumed direction, since guessing wrong would kill
healthy tracks.

---

## 2026-08-14: git is the root of trust for pre-registration integrity

**Decision:** rebuilding the evidence store from `prereg.md` files no
longer trusts the files on disk. The rebuild hashes the committed git
blob and compares it against the working tree; a mismatch is an
integrity error rather than a fresh row. A file with no commit behind it
is marked unverified instead of being presented as equivalent to a
committed one. `verify_prereg_integrity` now also checks the recorded
`git_commit_hash`, which was previously written but never read.

**Why:** the recovery path recomputed the hash from whatever was on disk
and stored that as authoritative, so deleting the DuckDB row or
corrupting one byte of the store turned a tampered pre-registration into
a verified one. The tamper-evidence guarantee was defeated by the
recovery path meant to protect it. Two existing tests asserted this
behavior as correct and were rewritten.

---

## 2026-08-14: the vector library is opt-in, not part of research

**Decision:** LanceDB moves behind a non-default `library` feature in
`zorp-track`, with a matching opt-in feature in `zorp-agent` that
`research` deliberately does not enable. `Project::library` opens
lazily, and `validate` skips the embed-and-insert step when the feature
is off.

**Why:** it was a write-only sink. `validate` wrote cited sources into
it, nothing ever read them back, and the citations `co-write` actually
uses come from the DuckDB `validations` columns. It cost roughly 390 of
`zorp-track`'s dependencies (the whole arrow and datafusion tree) for no
behavior. It stays available rather than deleted, because a retrieval
story is a plausible future.

---

## 2026-08-14: measurement code fails loudly instead of guessing

**Decision:** `zorp-eval` gained three honest non-result states rather
than folding unevaluable runs into pass or fail. An unreadable trace
records `trace_unavailable` and skips contract evaluation entirely,
malformed lines inside a valid trace are skipped and counted in a new
`runs.trace_malformed_lines` column, and ordering predicates over
seq-less events report `unevaluable`. Unknown predicate ids are a
load-time hard error. The unimplemented LLM grader and the `eval`
subcommand now return not-implemented errors instead of reporting
success.

**Why:** every one of these paths previously produced a confident,
recorded result from evidence that was never actually evaluated. A
truncated final trace line became "all contracts failed"; a typo in a
contract id became a permanent violation or a silent pass. For a harness
whose only purpose is trustworthy measurement, a fabricated result is
worse than a missing one.

---

## 2026-08-14: command policy analyzes substitutions and redirect targets

**Decision:** the run_command denylist now recurses into `$(...)`,
`<(...)`, and `>(...)` bodies the same way it already did for `sh -c`
payloads, tokenizes redirect operators as distinct tokens and checks
their targets, and denies destructive `rm` whose targets escape the
repository root. Unbalanced substitution syntax fails closed. `>
/dev/null` is now explicitly allowed, where the old substring check
denied it.

**Why:** `$` was an ordinary word character to the tokenizer, so
`echo $(sudo rm -rf /)` parsed as a call to `echo`, resolved to Ask, and
ran under `--yes`. The redirect check matched four literal spellings and
missed `> ~/.ssh/authorized_keys`. The root-rm guard matched only a bare
`/`, so `rm -rf /*` passed. These were holes in an otherwise careful
fail-closed design, not a missing design.

---

## 2026-08-14: CI covers the research stack, and the lockfile is committed

**Decision:** `Cargo.lock` is tracked and CI builds with `--locked`. The
research stack (`zorp-track` plus `zorp-agent --features research`) gets
its own job, running nightly and on pull requests that touch it, while
the per-PR fast path still excludes `zorp-track`. Added a macOS matrix
leg and a `cargo fmt --check` gate. `panic = "abort"` is gone from the
release profile.

**Why:** an entire crate and a feature-gated surface could stop
compiling while main stayed green, which is exactly what "excluded from
CI" means over time. An untracked lockfile made builds
non-reproducible and degraded cache hits. `panic = "abort"` silently
disabled the `catch_unwind` guard around subagent execution in every
release build, so a subagent panic killed the whole process in
production while passing in tests.

---

## 2026-08-13: paper rebuilt as a real arXiv preprint, with a bibliography

**Decision:** the paper is now built through a proper LaTeX toolchain
rather than pandoc's defaults. `docs/paper/arxiv-template.tex` is a
pandoc template implementing the standard arXiv preprint presentation
(Times via `newtx`, ruled abstract block, numbered sections, small-caps
running head), `docs/paper/references.bib` is a real bibliography, and
`docs/paper/Makefile` runs the full pandoc, pdflatex, bibtex, pdflatex,
pdflatex cycle and cleans up after itself. Figures were regenerated in
Times to match the body text, in a restrained academic style rather
than a marketing one. The paper itself was rewritten: a Design
Principles section now states the three commitments the system is
organized around, a Limitations section states what the tamper-evidence
guarantee does and does not cover (it is scoped to the pre-registration
file and inherits git's trust model), and claims are cited throughout.

**Why:** the previous draft had no citations at all, which no venue
would take seriously, and its Helvetica-on-pandoc-defaults presentation
read as a rendered README rather than a paper. The bibliography also
does real argumentative work: zorp's central mechanism, committing a
falsification threshold before gathering evidence, is the
pre-registration literature's answer to undisclosed analytic
flexibility, and the paper is stronger for saying so and citing it than
for presenting the idea as novel. The two AI-Scientist references were
verified against arxiv.org directly, since they carry the paper's
framing; `docs/paper/README.md` flags that the rest need a verification
pass before submission.

---

## 2026-08-13: paper corrected to zorp-landing's real branding and arXiv formatting

**Decision:** the first paper draft (below) used the wrong logo (the
purple-on-dark favicon glyph) and an assumed dark/purple color scheme
that doesn't match zorp-landing. Corrected: the header and title page
now use the real zorp mark (a node-and-edge "Z", navy with two
electric-blue accent nodes), and all three generated figures were
recolored to zorp-landing's actual palette
(`zorp-landing/src/styles/tokens.css`: light theme, `--z-fg` navy,
`--z-accent` electric blue). The paper's language was also brought in
line with the live site (`zorp-landing/src/config/site.ts`): opens with
the site's own hook ("Answers are cheap. Evidence is not."), names the
kill threshold the way the site does, and documents the real six-table
evidence store (`tracks`, `preregistrations`, `experiments`, `metrics`,
`checkpoints`, `validations`) rather than a general two-store
description. Test and line counts were re-verified directly against the
repo rather than trusted from the landing page, which turned out to be
stale in places (a screenshot asset showed a different tagline and an
older test count than the current site config); the paper now cites
538 passing tests and 24,965 lines, both freshly confirmed against
HEAD. Formatting changed to a single-column arXiv-preprint style:
numbered sections, colored running header, boxed abstract. Also, per
house style (`CLAUDE.md`), all em/en dashes used as punctuation were
rewritten as periods, commas, colons, or plain hyphenated compounds.

**Why:** a systems paper about zorp representing zorp with the wrong
brand mark and colors undermines the credibility it's trying to
establish. Trusting a stale marketing asset over the actual repository
state would have repeated the same kind of unverified-claim mistake the
paper explicitly argues against.

---

## 2026-08-13: zorp's own arXiv-style systems paper written, first draft

**Decision:** `docs/paper/zorp-paper.md` is a first draft of the systems
paper about zorp itself, scoped in `docs/paper/README.md` since
2026-08-09. Covers architecture, the `zorp-track` foundation, and the
four capabilities, all grounded in what's actually built and tested as
of this writing (real test counts and LOC, not aspirational numbers).
Figures (layered architecture, the four-capability pipeline, test
counts) are generated from real repo state by `docs/paper/make_figures.py`,
not mocked up. The header logo is redrawn from
`zorp-landing/public/favicon.svg`. Built to PDF via
`pandoc --pdf-engine=xelatex`; both the Markdown source and the rendered
`zorp-paper.pdf` are committed.

**Why:** the paper needs a real eval story before it's submittable
anywhere ranked (see `docs/paper/venues.md`), which doesn't exist yet.
This draft is explicit about that gap: it reports the design and what's
tested, and lists the comparative evaluation against AI-Scientist-v2 as
future work rather than fabricating numbers to fill the gap. Posting to
arXiv as a preprint doesn't need review or a deadline, so this draft can
go out regardless of the ranked-venue timeline in `venues.md`.

---

## 2026-08-13: README/CONTRIBUTING default to excluding zorp-track, and default-run fixes the ambiguous zorp-agent binary

**Decision:** README and CONTRIBUTING now lead with
`cargo build --workspace --exclude zorp-track` / `cargo test --workspace
--exclude zorp-track` instead of the plain `--workspace` forms, with a
note on the 20-30 minute cold build if `zorp-track` is included. README
also gained a short section on connecting an MCP tool for
`validate`/`deliver`, since neither can run without one and the failure
mode gave no pointer to the fix. Separately, `zorp-agent/Cargo.toml`
now sets `default-run = "zorp-agent"`, since the test-only
`stub_search_mcp_server` binary made `cargo run -p zorp-agent -- "<task>"`
(the exact command in the README) fail with an ambiguous-binary error.
`src/main.rs`'s one-shot path also now prints a hint to set
`ZORP_API_KEY` when a request fails with no key configured, instead of
just surfacing the raw HTTP status.

**Why:** usability testing of the public-release candidate found that a
first-time user following the README verbatim hit all four of these in
the first few minutes: an unexplained 20-30 minute build, a broken
documented command, an unrunnable flagship capability with no
documented fix, and an opaque 401 with no actionable next step. Each is
a small, targeted fix; none change behavior for anyone already working
around them.

## 2026-08-13: CI excludes zorp-track from the default workspace test run

**Decision:** `.github/workflows/ci.yml` runs
`cargo test --workspace --exclude zorp-track` instead of
`cargo test --workspace`, and no longer runs
`cargo test -p zorp-agent --features research` at all. Both remain
required locally before considering Rust changes done (see CLAUDE.md);
CI just doesn't enforce them yet.

**Why:** `zorp-track` is the only workspace crate depending on `duckdb`
(bundled — compiles DuckDB's C++ amalgamation from source) and
`lancedb` (pulls in Arrow and DataFusion transitively). On a cold
cargo cache, compiling it took CI past 20+ minutes on GitHub's shared
runners, several times in a row, before `Swatinem/rust-cache` ever got
a chance to save a cache (it only saves on successful job completion,
so a run cancelled for taking too long guarantees the next run is cold
too). Excluding it keeps CI fast and cheap for every other crate, at
the cost of not catching zorp-track/research-feature regressions in CI
until a better strategy (self-hosted runner, a slower opt-in workflow,
or seeding the cache once) is worth the cost.

**Ruled out (for now):** letting one run finish uncapped to seed the
cache, and requesting a larger/paid runner — both reduce or shift cost
rather than remove it, so scope reduction was chosen instead.

---

## 2026-08-09: deliver's design: huiban-only, academic venues only, checkpoint doesn't kill the track

**Decision:** `deliver` is scoped to academic venue-matching only for v1,
not the broader "right format for any audience" language used elsewhere.
It requires a `draft.md` (from `co-write`) and a huiban-prefixed MCP
tool to be configured, checked the same way `validate` requires a
search-capable tool. The agent uses huiban to find and rank real
conferences and journals fitting the draft's scope, writes the shortlist
to `venues.md`, and checkpoints it. Rejecting the checkpoint does not
kill the track, matching `co-write`'s behavior, not `validate`'s or
`investigate`'s.

**Why:** A non-academic artifact has no equivalent of a "venue" in the
same concrete sense a paper does, and a generic reformatting mechanism
for arbitrary audiences is a different, larger problem than a first
version needs to solve. Requiring huiban specifically, rather than
falling back to generic search, avoids weak or fabricated venue matches
from a tool not built for this. Not killing the track on rejection
matches `co-write`'s reasoning: a shortlist not being good enough isn't
evidence anything upstream failed.

**Ruled out:** A general "format for any audience" mechanism for
non-academic artifacts (would need its own design if it becomes a real
need, not a bolt-on here). A shipped, static venue catalog (already
ruled out earlier in the decision log). Falling back to generic web
search when huiban isn't configured.

**Full writeup:** `docs/superpowers/specs/2026-08-09-zorp-deliver-design.md`

---

## 2026-08-09: co-write's design: grounded drafting, no post-hoc claim-check, rejection doesn't kill the track

**Decision:** `co-write` hands the agent the track's actual recorded
evidence (validate's verdict if present, every metric investigate
recorded) as structured data in the prompt and instructs it to cite only
those figures, rather than drafting freely and then scanning the output
to verify numeric claims afterward. Requires at least one recorded
metric to run at all. The agent's answer is written directly to
`draft.md`, no scored JSON block. Unlike validate and investigate,
rejecting co-write's checkpoint does not kill the track: a draft not
being ready isn't evidence the investigation failed.

**Why:** Grounding at the input side (only real numbers ever reach the
model) is simpler and more reliable than extracting and re-verifying
numeric claims from free-form prose after the fact, which is a much
harder problem with its own false-positive/negative risk. Requiring a
metric to exist keeps co-write from drafting off a validate pass alone,
which is a go/no-go check, not evidence. Not killing the track on
rejection matches the normal expected path once a draft exists: a human
takes over editing `draft.md` directly, or the call runs again.

**Ruled out:** A post-hoc claim-check pass over the drafted prose.
Tamper-evidence hashing of `draft.md` like `prereg.md`'s SHA-256 (a
mtime-based warning only, not an integrity guarantee). Killing the track
on a rejected co-write checkpoint.

**Full writeup:** `docs/superpowers/specs/2026-08-09-zorp-co-write-design.md`

---

## 2026-08-09: investigate's design: CLI-supplied prereg, one attempt per call, checkpoint decides kill

**Decision:** `investigate` takes `--metric-name`/`--kill-threshold` as CLI
arguments (not agent-proposed) the first time it runs for a track, writes
and checkpoints the pre-registration, then runs exactly one attempt per
invocation, records a typed metric via the existing `zorp-track`
experiment tables, and hands the kill/keep decision to a human checkpoint
rather than comparing the metric to the threshold in code.

**Why:** A human-committed threshold is the whole point of
pre-registration; an agent-proposed one would defeat it. One attempt per
invocation keeps every attempt visible at a checkpoint instead of burning
budget inside a single call before a human sees anything. No stored
"kill direction" (above/below is favorable) means no risk of that logic
guessing wrong; the checkpoint prompt shows the human the number and the
threshold and lets them decide, matching the existing "no hard experiment
budget" decision.

**Ruled out:** Multi-attempt loops within a single invocation. Automatic
threshold comparison deciding kill/keep without a human. Requiring a
prior `validate` approval before `investigate` can run (the existing
standalone-capabilities decision already rules this out).

**Full writeup:** `docs/superpowers/specs/2026-08-09-zorp-investigate-design.md`

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
