# Contributing to zorp

Thanks for taking a look at zorp. It's early and moving fast, so please
read this before sending a PR.

## Before you start

For anything beyond a small fix (typos, docs, obvious bugs), open an
issue first to discuss the change. This avoids wasted work on PRs that
don't fit the project's direction.

Read [`AGENTS.md`](AGENTS.md) for the tool-agnostic project instructions,
[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for the current approved
architecture and scope, and [`docs/DECISIONS.md`](docs/DECISIONS.md) for
the product and architecture decision log. Check there before proposing
something that's already been decided against.

## Development setup

```bash
git clone https://github.com/aviskaar/zorp.git
cd zorp
cargo build --workspace
```

## Testing

```bash
cargo test --workspace
```

The `research` feature (validate, investigate, co-write, deliver
capabilities) isn't exercised by the default test run. If you touch
`zorp-agent` research-feature code, also run:

```bash
cargo test -p zorp-agent --features research
```

Both must pass before a PR is reviewed.

## Style

- Prose in docs, commit messages, and comments should read plainly: no
  em dashes or en dashes as punctuation, short direct sentences.
- Follow existing code patterns in the crate you're touching. `src/`,
  `zorp-agent/`, `zorp-mcp/`, `zorp-eval/` are inherited harness code;
  `zorp-track/` and the four research capabilities are zorp-specific.
  Keep new zorp-specific work clearly separated from inherited harness
  code.
- No unnecessary abstractions, comments that restate the code, or
  speculative features. Keep changes scoped to what the issue asks for.

## Pull requests

- Keep PRs focused on one change.
- Include or update tests for behavior changes.
- Update relevant docs (`README.md`, `docs/ARCHITECTURE.md`,
  `docs/DECISIONS.md`) when a change affects them.
- CI (build + test) must pass.

## Reporting issues

Open a GitHub issue with steps to reproduce, expected vs. actual
behavior, and your environment (OS, Rust version).
