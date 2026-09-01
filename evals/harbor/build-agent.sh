#!/usr/bin/env bash
# Build a Linux zorp-agent for the Harbor adapter to upload into task
# containers. The adapter benchmarks the binary this produces, so it is
# built from the working tree and not downloaded from a release.
#
# Usage:
#   evals/harbor/build-agent.sh [platform]
#
# platform defaults to the Docker daemon's own platform, which is what
# Harbor's task containers will run under unless you ask for something
# else. Pass linux/amd64 explicitly if a dataset ships amd64-only images.
#
# Output: target/harbor/<arch>/zorp-agent
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

platform="${1:-}"
if [ -z "$platform" ]; then
  platform="linux/$(docker version --format '{{.Server.Arch}}')"
fi
arch="${platform##*/}"

out_dir="$repo_root/target/harbor/$arch"
mkdir -p "$out_dir"

# A separate CARGO_TARGET_DIR keeps Linux objects out of the host's target/.
# Named volumes keep the registry and that target dir warm between runs, so a
# rebuild after an edit is incremental rather than a fresh 10 minute compile.
docker run --rm \
  --platform "$platform" \
  -v "$repo_root:/src:ro" \
  -v "zorp-harbor-target-$arch:/build" \
  -v "zorp-harbor-cargo-$arch:/usr/local/cargo/registry" \
  -w /src \
  -e CARGO_TARGET_DIR=/build \
  rust:slim-bookworm \
  sh -c 'cargo build --release --locked -p zorp-agent && cp /build/release/zorp-agent /build/zorp-agent.out'

# The build wrote inside the volume, so copy it out through a throwaway container.
container="$(docker create --platform "$platform" -v "zorp-harbor-target-$arch:/build" rust:slim-bookworm true)"
docker cp "$container:/build/zorp-agent.out" "$out_dir/zorp-agent"
docker rm -f "$container" >/dev/null
chmod +x "$out_dir/zorp-agent"

echo "built $out_dir/zorp-agent"
file "$out_dir/zorp-agent" 2>/dev/null || true
