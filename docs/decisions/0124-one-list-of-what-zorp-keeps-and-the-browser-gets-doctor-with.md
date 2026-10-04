---
status: accepted
date: 2026-09-13
---

# one list of what zorp keeps, and the browser gets doctor without the wait

**Decision:** `zorp-agent/src/state.rs` is the one place that knows which
files zorp keeps on this machine and how to remove each one. `GET /api/data`,
`DELETE /api/sessions`, `DELETE /api/recall/index`, `DELETE /api/settings`,
`GET /api/mcp` and `GET /api/doctor` are adapters over it and over
`doctor::`, and `zorp-agent data` is the terminal's.

**Why the list is in `zorp-agent`.** Five files written by four different
parts of the program, and until now the only way to find out what was held
was to know where to look. Both surfaces keep the same state in the same
place, so a second implementation of "delete everything" is a second chance
to delete the wrong thing. `zorp-agent data` came free from putting it
there, which is what #217 asked for and did not have to re-open.

**A settings reset takes the settings.** It removes `zorp.toml` and the
trust file and nothing else. Not the conversations, not the input history,
and above all not a workspace file: `<workspace>/scratch` is the person's,
and a reset that reached into it would be the one action nobody could undo.
There is no code path in `state.rs` that can. The test writes a file into a
workspace and a conversation into the store and asserts both survive.

**It cannot unset `ZORP_API_KEY` and says so every time.** A process does
not own the environment it was started in. `RESET_LEAVES_THE_KEY` is a
constant rather than a comment because the sentence has to be in front of
whoever clicks the button: somebody resetting settings is usually trying to
get rid of a credential, and this is the one that survives.

**The MCP listing carries key names and never values.** `env` and `headers`
are where a token goes, and a settings page is exactly where somebody
screenshots. `ServerConfig::redacted` lives in `zorp-mcp` beside the type it
protects, and its destructuring is exhaustive on purpose: adding a field
stops it compiling, which forces a decision about whether the new field may
be shown. A version that read fields through `self.` would silently omit a
new one, and the failure mode of a redaction that silently omits is that
somebody adds `token: String` and nothing notices.

**The listing says `zorp-web` loads none of them.** The `mcp` feature is on
`zorp-agent` and this binary does not forward it, so a browser turn gets no
MCP tools whatever the file says. `loads_servers` is false at the top and
`loaded` is false on every row. Showing configured servers without that
would read as "these are working", which is the opposite of true. The field
exists so it can stop being always false without the page changing shape.

**`GET /api/doctor` does not probe unless asked, which is the one place it
differs from the CLI.** `zorp-agent doctor` calls the endpoint because
somebody ran a command and expects it to take a moment. This is opened by
clicking a pill, and `probe_completion` waits up to thirty seconds on the
read. Probing by default would mean a settings pane that hangs on a slow
endpoint. Without `?probe=1` the row says it was not checked rather than
guessing, and `POST /api/settings/test` is still there for an explicit one.

**Nothing here is reachable by a model.** No tool clears data, resets
settings or writes MCP configuration, and `agent.rs` has
`no_tool_clears_state_or_resets_settings` naming each one. A running turn is
refused with the 409 `delete_session` already gives, in the same words, so
the two do not read as different problems.
