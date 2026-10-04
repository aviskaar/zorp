---
status: accepted
date: 2026-09-18
---

# a small model is offered the plain HTML skill, and the rule says so

**Decision:** the authoring skills are two. `landing-page` writes one
self-contained HTML file with no framework and no build step, as before.
`.claude/skills/react-components` writes the same kind of page as React
components in a Vite project the person builds, and it sits on top of
`landing-page` rather than repeating it: the copy, structure, markup and
accessibility rules stay in one file and the component skill covers only
what changes. `zorp-agent/src/skill_routing.rs` decides which of them a
model is offered, and the `skill` tool is built from that subset.

**At the index, not at load time.** The tool's description is in the
request on every turn. A skill left out of it costs nothing: not the index
line each turn, not the body the model might have loaded, and not an entry
a small model would have to be trusted to decline. So the component skill
is absent from a small model's description, from the schema's `enum`, and
from the lookup behind `run`, where asking for it by name is the same
"no skill named" error an invented name gets. Discovery is unchanged:
`SkillRegistry::filtered` is a view over what was found, and every listing
a person reads still shows every skill on disk.

**The rule, in code, with no model call.** Onboarding already settled that
a classifier would be a model call spending someone's money to decide how
to spend it. Two facts are read instead, because the parameter count does
not exist anywhere: providers do not send it and `ModelDetail` has no such
field.

- The size written in the id: a number followed by `b` (or `m`) standing
  between separators, so `qwen2.5:7b`, `gemma2:2b`, `phi3:3.8b`,
  `llama-3.3-70b-instruct`, `mixtral:8x7b` as the product and Gemma's
  `e4b`. A number has to start at a boundary and is consumed whole, so the
  `2.5` in `qwen2.5` and the `8` in `3.8b` are never sizes. Under 14B the
  component skill is withheld. The line sits below the 14B tier, the
  smallest where a multi-file component tree is a reasonable ask.
- The context window, when anybody has stated one: `ZORP_CONTEXT_TOKENS`,
  or the `context_length` a provider's listing gave for that endpoint and
  id, which `zorp-web` now remembers in memory from the last `/api/models`
  call and never writes to the settings file. The smaller of the two wins.
  Under 16,384 tokens the component skill is withheld: a turn that uses it
  loads both authoring bodies, around five thousand tokens, and in an 8K
  window that is most of the request.

**Unknown means the full set.** Ids that encode no size, `gpt-4o`,
`claude-opus-5`, `mistral-nemo`, are overwhelmingly the large hosted
models, since it is Ollama style tags that carry `:7b`. Defaulting unknown
to small would downgrade nearly every frontier model, which is a more
common and more annoying failure than occasionally offering the component
skill to a model that cannot use it.

**The override is an environment variable.** `ZORP_SKILL_TIER=full`
offers everything to every model and `ZORP_SKILL_TIER=plain` withholds the
component skill from every model; unset or `auto` lets the rule decide. It
is read live, like `ZORP_CONTEXT_TOKENS`, which is the knob it most
resembles. A settings field was the alternative and was not taken: the
settings file holds what model to talk to, and a UI control would be a
second place to state what the environment already can. An unrecognised
value is not silently dropped: the rule decides and the reason says the
value was ignored.

**The rule is visible.** The same precedent onboarding set: the rule is
printed beside the choice it made. `/api/skills` and
`/api/sessions/:id/skills/active` both carry an `offer` with the model, the
tier, the setting, the rule's sentence and the withheld names, the second
resolved with the conversation's agent applied since an agent can name its
own model. The skills panel dims a withheld skill, says "not offered to this
model" beside it, and prints the sentence above the list. The CLI prints it
on stderr when the tool is registered, and the chat REPL's `/skills` shows
the offered index with the sentence under it. Every surface calls the same
`skill_offer` over the same facts, so what a person reads is what the index
was built from.

**Named, not declared.** Which skills are tiered is a const list,
`COMPONENT_SKILLS`, and not a frontmatter field. A field would let any
`SKILL.md` on the machine put itself into a tier, which is a capability
system, and the issue ruled out anything beyond this one split. A test pins
that every name in the list is a skill this repository ships, so a rename
cannot quietly turn routing off.

**The trust boundary does not move.** An offered skill is still untrusted
text that grants no tool and bypasses no denylist entry, and a withheld one
is simply not in the list. Switching models, or suggesting one, is out of
scope.
