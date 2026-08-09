# zorp

**A research agent for scientific discovery.**

zorp is an agent harness aimed at autonomous scientific research: forming
hypotheses, running experiments, evaluating results, and writing up
findings — with the goal of publishing its own output as a paper. It is
part of [Aviskaar](https://github.com/aviskaar)'s applied AI research
suite, and will get a dedicated home at **zorp.dev**.

> **Status: early / pre-alpha.** The base execution harness is in place;
> the research-agent capabilities (experiment tree search, autonomous
> hypothesis-to-paper loop, etc.) are still being designed.

## Origins

zorp's execution layer is built on top of
[quecto](https://github.com/adityak74/quecto), a minimal, vendor-neutral
harness for LLM agents (MIT licensed) — see `NOTICE.md` for attribution.
We're modifying and extending it directly rather than depending on it as
an external crate, since zorp's needs (long-running research loops,
experiment tracking, paper synthesis) diverge substantially from a general
agent harness.

## Repo layout

```
.
├── src/                # zorp binary crate (was quecto's root crate)
├── quecto-agent/       # coding/reasoning agent crate
├── quecto-mcp/         # MCP integration crate
├── quecto-eval/        # eval harness crate
├── evals/              # eval suites
├── examples/           # usage examples
├── docs/               # docs, specs, upstream references
│   └── paper/          # arXiv writeup (WIP)
└── reference/           # gitignored — local-only inspiration material
```

## Building

```bash
cargo build --workspace
```

See `docs/UPSTREAM_QUECTO_README.md` for the underlying harness's original
documentation until zorp-specific docs land.

## License

MIT — see `LICENSE` and `NOTICE.md`.
