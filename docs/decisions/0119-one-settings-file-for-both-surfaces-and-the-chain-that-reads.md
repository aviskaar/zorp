---
status: accepted
date: 2026-09-10
---

# one settings file for both surfaces, and the chain that reads it

**Decision:** the provider, base URL, model and max tokens the browser saves
are read by the terminal too, from one file with one implementation. It
lives in `zorp-agent/src/config.rs`, `zorp-web` re-exports it under the names
it already used, and it is called `zorp.toml` rather than `web.toml` because
it stopped being the browser's.

**The chain is flag, environment variable, flavor, saved file, default.**
Somebody will be surprised by that order eventually, so here is the
reasoning for each step.

A flag is this invocation and beats everything, because a person typing
`--model` on one command means it for that command. An environment variable
is this shell and beats anything on disk, which is what makes
`ZORP_MODEL=x zorp-agent ...` work the way people expect. A flavor manifest
is this repository, and it beats the saved file because a project that pins
a model means it for that project, where the file is a person's standing
preference across all of their projects. The default is what is left.

The only new step is the saved file, and it is slotted *below* the flavor
and *above* the default on purpose: nothing that used to win stops winning,
so an existing setup sees no change at all. That was the requirement, not a
convenience, and `adding_the_saved_step_changes_nothing_that_was_already_set`
is the test that says so.

**The API key is not in that file and there is no field for it.** `Saved`
has no `api_key`, so there is nothing on the struct to serialize a secret
through by accident, and `config set api-key` refuses with a message
pointing at `ZORP_API_KEY`. The key was already the one thing both surfaces
shared, because neither persisted it. That does not change here and must not
change later: a settings file is a thing people paste, sync, and check into
dotfile repositories.

**`workspace` stays the browser's.** It is written to the shared file,
because it is a path and not a secret, and nothing in `zorp-agent` reads it.
The CLI works in the directory it was started in, which is a directory the
person chose by standing in it (`docs/DECISIONS.md`, 2026-09-05). A CLI that
read this field and moved itself would be a surprise rather than a
convenience.

**The old name is still read.** `web.toml` is read when `zorp.toml` is not
there, and `ZORP_WEB_CONFIG` is honoured after `ZORP_CONFIG`. Neither is
ever written, so the first `config set` after upgrading moves somebody onto
the new name without asking them to do anything. An explicit path from
either variable does not fall through to the old name: an explicit path is
an explicit path, and inferring a second file beside it is how a test
fixture quietly picks up the developer's own settings.

**Provenance is the useful half of `zorp-agent config`.** A person debugging
why they are talking to the wrong model can already see the value: it is in
the answers they are getting. What they cannot see is which of the flag, the
variable, the flavor and the file won, so every line says.

**What it rules out:** a second settings file, a per-surface override of the
shared one, and any secret on disk. A setting that only one surface can
usefully have goes in that surface's own state, the way `workspace` does,
rather than growing a second file.
