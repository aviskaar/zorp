---
status: accepted
date: 2026-08-20
---

# the API answers named origins, and the MSRV is checked

**Decision, part one:** `zorp-web` allows no cross origin caller unless one
is named with `--allow-origin`, repeatable. It was `allow_origin(Any)`.

**Why:** `POST /turn` runs an agent that executes commands and edits files on
the machine the server was started on, and the token gate only arms on a non
loopback bind. On the ordinary install the two together meant any page the
user happened to visit could drive that agent and read back what it produced,
with nothing on screen to show it happening. The permissive header was there
for a real reason, the container split and an `index.html` opened off disk,
but "some other origin needs this" was answered with "every origin may".

The default costs the normal install nothing. When this server serves the UI
the page and the API share an origin and the browser runs no CORS check at
all, which is why the flag is empty by default rather than seeded with
loopback addresses.

`null`, the origin of a `file:` page, is available as `--allow-origin null`
and is deliberately not on by default: `null` is also the origin of a
sandboxed iframe, so allowing it silently would reopen the hole under another
name.

**One implementation note worth keeping.** The origins go to `CorsLayer` as
one list, not one call each. A single value sets a fixed
`Access-Control-Allow-Origin` that goes out whatever the request asked for,
leaving the browser to catch the mismatch; the list form makes the server
compare and answer only for an origin on it. Repeated calls also replace
rather than accumulate. The first version here did it the wrong way and a
test caught it.

**What this does not fix:** an agent with one approved `run_command` can still
`curl` this server's own endpoints, because that is a local process and not a
browser. CORS is not the control for that; a policy rule denying loopback
calls to zorp-web's own port would be, and it is not written yet.

**Decision, part two:** `rust-version` is 1.95, and CI has an `msrv` job that
installs exactly the declared version and builds with it.

**Why:** the manifest said 1.82 and had for a long time. Every CI job used
`stable`, so nothing ever checked it, and the dependency tree had walked to
1.95 without a single build going red: `hashbrown` wants Cargo's
`edition2024` feature (1.85), `globset` and `icu_collections` want 1.88, and
`merman`, a non optional dependency of `zorp-agent`, wants 1.95. The declared
floor was thirteen minor versions below the real one.

The job reads the version out of the manifest rather than hardcoding it, so
the two cannot disagree. Note there is no headroom left: 1.95 is current
stable, so anyone on an older Rust cannot build zorp at all. That is worth
revisiting, and it is a dependency question rather than a code one.
