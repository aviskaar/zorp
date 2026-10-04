---
status: accepted
date: 2026-09-05
---

# zorp-web works in a workspace somebody chose, and in none until they have

**Finding:** `zorp-web` ran the agent in the directory the server was
started in. In practice that is the zorp checkout, so every file the agent
wrote, every PDF it rendered and every throwaway script it left behind
landed in zorp's own source tree, and `git status` in this repo grew
`k8s-load-balancing.pdf` and friends.

**Decision:** the working directory is a workspace a person picks.
`--workspace`, then `ZORP_WORKSPACE`, then the path saved in
`~/.config/zorp/web.toml`, and then nothing. There is deliberately no
fallback to the current directory, because falling back is the bug: a
server that guesses writes somewhere nobody chose, and the guess is
invisible until the files turn up. With none of the three set the server
still binds, still serves the UI and still answers the settings endpoints,
says on stderr that nothing is chosen, and refuses to run work.
`POST /api/sessions/:id/turn`, `/panel` and `/investigate` answer 409 with
the body `no workspace chosen`, exactly, because the browser matches on the
status and the sentence to open its picker rather than show an error. The
ledger read answers the same way. `zorp-web/src/turn.rs`,
`panel.rs`, `investigate.rs` and the ledger handler in `api.rs` no longer
call `std::env::current_dir` at all. The CLI in `zorp-agent` is untouched:
it works where it was started, and standing in a directory is how a person
chooses one there.

Every path is checked by one function, `workspace::validate`, whichever of
the four sources it came from: absolute, present, a directory, canonical,
not the filesystem root, and readable. It answers with a sentence somebody
can act on rather than a code, because that sentence is what the browser
shows. A named path that fails validation does not fall through to the
next candidate; somebody said to work there, and quietly working somewhere
else is the same failure as guessing.

**The path is persisted and the API key is not,** which is not an
inconsistency. A key is a credential and the settings file is not a
keychain, so `PersistedSettings` has no field that could carry one. A
workspace path is a directory name: somebody reading the file learns where
their own work lives, which they already knew. Not saving it would mean
choosing a directory again after every restart, and the state that pushes
people towards is the one this decision exists to remove.

**`<workspace>/scratch` is where generated files go.** The turn's system
prompt says so in one plain sentence, added to the shared identity prompt
by `turn::turn_prompt` rather than to `DEFAULT_SYSTEM_PROMPT`, because the
CLI has no such rule. The directory is created when a turn starts, not at
startup, since a workspace can be chosen while the server is running, and
a failure to create it is said out loud and never fails the turn. No new
tool was invented for it: the agent already writes files.

**`GET /api/workspace/browse` lists directory names, and that is all.**
Subdirectories of one directory, sorted case insensitively, dotted ones
left out unless `hidden=1` is passed, symlinked directories included. It
never returns a file name, never a file's contents, and never walks
anywhere but the directory it was asked for. A relative path is 400, a
missing one 404, an unreadable one 403. It is exactly as exposed as the
shell the agent already runs on this machine, which is why this server
refuses a non-loopback bind without a token; on a loopback install it
tells the person running it what is on their own disk.

**Ruled out:** falling back to the current directory when nothing is set
(the bug); a default workspace under the user's home (a directory nobody
chose is the same problem with a nicer address); making the workspace a
field on `PUT /api/settings` (it would bypass the validation the dedicated
endpoint does); changing the workspace while a run is in flight (409: the
agent is working in the old directory).
