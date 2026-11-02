#!/bin/sh
# Emit a per-build feature manifest as JSON.
#
#   feature-manifest.sh <version> <target> [feature,feature,...]
#
# The third argument is the exact comma-separated feature set the build was
# compiled with, empty for a default-feature build. The workflow that builds
# an artifact knows this for certain (it is what it passed after --features),
# so the manifest is the ground truth for which capability a build carries,
# not a guess read back from one binary that cannot see another crate's
# features.
#
# The point of this file is #303: RL Studio, speech output and the rest are
# gated differently, and a download gave no machine-readable way to tell which
# build had which. A release publishes one of these per build so the answer is
# an artifact, not prose in the notes.
#
# Output is stable and sorted, so a diff between two releases is readable and
# a test can pin the shape.
set -eu

version="${1:-}"
target="${2:-}"
features="${3:-}"

if [ -z "$version" ] || [ -z "$target" ]; then
    echo "usage: feature-manifest.sh <version> <target> [feature,...]" >&2
    exit 2
fi

# Escape a value for a JSON string: backslash first, then double quote, then
# drop control characters. The real inputs (a vN.N.N tag, a target triple,
# cargo feature names) never need this, but a file sold as machine-readable
# ground truth should not emit invalid JSON for an odd value either.
json_escape() {
    printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g' | tr -d '\000-\037'
}

# Turn the comma-separated list into a sorted JSON array of strings, dropping
# empty entries so a trailing comma or an empty argument yields [], and
# escaping each entry.
features_json="$(
    printf '%s' "$features" |
        tr ',' '\n' |
        sed 's/^[[:space:]]*//; s/[[:space:]]*$//' |
        grep -v '^$' |
        sort -u |
        while IFS= read -r f; do
            printf '%s\n' "$(json_escape "$f")"
        done |
        awk 'BEGIN { printf "[" }
             { printf "%s\"%s\"", (NR>1 ? "," : ""), $0 }
             END { printf "]" }'
)"

printf '{"version":"%s","target":"%s","features":%s}\n' \
    "$(json_escape "$version")" "$(json_escape "$target")" "$features_json"
