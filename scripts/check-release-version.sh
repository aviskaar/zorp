#!/bin/sh
# Refuse a release whose tag, workspace version and Dockerfile default
# disagree.
#
#   check-release-version.sh <tag> [dir]
#
# Run by the Release workflow on a tag push, and by
# tests/release_version_check.rs on every pull request. It lives in a script
# rather than inline in the workflow so that its logic is testable: a check
# that only ever executes during a release is a check nobody has run.
#
# The drift is not hypothetical. v0.3.0 and v0.3.1 both shipped a binary
# answering `--version` with 0.2.1, and a bare `docker build` fetched the
# v0.2.1 tarball on purpose, because both defaults were left behind. v0.2.1
# is the release whose first message times out on a cold model, so everyone
# who installed the fix and checked was told they had not.
set -u

tag="${1:-}"
dir="${2:-.}"

if [ -z "$tag" ]; then
    echo "usage: check-release-version.sh <tag> [dir]" >&2
    exit 2
fi

cargo_file="$dir/Cargo.toml"
docker_file="$dir/Dockerfile"
for f in "$cargo_file" "$docker_file"; do
    if [ ! -f "$f" ]; then
        echo "missing: $f" >&2
        exit 2
    fi
done

# Read the version out of [workspace.package], the one every product crate
# inherits. Scoped to that table on purpose: the root [package] block sits
# above it, so a plain `grep '^version'` finds the wrong line.
cargo_version="v$(sed -n '/^\[workspace.package\]/,/^\[/p' "$cargo_file" |
    sed -n 's/^version = "\(.*\)"$/\1/p')"
docker_default="$(sed -n 's/^ARG VERSION=\(.*\)$/\1/p' "$docker_file")"

echo "tag=$tag cargo=$cargo_version dockerfile=$docker_default"

# Both are checked before exiting. Blocking a release twice in a row over
# one problem at a time is how people learn to reach for --no-verify.
fail=0
if [ "$tag" != "$cargo_version" ]; then
    echo "Cargo.toml: [workspace.package] version is '${cargo_version#v}'" \
        "but this release is '$tag'. Bump it, or the published binary" \
        "reports the wrong version to everyone who checks." >&2
    fail=1
fi
if [ "$tag" != "$docker_default" ]; then
    echo "Dockerfile: ARG VERSION is '$docker_default' but this release is" \
        "'$tag'. Bump it, or a bare docker build ships the old binary." >&2
    fail=1
fi

exit $fail
