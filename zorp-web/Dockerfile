# The zorp web server.
#
# Built from source rather than from a release artifact, because zorp-web is
# not part of the binary release and the image should track the branch it is
# built from.
# Stable, not the declared MSRV: the lockfile has resolved dependencies a
# 1.82 cargo cannot parse (clap_lex 1.1.0), and CI builds on stable too.
#
# bookworm specifically, matching the debian:12-slim runtime below. Plain
# `rust:slim` tracks a newer Debian, and a binary linked against its glibc
# will not start on Debian 12: "GLIBC_2.39 not found". The release workflow
# hit the same thing for the same reason.
FROM rust:slim-bookworm AS build
WORKDIR /src
# rusqlite bundles SQLite, which compiles C.
RUN apt-get update \
 && apt-get install -y --no-install-recommends pkg-config libssl-dev build-essential \
 && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY zorp-agent ./zorp-agent
COPY zorp-mcp ./zorp-mcp
COPY zorp-eval ./zorp-eval
COPY zorp-track ./zorp-track
COPY zorp-web ./zorp-web
# Every workspace member has to be here or cargo cannot parse the workspace,
# even the ones this binary does not use. These four arrived after the file
# was first written and the build had been failing on the missing manifests.
COPY zorp-search ./zorp-search
COPY zorp-skill ./zorp-skill
COPY zorp-recall ./zorp-recall
COPY zorp-voice ./zorp-voice
COPY erbga ./erbga
# voice and recall are both non-default. Without them the microphone and the
# conversation search are compiled out and the endpoints answer 501, which
# looks like a broken image rather than a deliberate build. Overridable
# because a deploy with no local Ollama sidecar (recall's embedder must stay
# loopback, so it has nothing to reach) has no use for either: pass
# `--build-arg FEATURES=` for that build.
ARG FEATURES=voice,recall
RUN if [ -n "$FEATURES" ]; then \
      cargo build --release --locked -p zorp-web --features "$FEATURES"; \
    else \
      cargo build --release --locked -p zorp-web; \
    fi

FROM debian:12-slim
# ca-certificates is required: the agent calls an OpenAI-compatible endpoint
# over TLS and without it every model call fails certificate verification
# with something that reads like a network fault.
#
# python3 and its venv and pip are here for zorp-voice, which installs the
# Qwen3-ASR runtime into a marked virtual environment on first use rather
# than baking several gigabytes of model and torch into this image.
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      ca-certificates python3 python3-venv python3-pip \
 && rm -rf /var/lib/apt/lists/* \
 && useradd -m -u 1000 zorp
COPY --from=build /src/target/release/zorp-web /usr/local/bin/zorp-web
# Made here so the named volumes mount onto paths that already exist and are
# owned by uid 1000. Left to Docker to create, they arrive owned by root and
# the server cannot write its databases or set the voice runtime up.
RUN mkdir -p /home/zorp/state /home/zorp/voice \
 && chown -R zorp:zorp /home/zorp/state /home/zorp/voice
# zorp-voice refuses to set itself up as root on purpose, so this is not just
# good practice here: as root the microphone would not work at all.
USER zorp
WORKDIR /work
EXPOSE 7777
# 0.0.0.0 is required to be reachable from outside the container, so a token
# is mandatory. The server refuses to start without one on a non-loopback
# bind, which is the behavior we want here rather than something to work
# around.
ENV ZORP_WEB_TOKEN=""
# Space-separated origins, e.g. "https://app.zorp.dev". Empty by default, which
# reproduces the old fixed command exactly: no flag added, nothing allowed,
# same as every deploy before this existed (the compose.yml `ui` service talks
# same-origin through nginx and never needs this).
ENV ZORP_ALLOW_ORIGIN=""
ENTRYPOINT ["/bin/sh", "-c", "set -- --bind 0.0.0.0 --port 7777 --token \"$ZORP_WEB_TOKEN\"; for o in $ZORP_ALLOW_ORIGIN; do set -- \"$@\" --allow-origin \"$o\"; done; exec zorp-web \"$@\""]
