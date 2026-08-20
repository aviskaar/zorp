# Decision log

A running record of product and architecture decisions made while
building zorp. Newest entries at the top. Each entry is short: what was
decided, why, and what it ruled out. Full design writeups, when they
exist, live in `docs/superpowers/specs/` and are linked from here.

Entries are never rewritten or deleted. When a later decision reverses an
earlier one, the earlier entry gets a **Superseded by** line pointing
forward and is otherwise left as it was written, so the record shows what
was believed at the time and not only what survived.

---

## 2026-08-19: the browser can stand its approvals down, per session, loudly

**Decision:** the web UI gets auto-approve: a per session standing yes
that stops the browser being asked about every tool. Off for every new
session, turned on only by an explicit control, revocable in the middle
of a run, and never written to disk. `POST /api/sessions/:id/auto-approve`
sets it, `GET` reads it back, and it lives on `SessionState` as an
`AtomicBool` shared with each turn's `WebApprover`.

**Why it is not a new `ApprovalMode`:** the mode already exists.
`ApprovalMode::AutoApprove` is what `--auto-approve` and the chat
`/approve` command have always given the CLI, and this is the same thing
with the same name. What the web needed that the CLI did not is the
ability to change its mind mid-run: `Agent::set_approval` takes `&mut
self` and the agent is owned by the turn thread, out of reach of an HTTP
handler. The session's `WebApprover` is the one object a handler can
reach while a turn is running, so the standing answer lives there and is
read per call rather than baked into the agent at construction.

**Why it is not a `Preset` change either:** presets say which operations
have to ask. This says who answers. Flipping the web to `Preset::Full`
would change what the policy decides, would still leave `mcp__` tools
asking, and would be a server configuration decision taken by a browser
button. Wrong axis, wrong owner.

**What it does not bypass:** anything. `Policy::decide` runs first in
`Agent::run` and only its `Ask` reaches an approver at all, so the hard
denylist, compound commands and `sh -c` payloads included, refuses the
same commands with this on as with it off. `zorp-agent` was not modified
to add this feature; the only change there is a test that pins the
ordering (`a_denylisted_command_is_refused_even_under_auto_approve`),
which fails if `decide` and the approval gate are ever swapped.

**What it costs:** the user has to be able to see it. A red pill in the
toolbar and a banner over the composer both say so for as long as it is
on, each auto-approved call leaves an `auto-approved <tool>` line in the
transcript, and a call that cannot be written to that transcript is
refused rather than run unrecorded.

**What it rules out:** persisting it. There is no config key and no
database column, so it cannot follow the user into tomorrow's session or
survive a restart, and no manifest can turn it on for someone. It also
rules out turning it on as a side effect of anything else: the switch
does not answer the approval already on screen, and the "Allow all for
this chat" button on a card does both steps in the open, mode first.

---

## 2026-08-19: a turn is seeded from the store, and compaction never writes to it

**Decision:** every web turn rebuilds its agent from the stored
transcript rather than starting empty. The store is the source, not a
live agent kept in memory per session. `zorp_agent::plan_seed` builds
what gets sent: the current system prompt, then the recorded
conversation, repaired so no tool call is left dangling, compacted if it
will not fit. The CLI's `resume` goes through the same planner, so both
surfaces continue a session the same way.

**Why the store and not a live agent:** the store outlives the process
and a live agent does not. Reopening a session from the sidebar after a
restart and continuing it is then the same code path as continuing it a
second after the last turn, rather than a second mechanism that only gets
exercised when something has already gone wrong.

**Why the prompt is not replayed:** stored system messages are dropped
and one current prompt is put at the front. The prompt is the harness's
to set. A session recorded before a prompt change should not keep
re-sending the old one, and the several stored copies the web server used
to write (one per turn, since every turn built a fresh agent that
recorded its own system message) collapse back to one.

**What compaction throws away, in order:** the bodies of the oldest tool
results, replaced by a marker saying how many bytes went. That extends
the byte cap that already existed rather than sitting beside it: one
mechanism with two triggers, the 512 KiB cap on accumulated tool results
and, when a window is configured, a token target. Two passes eliding the
same bodies for their own reasons would double-count what they freed. On
the seed path only, whole exchanges go from the front first, a user
message and everything that answered it together, so the model never sees
a reply to a question that is no longer there. The newest exchange is
never dropped.

**No model-written summary.** A summary is a second chance to
hallucinate, and when it is wrong the material it replaced is no longer
in the request to contradict it. It also costs a model call that can fail
mid-turn, on the endpoint that just ran out of room. Deterministic
elision states exactly what left.

**Compaction never rewrites the record.** It rewrites message bodies in
the agent's own transcript after the recorder has already been handed the
originals, and drops whole messages only before a run starts, never
during one, because `sync` tracks what it has persisted by index. What is
sent shrinks; what was said does not. `zorp-track` and the research
capabilities treat the record as evidence and the 2026-08-18 critique
entry below says why a record that moves under you is worthless.

**The user is told.** Compaction emits a `notice` naming what went and
saying the full transcript is still on disk, because silent context loss
is how an agent starts confidently contradicting itself.

**What it rules out:** guessing the context window. zorp talks to
arbitrary OpenAI-compatible and Anthropic endpoints, local Ollama
included, and none of them can be asked how large their window is. There
is no default that is not wrong for somebody, so the window is unknown
unless `ZORP_CONTEXT_TOKENS` says otherwise. Unknown means the meter
shows tokens with no percentage and no bar and names the variable, and
token-driven compaction is off while the byte cap still runs. The meter
prefers the provider's own reported usage and marks the fallback estimate
with a tilde and its own wording, because a measurement and an estimate
must not be drawn the same way.

---

## 2026-08-19: a PDF is read for its text, not framed for its layout

**Decision:** `.pdf` leaves the sandboxed-iframe category and joins the
office formats. The server reads the text out of it with `pdf-extract`,
in a new `zorp-web/src/pdf.rs`, and sends markdown. The pane renders that
through the same renderer `.md` and `.docx` already go through. This
reverses the PDF half of the 2026-08-18 entry below and the "PDFs" section
of `superpowers/specs/2026-08-17-artifact-pane-design.md`.

**Why:** the old design said the browser's own viewer would render the file
inside the iframe. It does not, and never did. The raw endpoint sends every
file with a bare `Content-Security-Policy: sandbox`, which puts the document
in an opaque origin with scripting off, and no browser's PDF viewer starts
under that. What a user actually saw was a broken-document icon on grey.
Confirmed in Chrome against a running server, and the frame was the right
size and visible, so it was never a layout bug.

That left three ways out and only one of them is defensible.

**Ruled out, loosening the sandbox for PDFs.** Making the viewer run needs
`allow-scripts` and `allow-same-origin`, and the raw endpoint is same origin
with the app, so `allow-same-origin` hands the framed document back the
handle the sandbox existed to take away. It also has to be loosened twice,
in the response header and in the iframe's own `sandbox` attribute, and both
would become per-type decisions. Today the header is one string for every
response, which is a property that cannot be got wrong for one type; making
it conditional trades that away. And the thing being made to run is PDFium
plus a PDF's own embedded JavaScript, over a file a model wrote or
downloaded. Paying all of that to render page images, when the request was
to read the file, is the wrong trade in both directions.

**Ruled out, bundling a JavaScript PDF renderer.** `web/` has zero runtime
dependencies on purpose, pdf.js is the opposite of small, and it would parse
a hostile PDF in this origin, which is strictly worse than what is there
now.

**What the extraction gives up:** layout. No page images, no figures, no
columns reassembled, no headings, because a PDF has no headings to recover:
it records where each glyph was drawn, and a heading in one is text that was
set larger. Promoting it to `#` would be inventing structure the file does
not carry. Text and the breaks between blocks survive, and the renderer
reflows them, which is what a narrow side pane wants anyway. Same scope as
the office formats and the same sentence applies: this is for reading what a
run produced, not for rendering a document.

**Cost paid:** one dependency, `pdf-extract`, and nineteen crates the
workspace did not have, all MIT or MIT/Apache-2.0. Roughly half of them are
`lopdf`'s AES and hashing for encrypted files, which lopdf does not put
behind a feature. `pdf-extract` already asks for lopdf with default features
off, so lopdf's chrono, jiff, rayon and time stay out. The alternative was
`lopdf` alone, which is most of that tree anyway and was measured before it
was rejected: on zorp's own paper its text extraction returns
`zorp:AHuman-CheckpointedResearchAgent`, with every space gone and the
ligatures dropped, because it does not apply the kerning offsets that stand
in for spaces or resolve the font's ToUnicode table. `pdf-extract` returns
the sentence. Hand-rolling it, the way `documents.rs` is hand-rolled, lands
at lopdf's quality and not at pdf-extract's, because that gap *is* the font
machinery.

**Also changed:** both readers now run under `spawn_blocking`. Both are
parsers pointed at a file a model wrote, both are slow enough to matter on a
long document, and a panic in one comes back as a join error rather than
taking anything else with it.

**What did not change:** `.svg` and `.html` are still the only sandboxed
types, still served with a bare `sandbox` and `nosniff`, and the test that
says so now asserts the exact header for every type the endpoint serves
rather than for one of them. Verified in the browser alongside the PDF: a
served SVG carrying `parent.document.title = "pwned"` is still refused by
the engine with "Blocked script execution ... the document's frame is
sandboxed", and the title is unchanged.

---

## 2026-08-19: a run that wrote a file opens the pane showing it

**Decision:** when a turn produces a file, the artifact pane opens by
itself and shows the newest one. This reverses part two of the
2026-08-18 entry below, which had the pane stay shut and put a count on
the Files button instead.

**Why:** the earlier reasoning was that a pane appearing over a half-read
answer is an interruption. That holds for a run that happened to touch a
file on its way to answering something else. It does not hold for the
ordinary case, which is somebody asking for a document: they get a small
dot on a button, read it as nothing having happened, and the document
sits unread behind a click nobody knows to make. The model cannot open
the pane either, so it ends its turn telling the user to go and find the
file, which is the interface asking the user to do its job.

**What it rules out:** the badge. Nothing sets it once the pane always
opens, so `showArtifactBadge`, the `artifacts-badge` span and its styles
are gone rather than left as unreachable code. Somebody who wants the
pane out of the way closes it, and closing it once is a smaller cost
than never finding the file at all.

---

## 2026-08-18: a draft gets audited against the record, and the auditor is code

**Decision:** `zorp-agent critique "<question>"` audits a track's
`draft.md` against that track's evidence record and revises what the
record does not support. It is not a fifth capability. It has no scope of
its own, gathers nothing, and produces no evidence: it reads the record
and edits the artifact `co-write` produced. Four capabilities is still
the whole set. See
[`superpowers/specs/2026-08-18-zorp-self-critique-design.md`](superpowers/specs/2026-08-18-zorp-self-critique-design.md).

**Why the model does not judge:** asking a model whether its own draft is
good produces a confident answer about a confident answer. The model's
only job here is extraction: inventory the draft's claims and name, from
a fixed list, the one piece of evidence each rests on. Code decides
whether that evidence exists. Alongside it, a purely deterministic pass
flags every figure in the draft that the record cannot account for at any
rounding, with no model involved at all, so a critic that reports nothing
cannot declare a draft clean.

**Why it terminates:** two bounds, neither of them the model's opinion. A
configured round bound (`--critique-rounds`, `ZORP_CRITIQUE_ROUNDS`,
default 2, where 0 means audit only), and a strict-improvement rule: a
revision is kept only if it leaves strictly fewer findings than the draft
it replaced, and the first one that does not ends the pass. Findings are
a non-negative integer that must strictly decrease, so the loop cannot
run longer than the initial finding count. That rule is also what makes
"the draft is fine" reachable: a clean draft costs one model call and
nothing is rewritten.

**What is recorded:** a `critiques` table in `zorp-track`, one row per
audited draft (round, draft hash, findings as JSON, whether it was
carried forward), plus `draft.pre-critique.md` for the diff and
`critique.md` for reading. Findings are deliberately **not** recorded as
metrics: they would then be evidence `co-write` reads, and a later draft
could cite its own critique as a finding about the question.

**What it rules out moving:** the Kill Threshold and everything else
pre-registered. The pass snapshots the record before it runs and
re-checks it after every model turn, and any movement aborts with the
draft untouched. This is enforced at runtime rather than assumed from the
call graph, and it is tested against a write-capable agent that really
does rewrite `prereg.md` mid-run.

**What it cannot catch:** that cited evidence actually implies the
sentence citing it. The pass verifies the evidence exists, not that it
supports the claim. Derived figures also read as invented. Both are in
the spec's limits section.

---
## 2026-08-18: skills are Claude Code's format, and they grant nothing

**Decision:** zorp discovers and loads skills in Claude Code's format,
unchanged: a directory holding a `SKILL.md` with YAML frontmatter
(`name`, `description`) and a markdown body. Discovery reads
`~/.claude/skills/<name>/SKILL.md`, then `<cwd>/.claude/skills/<name>/SKILL.md`,
then `$ZORP_SKILLS_DIR/<name>/SKILL.md`, later scopes winning a name
collision. Parsing and discovery live in a new crate, `zorp-skill`, that
depends on nothing else in the workspace. `zorp-agent` exposes it as one
built-in, `skill`, through the existing `Tool` trait.

**Why Claude Code's format and not a zorp native one:** the value of a
skill is that the user already wrote it, or already installed someone
else's. A zorp specific format would start with zero skills in existence
and ask users to port. Reading the format that already has skills in it
costs one small parser and buys the whole existing corpus. This was
checked, not assumed: 342 `SKILL.md` files on the author's machine, 339
parsed, and every one of those 339 descriptions matches what PyYAML
reads from the same file. The three that did not parse have no
frontmatter at all and are malformed by Claude Code's own rules.

**How this differs from capsules.** A capsule is loaded by the *human*,
with `/load`, and stays in the system prompt for the rest of the session,
where it can also point at a `scripts/` directory. A skill is chosen by
the *model*, mid turn, from a one line index, and arrives as a tool
result for that turn. The two answer different questions: "run this whole
session in this mode" against "this particular task looks like something
the user has written down". They also live in different places, `.zorp`
against `.claude`, because the second one is not ours to move. Neither is
a replacement for the other, and neither should be deleted for the other.
If they ever converge, the merge point is the parser, not the lifecycle.

**How this differs from flavors.** A flavor is configuration: model,
tool allow-list, approval preset, verification commands, all of it
trusted or gated as configuration. A skill is content. The layering rule
is borrowed from flavors, user then project, because users already know
it. Nothing else is.

**What a skill is not allowed to do.** It cannot enable a tool, widen an
approval preset, or get anywhere near the `run_command` denylist. Real
skills in the wild carry an `allowed-tools` frontmatter field; zorp
parses it, prints a warning saying it is being ignored, and ignores it.
The reason is that a skill is untrusted input by construction. It is a
markdown file that can arrive by `git clone`, and its body becomes
instructions. Anything that let that file also move the boundary would
make the boundary decorative. So: the tool argument is a lookup key into
an already scanned registry and is never joined onto a path; a name has
to be a single ordinary path component; a `SKILL.md` that resolves
outside its own skill directory is skipped (the directory itself may be a
symlink, because installing a skill that way is normal); and a file over
64 KiB is skipped rather than truncated.

**The one thing a skill does get:** `Policy::decide` treats `skill` like
a local read, `Allow`, alongside `read_file`. Gating it at `Ask` would
make skills unusable under the default `NonInteractive` mode, where `Ask`
means deny, and would buy nothing: the body confers no capability, and
every action it suggests still routes through the same policy. zorp
already folds project `AGENTS.md` and `CLAUDE.md` into the system prompt
with no prompt at all, so project controlled instruction text already
reaches the model unasked. A skill is strictly narrower than that: named,
capped, opt in per turn, and reported.

**Ruled out:** a YAML dependency (the frontmatter in use is flat scalars,
block scalars, and one level of nested map, and a full YAML parser is a
much larger surface aimed at a hostile file for features no skill uses);
letting a skill widen the tool set behind a trust prompt like the one
project flavors get (a flavor is configuration the user is choosing, a
skill is content that arrived with a repository); and putting skill
discovery in `capsule.rs` (adjacent, different lifecycle, and capsules
are inherited harness code).
## 2026-08-18: the artifact pane surfaces what a run wrote, and reads office formats

**Decision, part one:** the pane notices produced files by diffing the
`/api/artifacts` listing, not by reading tool events. Every listing row now
carries `modified_ms`. The browser snapshots the listing when a turn starts
and compares after tool activity and at the end of the turn; anything new or
newer is something the run wrote.

**Why not tool events:** because how a file got written is not knowable from
one. A PDF that pandoc produced under `run_command` names no path anywhere,
and it is exactly as much a result of the run as one `write_file` wrote.
Parsing tool summaries for paths would catch the easy half and quietly miss
the other, which is the worst of both. Asking the directory catches whatever
the next way of writing a file turns out to be.

**Decision, part two:** a run that produced something says so with a count on
the Files button, not by opening the pane. The pane opens itself only when it
is already open. A pane that appears over the answer somebody is reading is an
interruption, and the run has already finished, so there is nothing being
missed by waiting for a click.

**Superseded by** 2026-08-19: a run that wrote a file opens the pane showing
it. Parts one and three stand.

**Decision, part three:** `.docx`, `.odt`, `.xlsx` and `.pptx` are extracted
to markdown on the server, in `zorp-web/src/documents.rs`, and rendered
through the markdown renderer that already exists. Headings, paragraphs,
lists and tables. Images, fonts, colours, page layout, footnotes, comments,
tracked changes and numbered-list numbering are out of scope and stay out.
This is for reading what a run produced, not for rendering a document. The
extractor treats its input as hostile: caps on archive entries, on
decompressed bytes per part and per archive, and no XML entity expansion
beyond the five predefined ones, so neither a zip bomb nor billion laughs is
a case that needs thinking about at the call site.

**Decision, part four, and the one that needed the argument:** `.svg` and
`.html` are served and shown. Both execute. They are safe here for exactly
one reason: they load into the pane's iframe by URL, and the raw endpoint
sends every file with `X-Content-Type-Options: nosniff` and a bare
`Content-Security-Policy: sandbox`, no `allow-` token. That puts the document
in a unique origin with scripting off, so script inside it neither runs nor
gets a handle on the page that framed it. Verified in a browser: a served SVG
containing `parent.document.title = "pwned"` is refused by the engine with
"Blocked script execution ... the document's frame is sandboxed".

**What that rules out:** inlining either one. An `<svg>` element built from a
served file, or an iframe `srcdoc`, would run that script in this origin and
make every precaution in `web/src/markdown.ts` beside the point. The rule is
in `web/src/artifact-view.ts` and there are tests either side of it: the
server's headers, and the pane's refusal to put the bytes on the page even
when handed them.

**Cost paid:** two dependencies, `zip` and `quick-xml`. `zip` is pinned to 4
rather than the 6 already in the lock as a build dependency of
`libduckdb-sys`, because 6 requires Rust 1.83 and this workspace's floor is
1.82. Every codec but deflate is switched off; an office file uses no other.

## 2026-08-18: answers stream, and the streaming path filters reasoning

**Decision:** `Model::complete_streaming` is the agent loop's model call.
It has a default body that runs the buffered path and reports the answer
as one delta, so every existing `Model` keeps working; only `HttpModel`
on an OpenAI-compatible provider genuinely streams. `Renderer` gains
`assistant_delta`, empty by default, so only the browser changes.
Anthropic and the CLI are unchanged on purpose.

**Why the default body matters:** it means there is one code path in the
agent loop rather than a streaming branch and a buffered branch that
drift. A provider that cannot stream is not a special case, it is a
`Model` that reports its answer in one piece.

**The part that is not obvious:** `extract_think_tags` strips
`<think>...</think>` out of content before anyone sees it, so a
qwen-family model's chain of thought never reaches the browser today.
Streaming raw content deltas would put it on screen, formatted as the
answer. `streaming::ThinkGate` filters it, and withholds text that could
still turn out to be a tag opening, because the tags arrive split across
chunks like anything else. The accumulator then rebuilds a
buffered-shaped response and hands it to `parse_assistant_completion`, so
streamed and buffered turns cannot produce different messages.

**What it rules out:** shortening the SSE backlog. Pruning fragments once
the finished answer arrived was tried, and `stream_events` holds an index
into that vector across polls, so it panicked the streaming task and
poisoned the session mutex. The backlog is append-only and the doc
comment on `record` says why.

**Also decided:** a provider that ignores `stream` and returns a whole
JSON body is handled rather than treated as an empty stream. Silence is a
worse answer than a slow one.

---

## 2026-08-18: one system prompt, and it says zorp is a research agent

**Decision:** `zorp_agent::DEFAULT_SYSTEM_PROMPT` is the single default
system prompt for every zorp surface. The CLI and `zorp-web` both use it.
It positions zorp as a research agent, asks the model to ground claims in
what it actually checked, tells it to say so when it cannot, and says the
question does not have to be about code.

**Why:** the two surfaces had written their own. The CLI carried quecto's
"You are zorp-agent, a helpful coding assistant" unchanged through the
rename. `zorp-web` said "You are zorp, a careful assistant", which is
vague enough that the model fills in the rest itself: asked "zorp", a
local model introduced itself to a user as their "coding buddy". Neither
string matched the README, zorp.dev, or the four research capabilities.

The positioning is not decoration. A prompt that says "coding assistant"
is asking for a different behaviour than one that says claims must be
grounded and counter-evidence reported, and that behaviour is the
product.

**What it rules out:** a surface keeping its own copy. That is the
mechanism that produced two wrong answers here, so `zorp-web` gets a test
asserting its prompt is the shared constant, not merely a similar one.

**What it does not change:** overriding is still supported and still
cheap. `ZORP_SYSTEM` in the environment and `system_prompt` in a flavor
both win over the default, so a coding-focused flavor is a config file,
not a fork. `validate`'s preamble still narrows the frame further for
that one subcommand.

---

## 2026-08-18: the markdown renderer is ours, because the alternative is innerHTML

**Decision:** `web/src/markdown.ts` renders markdown by building DOM nodes.
No markdown dependency, and no `innerHTML` anywhere in the path from model
output to the page. Alongside it, `GET /api/artifacts` and
`GET /api/artifacts/raw` serve files from the directory the server was
started in, and a pane renders them: markdown through the same renderer, PDF
in an iframe. Design:
`docs/superpowers/specs/2026-08-17-artifact-pane-design.md`.

**Superseded by** 2026-08-19: a PDF is read for its text, not framed for its
layout, and only as to the PDF. The iframe never rendered one: the sandbox
this entry relies on is exactly what stops a browser's PDF viewer starting.
Everything else here stands, the sandbox included, which is still what makes
`.svg` and `.html` safe to show.

**Why not a library:** every markdown library worth using returns an HTML
string and hands you the `innerHTML` call. The text being rendered here is
model output, and the model has been reading tool results, web pages and
files, so treating it as trusted markup is cross-site scripting with extra
steps. The old code-block-only renderer already had this property and said
so in a comment; keeping it was the requirement, and writing the renderer
was the only way to keep it while rendering more than code blocks.

Two consequences that needed their own care. A markdown link is the one
construct where model output chooses a URL that the page makes clickable, so
only `http`, `https` and `mailto` become anchors and everything else renders
as visible text. Images are not supported at all, because `![](url)` would
make the page fetch an attacker-chosen URL on render, which is a beacon.

**Why serving files is scoped the way it is:** `zorp-web` already runs
commands and edits files in that directory, so reading from it is not new
reach. The new thing is a door that takes a path from a URL. Paths resolve
against the root and are checked for containment *after* canonicalization,
because looking for `..` in a string does not catch a symlink, and a symlink
inside the workspace can point anywhere. Types are an allowlist rather than
a denylist: an unknown type guessed at as `text/html` is a hole, while an
unknown type refused is an inconvenience. Every response carries `nosniff`
and a `sandbox` CSP, which is what makes it safe to put a PDF in an iframe.

**What it rules out:** rendering raw HTML in markdown, which shows as text
instead; generating PDFs, which needs LaTeX or typst and is its own project;
and editing from the pane, which is read-only. Syntax highlighting is
deferred, and the existing `data-lang` attribute is enough to add it later.

`web/` also gained its first test harness, jsdom plus `node:test`, wired
into the `web` CI job. A security-critical renderer with no automated test
was not worth shipping, and a typecheck does not notice a renderer that
starts producing markup.

## 2026-08-17: open-context connects as an MCP server, and searching your own material is not evidence

**Decision:** open-context is documented as an MCP server that a user
connects, not something zorp depends on, embeds, or starts. The README
carries a `.zorp/mcp.toml` recipe pointing at a local checkout.

Separately, `validate`'s search gate now rejects tool names that carry a
search verb over the user's own stored material (`context`, `memor`,
`note`, `recall`), not only names with no verb at all.

**Why:** zorp already has a general MCP client, so "use open-context in
zorp" needs no new coupling: eleven tools show up prefixed
`mcp__opencontext__` and the agent gets memory that outlives a session.
Wiring a Node package into a Rust product as a soft runtime dependency
would buy nothing this does not.

The gate change is not cosmetic. `mcp__opencontext__search_contexts`
carries "search", so before this, connecting a memory server silently
satisfied the evidence gate, and `validate` would score a question
against notes the user wrote themselves while reporting that it had
searched. A test now pins that, and it failed before the fix.

**Ruled out:** an `npx` recipe. The npm package named `opencontext` is an
unrelated project by another author, so that recipe would have zorp users
download and run a stranger's code.

**Known limit:** the gate is a heuristic over tool names, so it only
covers the case it can see. The durable version is an explicit per-server
flag in `.zorp/mcp.toml`, which needs the server config threaded from
`zorp-mcp`'s registry down into `validate`. Not done here: it changes
public API for a hazard the name check already closes in practice.

## 2026-08-17: model settings resolved server-side, UI over env over default

**Decision:** `zorp-web` gained `GET/PUT /api/settings`,
`GET /api/settings/models`, and `POST /api/settings/test`, all behind the
existing token gate. Settings are resolved on the server with a fixed
precedence per field: a value saved through the settings panel beats the
matching `ZORP_*` env var (`ZORP_PROVIDER`, `ZORP_BASE_URL`, `ZORP_MODEL`,
`ZORP_API_KEY`, `ZORP_MAX_TOKENS`), which beats the hardcoded default
(`https://api.openai.com/v1`, `gpt-4o`). `GET /api/settings` reports
`configured: false` only when every field is still at its hardcoded
default and no key is set anywhere, which is exactly the shape that used
to fail silently: `turn.rs` called `HttpModel::try_from_env()`, which
defaults quietly and let the first message a fresh install ever sent die
on a raw 401 deep inside the provider call. Ollama is not a third
`Provider` variant; it is `OpenAiCompatible` pointed at
`http://localhost:11434/v1`, offered in the UI as a preset. One code path
(`GET {base_url}/models`) lists models for Ollama and OpenAI alike,
verified against a real local Ollama instance during this change.

**Why:** The bug was not "no Ollama support," it was "no way to tell the
server what to talk to from the one place a user actually is: the chat
UI." Browser-shipped provider config per turn was rejected in favor of
server-side settings so the API key never has to leave the machine
running the server, and so the UI stays a thin client for state the
server owns.

**Secrets are held in memory only; non-secrets persist.** `PersistedSettings`
(provider, base_url, model, max_tokens) is written to
`~/.config/zorp/web.toml` on every successful `PUT`. It has no `api_key`
field, so there is nothing on that type to accidentally serialize a key
through. The API key lives in `SettingsState::api_key`, seeded once from
`ZORP_API_KEY` when the server process starts and replaced in memory by a
UI save; it is never written to disk and `GET /api/settings` reports only
`has_api_key: bool`, never the key. This matches the existing stance in
`zorp-agent/src/main.rs` that `api_key` is never read from a flavor
manifest either: secrets stay out of anything that gets committed, synced,
or read back over HTTP.

**What it rules out:** the settings file surviving a restart requires
`main.rs` to load it into `AppState` explicitly at startup; `AppState::new`
and `AppState::with_token` deliberately do not do this themselves, so
`zorp-web`'s existing tests stay hermetic and are not affected by whatever
a developer's own machine happens to have saved.

---

## 2026-08-17: clippy gates CI, on one runner, over all targets

**Decision:** CI runs `cargo clippy --workspace --exclude zorp-track
--all-targets --locked -- -D warnings` in a single ubuntu job. Warnings
fail the build.

**Why:** clippy had never run in CI at all. Nothing gated it, so
warnings accumulated unnoticed. Among them: dead test scaffolding, a
`mut` that was never needed, a length comparison to zero, an argument
list that had outgrown the lint's limit, and a std lock held across an
await in zorp-web's tests. That last one is test-only. It serialises
tests that write process-global env vars, and no production path
touches it. A check that only prints is a check nobody reads, which is
how the pile grew, so this one fails instead. Every warning was cleared
before the gate went in, so it starts from zero.

One runner rather than the two-OS test matrix: every lint clippy found
here is platform independent, and the macOS test leg still compiles the
platform-specific code. `--all-targets` because most of the warnings
were in test code, and linting the library alone would have gated the
easy half.

**What it rules out:** nothing lints `zorp-track` or the `research`
feature. That is deliberate, for the same reason the fast test job skips
zorp-track: it builds DuckDB from source and would dominate the job.
Closing that gap means adding a clippy step to the research jobs, which
already carry the warm shared cache for that tree. It is a known gap,
not an oversight.

## 2026-08-17: the web event stream belongs to the session, not to the turn

**Decision:** `GET /api/sessions/:id/events` holds its response open for as
long as the browser is listening. A finished turn does not end it, and the
next turn streams down the connection that is already open. There is no
"this session is finished" state on the server. A session the server does
not have in memory gets a 404 rather than an empty stream.

**Why:** the stream used to end when a turn ended, and `EventSource`
reconnects on its own whenever a response ends. A finished conversation
therefore opened a fresh connection every three seconds for as long as the
tab stayed open, with the status badge stuck on "reconnecting" and nothing
actually wrong. Measured in a browser against a local model: 43 connections
in 135 seconds from two turns, versus 1 connection now. It was worse than a
cosmetic bug, because `SessionState::finished()` looked for any `Done` in a
backlog that is never cleared, so after the first turn every later stream
closed the instant it opened and the transcript arrived only over the
automatic reconnects. The reconnect storm was load bearing.

**What it rules out:** ending the stream on `done`, on either side. Closing
the `EventSource` when a turn finishes is the obvious tidy-up and it stops
the next turn streaming at all, so both sides now say so in comments. It
also rules out treating an unknown session as an empty stream: that is a
reconnect loop with extra steps, which is what opening a stored session
from the sidebar was after a restart.

The polling loop behind the stream stays, per the original design note that
a chat UI does not need sub-100ms latency. It now walks only the new tail of
the backlog each tick, because the loop runs for hours rather than for one
turn.

## 2026-08-17: one version for the product crates, and a release refuses to disagree with it

**Decision:** `zorp`, `zorp-agent`, `zorp-mcp`, `zorp-track`, `zorp-eval`
and `zorp-web` inherit a single `[workspace.package] version`, bumped
with each release tag. `erbga` keeps its own version. A tag push whose
tag, workspace version and Dockerfile `ARG VERSION` default disagree
fails the release.

**Why:** the versions had drifted from the tags without anyone noticing.
`zorp` sat at 0.1.0 and `zorp-agent` at 0.2.1 across both v0.3.0 and
v0.3.1, so the published v0.3.1 binary answered `--version` with 0.2.1.
That is the release whose first message times out on a cold model, and
v0.3.1 is its fix, so every user who installed the fix and checked was
told they had not got it. The Dockerfile default had rotted the same
way, which meant a bare `docker build` fetched the broken release on
purpose.

**What it rules out:** independent per-crate versioning for the product
crates. If one of them ever needs to be published to crates.io on its
own cadence, this has to be revisited. Nothing needs that today, and the
cost of the current scheme was a public release that lied about which
release it was.

`erbga` is excluded because it is standalone published prior work that
does not ship with zorp, and giving it zorp's release number would claim
a relationship that does not exist.

## 2026-08-17: --version and --help are answered by the binary, not the model

**Decision:** `zorp` intercepts a leading `--version`, `-V`, `--help` or
`-h` and answers locally. Anywhere other than the first argument they are
still part of the prompt.

**Why:** they were joined into the prompt and POSTed to the model, so two
of the first flags anyone types at an unfamiliar binary cost a
completion, and with no key configured the new user's first impression
was a 401 wall of JSON from OpenAI. This is not a departure from "argv is
the prompt": `main` already intercepted `--init`. These were missing from
that list rather than deliberately excluded from it.

**What it rules out:** a prompt whose *first* word is one of those four
flags. `zorp what does --version print` still reaches the model, and
there is a test for it, because that is the behavior the change could
plausibly have broken.

## 2026-08-17: CI decides what to run with git, not a downloaded action

**Decision:** the research-stack path filter is a `git diff` and a grep
instead of `dorny/paths-filter`.

**Why:** the action is fetched from codeload when the job starts, and
when codeload answers 429 or 503 the job fails during setup before
running anything. That happened on three runs in one morning, twice in a
row on a rerun of the same commit, on pull requests that had not touched
the research stack and would have skipped every step anyway. A
dependency that can be unavailable was sitting in the critical path of
every pull request in order to decide to do nothing.

**What it rules out:** the action's richer glob syntax. The filter is six
patterns; if it ever needs more than a grep can express, reconsider. A
diff that fails now assumes the research stack changed, because failing
towards running the tests is the only safe direction.

## 2026-08-16: web search is a capability with a provider behind it, not a Tavily integration

**Decision:** search becomes a new workspace crate, `zorp-search`,
holding a `SearchProvider` trait with Tavily as the first
implementation. `zorp-agent` exposes it as a built-in `web_search` tool
behind a non-default `search` feature, gated at `Decision::Ask`, and
`validate`'s search gate accepts it by exact name alongside the existing
`mcp__` verb matching. The API key comes from `ZORP_TAVILY_API_KEY` and
never from a flavor manifest. Design:
`docs/superpowers/specs/2026-08-16-tavily-web-search-design.md`.

**Why:** `validate` is zorp's entry point and it could not run without
an MCP server, which means installing node, choosing a server, and
writing `.zorp/mcp.toml` before asking the first question. That barrier
showed up in the first-run walkthrough (#2) and forced the first two
UATs to validate against a stub server. Tavily removes the setup chain:
one endpoint, one key, results already extracted. Putting a trait in
front of it keeps the "vendor-neutral harness" claim honest and lets a
second provider land without touching the agent.

**Why `search` is separate from `research`:** this is the first built-in
tool that sends anything over the network, and the thing it sends is the
user's hypothesis. Running `investigate` locally should not acquire an
egress path by side effect, so the feature is opted into on its own.
For the same reason the tool asks rather than being allowed like a local
read, and a failed search is an error rather than an empty result set,
since a validate novelty score cannot tell those apart and would record
a wrong number.

**What it rules out:** bundling an MCP server (Tavily publishes one and
it stays a fine option); implicit searching outside a visible tool call;
and a result cache, which is deferred because caching changes what "the
evidence said" means across a re-run and interacts with pre-registration
in ways that need their own decision.

---

## 2026-08-16: everything zorp writes into a project lives under .zorp/

**Decision:** the `take_note` and `search_notes` tools write to
`.zorp/notes/` instead of `.qkb/`. No migration is provided and no
fallback read of `.qkb/` is kept.

**Why:** `.qkb` is a quecto-era name that the rename missed. Crates,
binaries, env vars, and the config directory all became `zorp`, and the
research stack already writes `.zorp/flavor.toml`, `.zorp/mcp.toml`,
`.zorp/tracks/`, and `.zorp/zorp.duckdb`. A second, differently named
directory appearing in a user's repo the first time an agent takes a
note is a leak of the fork's history into a user's project. Found in the
first UAT (`docs/uat/UAT-report.md`, F2).

**What it rules out:** carrying a compatibility read of `.qkb/`. zorp is
pre-alpha and the notes tools have no install base worth a migration
path; a fallback would keep the old name alive in the code indefinitely
to serve nobody. Notes already written to `.qkb/` stay on disk and stop
being found. The bootstrap decision (2026-08-08) dropped quecto's own
`.qkb/` directory as a session artifact but left the tools that create
it; this finishes that.

---

## 2026-08-15: evolve's search layer is not approved, its measurement discipline is

**Decision:** the `evolve` spec is marked NOT APPROVED and nothing is
built from it. Two rounds of adversarial review, eight reviewers, found
the search layer unsound both times, and the second round showed the
first rewrite had moved the flaw rather than removed it. `erbga` ships on
its own terms as a validated implementation of prior work, off any
critical path.

**Why:** three findings compose into one conclusion. There is no free
inner search, because variation is model-proposed, so the affordability
argument the whole design rests on is false. The framing score is
maximized by an undifferentiated blob, because CPM's objective is
extensive and edge addition was priced free. And two of its three score
terms are identically 1.0 by construction, which is the same defect as
the draft before it under new names. At `V = 20` an exact
clique-partitioning ILP solves the partition to proven optimality in
about 0.2 seconds, so the cut was never the hard part; the framing is,
and there is no cheap search over framings either.

**What survives, and should be built on ordinary `investigate` runs:**
never selecting on the pre-registered metric (breeding toward a metric
and then reporting it is biased upward twice, and pre-registration does
not cover selecting the observation that best clears a fixed test); the
confirmatory stage of `n` passes with the threshold on the mean and nulls
counted as non-passing; refusing to call framing diversity corroboration
when all lines share one model; quorum rather than unanimity for track
death.

**Bugs found in shipped code, worth fixing regardless:**
`TrackStatus::from_str` and `ExperimentStatus::from_str` both have
catch-all arms that silently coerce an unknown status to `Active` and
`Planned`. For a product that must not let a non-result look like a live
result, that is the worst possible default.

**Full writeup:** `docs/superpowers/specs/2026-08-14-zorp-evolve-design.md`,
whose "Where this stands" and "Review record" sections carry the detail.

---

## 2026-08-14: a fifth capability, evolve, searches question framings and never selects on the metric

**Superseded by** the 2026-08-15 entry above, one day later. Nothing
below was built. The search layer described here is not approved and the
spec it points at is marked NOT APPROVED. `erbga` did ship, on its own
terms and wired to nothing. The rest of this entry is left as written,
because the findings against it need something to point at.

**Decision:** zorp gains `evolve`. It searches for a good way to
**decompose** a question, not for an answer. A population of framings
(sub-questions, a weighted "bears on" relation, marked cross-cutting
premises) evolves against a deterministic structural score, and the
partition of any given framing is solved directly rather than evolved.
The pre-registered metric appears nowhere in selection; it is measured
once per island after the search, over n independent synthesis passes,
with the threshold applied to the mean. Output is a distribution and its
dissent, not a number. Ships as an `erbga` crate (the algorithm of Rao,
Janikow, Bhatia, Climer, MWAIS 2018, zorp's author's prior work, used as
the partition solver for large framings and validated against that
paper's benchmarks) plus `zorp-agent/src/evolve/`.

**Why:** breeding a population to maximize a metric and then reporting
that metric is biased upward twice over. Framings that surface
inconvenient evidence score worse and stop breeding, and the maximum of
N noisy evaluations exceeds the truth by about sigma*sqrt(2 ln N).
Pre-registration does not cover this: it stops you moving the test after
seeing data, not selecting the observation that best clears a fixed
test. Separately, the uncertainty in this problem lives in the graph,
which a model invents and which was never revisited, not in the cut,
which is a small solvable problem. So the compute moved to the framings.

**What it rules out:** modularity as the objective, since its resolution
limit does not bind on the source's benchmarks but binds on every
realistic question graph, so CPM with a pre-registered gamma is used
instead. Corroboration as a claim, since all islands share one model;
the property is renamed framing diversity, cross-island evidence reuse
is forbidden for anything feeding a reported result, and component
source overlap is measured rather than assumed. Track death by
unanimity, replaced with a pre-registered quorum. A recorded-only
parameter tier, since when the result depends on a search, search effort
is answer selection, so every input is pre-registered. Also gone: Gene
Repair at this layer (the source's own results show accuracy degrading
monotonically with density with it enabled), and the claim that
evidence cost is bounded by vertex count.

**Full writeup:** `docs/superpowers/specs/2026-08-14-zorp-evolve-design.md`,
whose closing section records what four adversarial reviews changed.

---

## 2026-08-14: stdio MCP reads get a deadline, and unadvertised features are not probed

**Decision:** `StdioTransport` reads its child's stdout on a helper
thread and waits with a deadline, so `timeout_secs` now means the same
thing for stdio that it already meant for the HTTP transports. A blown
deadline is a real `McpError::Timeout` naming the server. The transport
also closes stdin and reaps the child on drop. Separately,
`list_prompt_names` returns empty without sending anything when
`initialize` did not advertise a `prompts` capability; a server that
reports no capabilities block at all is still asked, since "did not
say" is not "does not have".

**Why:** Found by end-to-end UAT. `timeout_secs` was accepted and
silently dropped on the stdio path, and a pipe read cannot carry its own
deadline the way a `ureq` call can. Any stdio server that took a request
and never answered wedged the whole CLI with no output and no way out
but Ctrl-C. That is not a hypothetical shape: zorp asked every server
for `prompts/list` whether or not it offered prompts, and a server is
free to ignore a method it never claimed. The capability gate removes
the common case, the deadline covers the rest, and reaping on drop keeps
a wedged server from outliving the process it was spawned for.

**Ruled out:** non-blocking pipes plus `poll`, which is Unix-only and
buys nothing over a reader thread here.

---

## 2026-08-14: subcommands win over the bare-task positional

**Decision:** the CLI uses `subcommand_precedence_over_arg` instead of
`args_conflicts_with_subcommands`. A task whose first word is a
subcommand name is still reachable with `--`.

**Why:** with the old setting, any global flag before a subcommand made
the trailing task positional swallow the subcommand. `zorp-agent --yes
undo` did not undo anything: it sent the word "undo" to the model as a
task, ran an agent with auto-approval on, printed whatever came back,
and exited 0. Every subcommand was affected, and it failed silently in
the direction that looks like success, which is the worst way for a CLI
to be wrong.

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
