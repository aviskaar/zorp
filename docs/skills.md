# Skills

zorp reads Claude Code compatible skills.

zorp reads skills in Claude Code's format, so skills you already have
work here without being ported. A skill is a directory holding a
`SKILL.md`: YAML frontmatter with a `name` and a `description`, then a
markdown body of instructions.

```
~/.claude/skills/code-review/SKILL.md     # yours, everywhere
<repo>/.claude/skills/code-review/SKILL.md # this project's, wins on a name clash
$ZORP_SKILLS_DIR/code-review/SKILL.md      # explicit for this run, wins over both
```

```markdown
---
name: code-review
description: Review a diff for correctness bugs. Use when asked to review changes.
---

Read the diff first, then the surrounding code. Report findings by
severity, and say when you are unsure.
```

The model sees only the names and descriptions, as one `skill` tool whose
description is the index. It loads a body by calling that tool, and the
body arrives as instructions for that turn. The two levels are the point:
descriptions are cheap enough to always carry, bodies are not.

Skills add guidance, never permissions. A skill cannot enable a tool,
loosen an approval preset, or reach past the `run_command` denylist, and
the `allowed-tools` field some skills carry is read, reported, and
ignored. A skill body is a markdown file that can arrive with a `git
clone`, and it is treated that way: names are single path components and
never joined onto a path, a `SKILL.md` that resolves outside its own
directory is skipped, files over 64 KiB are skipped, and a malformed one
is skipped with a message naming the file while its siblings still load.
A project flavor can withhold skills entirely by leaving `skill` out of
`[tools] enabled`.

Skills are not capsules. A capsule is something you load with `/load` to
put the whole session in a mode, and it stays in the system prompt. A
skill is something the model reaches for mid task and uses for that turn.
