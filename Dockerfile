# Runtime image for the two zorp binaries.
#
# Deliberately not a build image. The release workflow already produces
# binaries for four targets, and rebuilding them here would mean compiling
# the workspace again on every image push for no benefit. This copies the
# published artifact in.
#
# Build for a released version:
#   docker build --build-arg VERSION=v0.3.1 -t zorp .
#
# VERSION defaults to the current release and must be bumped with each one.
# It is not cosmetic: a stale default means a bare `docker build` silently
# produces an old binary. It sat at v0.2.1 through the v0.3.1 release, which
# handed anyone who built without the flag the one release whose first
# message times out on a cold model. The `default-version` check in the
# Release workflow now fails the release if this drifts again.
#
# The default feature set only. The research capabilities (validate,
# investigate, co-write, deliver) are behind the `research` feature and need
# a source build, because zorp-track bundles DuckDB.
FROM debian:12-slim

ARG VERSION=v0.3.1
ARG TARGETARCH

# ca-certificates is required: zorp talks to an OpenAI-compatible endpoint
# over TLS and would otherwise fail certificate verification with a message
# that looks like a network fault.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl \
 && rm -rf /var/lib/apt/lists/*

# Debian 12 ships glibc 2.36 and the release binaries are built against
# 2.35, so the gnu targets run here without a musl build.
RUN set -eux; \
    case "${TARGETARCH}" in \
      amd64) target="x86_64-unknown-linux-gnu" ;; \
      arm64) target="aarch64-unknown-linux-gnu" ;; \
      *) echo "unsupported architecture: ${TARGETARCH}" >&2; exit 1 ;; \
    esac; \
    base="zorp-${VERSION}-${target}"; \
    url="https://github.com/aviskaar/zorp/releases/download/${VERSION}/${base}.tar.gz"; \
    curl -fsSL "$url" -o /tmp/z.tar.gz; \
    curl -fsSL "$url.sha256" -o /tmp/z.sha256; \
    cd /tmp; \
    awk '{print $1"  /tmp/z.tar.gz"}' /tmp/z.sha256 | sha256sum -c -; \
    tar -xzf /tmp/z.tar.gz -C /tmp; \
    install -m 755 "/tmp/${base}/zorp" /usr/local/bin/zorp; \
    install -m 755 "/tmp/${base}/zorp-agent" /usr/local/bin/zorp-agent; \
    rm -rf /tmp/z.tar.gz /tmp/z.sha256 "/tmp/${base}"; \
    zorp-agent --version

# The agent reads and writes files in the working directory, so a caller
# mounts their project here. Running as a non-root user means files it
# creates are not owned by root on the host.
RUN useradd -m -u 1000 zorp
USER zorp
WORKDIR /work

ENTRYPOINT ["zorp-agent"]
CMD ["--help"]
