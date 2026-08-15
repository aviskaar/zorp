# Notice

zorp is built on top of [quecto](https://github.com/adityak74/quecto), a
minimal, vendor-neutral execution layer for LLM agents by
[@adityak74](https://github.com/adityak74), used under the MIT License (see
`LICENSE`). quecto's original README is preserved at
`docs/UPSTREAM_QUECTO_README.md` for reference.

zorp is not affiliated with or endorsed by the quecto project.

## Reference material (not distributed)

`reference/` (gitignored, not committed) may contain local checkouts of
other projects used purely for design inspiration while building zorp,
such as [AI-Scientist-v2](https://github.com/SakanaAI/AI-Scientist-v2) by
Sakana AI, which is licensed under a custom, restrictive "Responsible AI
Source Code License." No code from `reference/` is copied into zorp. It is
consulted for ideas only and never redistributed as part of this repo.

## Benchmark datasets (redistributed)

`erbga/tests/data/` holds four network datasets, committed so the
benchmark reproduces without a network fetch. They are other people's
data, not zorp's, and each file repeats its attribution in its own
header:

- `karate.edges`, Zachary's karate club. W. W. Zachary, "An information
  flow model for conflict and fission in small groups", Journal of
  Anthropological Research 33, 452-473 (1977). Taken from the copy
  bundled with networkx.
- `dolphins.edges`, the Doubtful Sound dolphin social network.
  D. Lusseau, K. Schneider, O. J. Boisseau, P. Haase, E. Slooten, and
  S. M. Dawson, Behavioral Ecology and Sociobiology 54, 396-405 (2003).
- `polbooks.edges`, books about US politics. Compiled by V. Krebs,
  unpublished.
- `football.edges`, US college football games, Fall 2000. M. Girvan and
  M. E. J. Newman, "Community structure in social and biological
  networks", PNAS 99, 7821-7826 (2002).

The last three come from M. E. J. Newman's network data collection at
http://websites.umich.edu/~mejn/netdata/, which asks that the original
work be cited rather than the collection. `erbga/tests/data/fetch.py`
regenerates all four from source and verifies the vertex and edge counts
before writing.
