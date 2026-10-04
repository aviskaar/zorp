---
status: accepted
date: 2026-09-01
---

# first run is a guided front door onto the settings that exist, and free means the provider said zero

**Decision:** `zorp-web` shows a first-run flow when the server reports
that nothing has ever been configured: a start page, a provider, a key
when that provider needs one, a model, and one check that it answers. It
is a client for `GET`/`PUT /api/settings`, `POST /api/settings/models`
and `POST /api/settings/test`, the same endpoints the settings panel
behind the model badge already uses. It holds no settings of its own and
adds no endpoint. OpenRouter joins the preset table
(`https://openrouter.ai/api/v1`, OpenAI wire format, needs a key), which
moved out of `main.ts` into `web/src/providers.ts` so the panel and the
flow read one table rather than two copies.

**First run is read, not remembered.** `isFirstRun` is
`has_api_key === false` with `provider_source`, `base_url_source` and
`model_source` all `default`, which is the 2026-08-17 provenance saying
no save and no `ZORP_*` var gave any field a value. An operator who
exported `ZORP_BASE_URL` reports `env` and is never shown setup for work
they already did. A dismissal is remembered in `localStorage` and every
way out remembers it, so this asks once.

**Free vs paid is decided on the stated price, and the backend now
carries it.** `ModelsResult` gained `details`: the same models in the
same order, each with the optional `name`, `context_length`,
`prompt_price` and `completion_price` the listing gave. `models` is
unchanged and every existing caller still reads it, the endpoint is
still always 200, and a provider that says only an id (Ollama, oMLX,
most OpenAI-compatible servers) produces details with no prices, which
the UI draws as one unsorted list rather than as "free".

The alternative was detecting OpenRouter's `:free` id suffix in the
browser and shipping no Rust at all. It was rejected because the suffix
is a convention and the price is the fact. A convention can be adopted
by a model that is not free, dropped by one that is, and it exists on
exactly one provider, so the browser would have been reading a naming
habit and telling somebody it had read a price. Telling a person a model
costs nothing is the one claim on that screen that can cost them money
when it is wrong, so it is made from the number the provider stated or
it is not made at all. That is also why there are three classes and not
two: a missing price is `unstated`, and so is a negative one, which is
what OpenRouter states for a model whose cost is decided per request.

**AutoRouter is offered as what it is.** `openrouter/auto` is
OpenRouter's own router, it routes across everything including paid
models, and it is labelled saying so. It is offered only when the
listing actually named it, so a withdrawn id is never presented as
selectable. Beside it is a free-only automatic choice picked in code
from the listing that came back: of the models priced at zero that the
listing shows answering with text and nothing else, the one with the
largest context window, ties broken by id. The rule is printed next to
the choice and the concrete model id is what gets saved, not the rule.

**The modality half of that rule is not tidiness.** Run against the real
listing, the first version handed a first-time user
`google/lyria-3-clip-preview`, a music model: several free models tie at
the largest context window OpenRouter publishes and the id tiebreak
picked that one. So `ModelDetail` also carries `output_modalities`, and
the pick skips anything the listing does not show answering with text
alone. "Outputs text" was tried first and was not enough, because that
model lists `["text", "audio"]`. A model that states no modalities still
passes, the same way an unstated price is not a price of zero. The
guard is on the automatic pick only. The grouped lists show what the
provider serves, because choosing from those is the person's to do, and
this is the one choice zorp makes on their behalf.

**Ruled out: a task classifier.** No model is asked which model suits a
question, here or anywhere in this flow. It would be a model call the
person did not ask for, spending their money to decide how to spend
their money, and it would produce a recommendation nobody could check.

Everything the flow draws from a listing goes on the page through
`textContent`. A model id and a model name are remote text zorp did not
write, and `web/src/onboarding.ts` has no `innerHTML` and must never
grow one. The flow reads and writes settings and nothing else: it cannot
start a turn, a panel or an investigate run.
