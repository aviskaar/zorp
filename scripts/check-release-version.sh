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
#
# Member crates that opt out of version.workspace = true are also invisible
# to a tag/workspace/Dockerfile-only check. zorp-search once shipped at
# 0.1.0 while the rest of the product was 0.3.2. After reading the workspace
# version, this script walks each workspace member and requires it to inherit
# unless it is on the deliberate exemption list (erbga today).
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

# Product crates must inherit the workspace version. Paths listed here are
# deliberate exemptions (standalone prior work that versions on its own terms).
# Keep this list short and documented in CLAUDE.md / docs/DECISIONS.md.
is_exempt() {
    case "$1" in
        erbga|./erbga|erbga/|./erbga/) return 0 ;;
        *) return 1 ;;
    esac
}

# members = [".", "zorp-agent", ...] — flatten onto one line then split.
members_line=$(sed -n 's/^members = \[\(.*\)\]$/\1/p' "$cargo_file" | head -n 1)
# Fallback: multi-line members tables are not used today; fail closed if missing.
if [ -z "$members_line" ]; then
    echo "Cargo.toml: could not read workspace members = [...]" >&2
    fail=1
else
    # shellcheck disable=SC2086
    old_ifs=$IFS
    IFS=','
    set -- $members_line
    IFS=$old_ifs
    for raw in "$@"; do
        # strip quotes and whitespace
        m=$(printf '%s' "$raw" | sed 's/^[[:space:]]*"//; s/"[[:space:]]*$//; s/^[[:space:]]*//; s/[[:space:]]*$//')
        [ -n "$m" ] || continue
        if [ "$m" = "." ]; then
            member_toml="$cargo_file"
            label="."
        else
            member_toml="$dir/$m/Cargo.toml"
            label="$m"
        fi
        if is_exempt "$label"; then
            echo "member=$label exempt"
            continue
        fi
        if [ ! -f "$member_toml" ]; then
            echo "member $label: missing $member_toml" >&2
            fail=1
            continue
        fi
        # Require version.workspace = true in the [package] table (before the
        # next [section]). An explicit version = "..." is the failure mode.
        pkg_block=$(sed -n '/^\[package\]/,/^\[/p' "$member_toml")
        if printf '%s\n' "$pkg_block" | grep -q '^version\.workspace = true'; then
            echo "member=$label inherits"
            continue
        fi
        explicit=$(printf '%s\n' "$pkg_block" | sed -n 's/^version = "\(.*\)"$/\1/p' | head -n 1)
        if [ -n "$explicit" ]; then
            echo "member $label: has version = \"$explicit\" instead of" \
                "version.workspace = true. Product crates share one version;" \
                "only listed exemptions may pin their own." >&2
            fail=1
        else
            echo "member $label: no version.workspace = true in [package]" >&2
            fail=1
        fi
    done
fi

exit $fail