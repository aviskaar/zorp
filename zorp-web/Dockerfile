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
COPY erbga ./erbga
RUN cargo build --release --locked -p zorp-web

FROM debian:12-slim
# ca-certificates is required: the agent calls an OpenAI-compatible endpoint
# over TLS and without it every model call fails certificate verification
# with something that reads like a network fault.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && useradd -m -u 1000 zorp
COPY --from=build /src/target/release/zorp-web /usr/local/bin/zorp-web
USER zorp
WORKDIR /work
EXPOSE 7777
# 0.0.0.0 is required to be reachable from outside the container, so a token
# is mandatory. The server refuses to start without one on a non-loopback
# bind, which is the behavior we want here rather than something to work
# around.
ENV ZORP_WEB_TOKEN=""
ENTRYPOINT ["/bin/sh", "-c", "exec zorp-web --bind 0.0.0.0 --port 7777 --token \"$ZORP_WEB_TOKEN\""]
