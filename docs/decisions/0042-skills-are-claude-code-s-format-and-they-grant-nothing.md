---
status: accepted
date: 2026-08-18
---

# skills are Claude Code's format, and they grant nothing

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
