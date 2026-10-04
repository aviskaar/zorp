---
status: accepted
date: 2026-08-09
---

# research means investigation, not academia, zorp's scope broadens

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
