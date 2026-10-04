---
status: accepted
date: 2026-09-09
---

# A project is a label on a conversation, and a scope for what it remembers

**Decision:** a project is a name a person typed and a nullable
`project_id` on the session row. It is copied into the recall index as one
more column on `conversations`, and a search or a memory recall can name
one. There is no project store, no second index file, and nothing about a
conversation changes when it joins one. Deleting a project unfiles its
conversations and deletes none of them. A conversation that is in a
project reads only that project when a turn asks for memory. A
conversation in no project reads everything, as it always did.
`sessions.task` is untouched by all of it.

**Why a label and not a container.** Every alternative shape costs
something the label does not. A separate store means two places a
conversation can live and a migration when it moves between them. A
per-project index file means the embeddings for one conversation exist
once per project it has ever been in, and the expensive part of that file
is the embeddings. A `WHERE` clause on the scan that already runs costs a
column and reads fewer rows than the unfiltered search did. The thing a
project has to do is narrow what gets read, and a column narrows it.

**Deleting a project deletes no conversation.** Both statements run in one
transaction: the sessions are unfiled, then the project row goes. This is
why the sidebar's delete control has no confirmation dialog. There is
nothing to confirm; the control's own tooltip says the conversations are
kept, and that is the whole of what a dialog would have asked about. A
project that could take a hundred conversations with it would need a
dialog, and would also be the wrong feature.

**Memory in a project reads that project and nothing else.** No preferring
the project with a fallback to everything. A project is what the person
chose as the context for a thread, and quoting an unrelated conversation
into it is the precise thing the scope exists to prevent; a fallback would
make the scope advisory, which is to say not a scope. When the project has
nothing relevant the turn runs with no memory block and the existing
`memory` frame says so, which is a truthful answer rather than a silently
widened one. This is a live-turn rule and not a search rule: the sidebar
search box has a select with `All conversations` on it, because a person
looking through their own history is choosing what to look at, and a model
reading a project is not choosing anything.

**The project is part of the recall fingerprint.** The feed skips a
conversation whose fingerprint has not moved, and a conversation that has
been filed somewhere else has not changed a word. Without the project in
the fingerprint the index would skip it and go on saying it is in the
project it left, which is a stale label on a filter, which is a search
that quietly lies. The move and delete routes also queue the affected
sessions on the indexer, so the label catches up in a moment rather than
on the next sweep five minutes later.

**`sessions.task` is untouched.** Nothing here writes it, nothing reads it
to decide anything, and no model is asked to name a project, pick one, or
read one. A project name is a third string beside `task` and
`display_title` and it lives only in `projects.name`. See the 2026-08-22
title entry for why that column is the way it is.

**Ruled out for now:** creating a chat directly inside a project, renaming
a project, and project-level settings, prompts or files. A person makes
the chat and moves it, and deletes and recreates a project to rename it.
Each of those is a separate decision about what a project is, and this
entry says it is a label.
