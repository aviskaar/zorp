# Getting started

Every way to get zorp running: from source, the install script, the Mac app, and Docker. The [README](../README.md) has the short version.

## Build from source

Requires a recent stable Rust toolchain ([rustup.rs](https://rustup.rs)).

```bash
git clone https://github.com/aviskaar/zorp.git
cd zorp
cargo build --workspace --exclude zorp-track
```

> `zorp-track` (the research foundation) bundles DuckDB, which compiles
> from source and takes a while on a cold cache. The command above skips
> it, which is enough for the core `zorp` and `zorp-agent` binaries
> below. Drop `--exclude zorp-track` (plain `cargo build --workspace`,
> or `cargo build --workspace --features research` for `zorp-agent`)
> once you need the `validate`/`investigate`/`co-write`/`deliver`
> capabilities, and budget time for that first build. The LanceDB vector
> library is behind a non-default `library` feature, so the Arrow and
> DataFusion tree is not built unless you ask for it.

Run the core transport directly:

```bash
export ZORP_BASE_URL="https://api.openai.com/v1"   # or a local endpoint (Ollama, LM Studio, vLLM)
export ZORP_API_KEY="sk-..."
export ZORP_MODEL="gpt-4o-mini"
# Optional. Seconds of silence to wait for, default 900. Loading a local model
# into memory can take minutes on modest hardware, and that wait happens
# before the first token. On a streamed reply this bounds the silence between
# chunks, not the length of the answer, so a long reply is never cut off and a
# provider that goes quiet stops being waited on. Exceeding it is an error
# that says so, and so is a stream that ends before the provider says it has
# finished.
export ZORP_HTTP_TIMEOUT_SECS=900
# Optional. A provider that answers 429 or 503 has not taken the request, so
# it is sent again: at most this many times in total, adding at most this many
# seconds of waiting. A Retry-After is waited out in full, and without one the
# wait is exponential backoff with jitter. Nothing else is retried, nothing is
# retried once an answer has started arriving, and every retry says so on
# stderr. Set either to 1 and 0 respectively to turn retrying off.
export ZORP_RETRY_ATTEMPTS=4
export ZORP_RETRY_BUDGET_SECS=30
cargo run -- "Summarize the second law of thermodynamics in one sentence."
```

Or the full agent:

```bash
cargo run -p zorp-agent -- "<task>"
```

## Install without a toolchain

```bash
curl -fsSL https://raw.githubusercontent.com/aviskaar/zorp/main/install.sh | bash
```

This downloads prebuilt `zorp`, `zorp-agent` and `zorp-web` binaries for
your platform from the latest release, verifies the published checksum, and
installs them to `~/.local/bin`. The chat UI's static files go to
`~/.local/share/zorp/web`. No Rust and no Node needed. Linux and macOS,
x86_64 and arm64.

If no prebuilt binary fits your platform, the same script falls back to
building from source, which does need a toolchain. `ZORP_INSTALL_FROM_SOURCE=1`
forces that path, and `ZORP_INSTALL_DIR` changes where the binaries land.

Prebuilt binaries carry the default feature set. The four research
capabilities are behind the `research` feature and still need a source
build, because `zorp-track` bundles DuckDB.

## Mac App (Zorp.app)

A native macOS application packaged as a universal `.dmg` on GitHub releases. It runs the local `zorp-web` server in-process via Tauri v2 and opens the chat UI in its own native window with inset traffic lights, single-instance enforcement, and automatic login shell `PATH` repair.

Download `Zorp_<version>_universal.dmg` from the GitHub Release, open it, and drag `Zorp.app` to `/Applications`.

Because release binaries are ad-hoc signed, macOS Gatekeeper requires one approval on first launch:
- Right-click (or Control-click) `Zorp.app` in `/Applications` and choose **Open**, then click **Open** in the confirmation dialog.
- Or clear the quarantine attribute from your terminal:
  ```bash
  xattr -d com.apple.quarantine /Applications/Zorp.app
  ```

`Zorp.app` shares conversation history and settings with the CLI under `~/.config/zorp` and `~/.local/share/zorp/conversations.db`.

## Docker

Or try it without installing anything:

```bash
docker run --rm -v "$PWD":/work \
  -e ZORP_BASE_URL -e ZORP_MODEL -e ZORP_API_KEY \
  ghcr.io/aviskaar/zorp "<your task>"
```

The image is about 150MB, runs as a non-root user, and mounts your project
at `/work`. `linux/amd64` and `linux/arm64`.

That pull does not work yet. The image is published but the package is
still private, so an anonymous `docker pull` answers `unauthorized`. See
[#30](https://github.com/aviskaar/zorp/issues/30). Until it is flipped,
use the install script above, or build the image yourself with
`docker build -t zorp .`.
