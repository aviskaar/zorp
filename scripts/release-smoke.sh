#!/bin/sh
# Smoke test a built release, with real output, before it is trusted.
#
#   release-smoke.sh <bindir> <tag>
#
# <bindir> holds the built `zorp`, `zorp-agent` and `zorp-web` binaries (plus
# `.exe` on Windows). <tag> is the release tag, e.g. v0.8.0.
#
# Checks, in order, each one failing the script loudly:
#   1. zorp-agent prints the tag's version. A release that reports the wrong
#      version is the drift check-release-version.sh exists to stop, seen from
#      the other side; this proves the built binary agrees.
#   2. zorp-agent --help runs.
#   3. zorp-web starts, answers GET /api/health with a healthy body, and stops.
#   4. A simple model interaction, only when a provider is configured (see the
#      note below). It is never faked: with no provider it is reported skipped,
#      because a release smoke test that invented a model reply would be the
#      one result worth nothing.
#
# The web check binds loopback and never leaves the machine. Nothing here
# needs a key unless step 4's provider is configured by the caller.
set -eu

bindir="${1:-}"
tag="${2:-}"
if [ -z "$bindir" ] || [ -z "$tag" ]; then
    echo "usage: release-smoke.sh <bindir> <tag>" >&2
    exit 2
fi

exe=""
case "${OS:-}" in
    Windows_NT) exe=".exe" ;;
esac
core="$bindir/zorp$exe"
agent="$bindir/zorp-agent$exe"
web="$bindir/zorp-web$exe"

fail() {
    echo "release smoke FAILED: $1" >&2
    exit 1
}

# A real release tag is `v<digit>...`; check-release-version.sh guarantees the
# binary's version then equals the tag, so assert it. A workflow_dispatch
# rehearsal passes a branch ref instead (e.g. `main`), which no binary reports,
# so there require only that a version prints, the way default-version skips
# its check off a tag. Never compare a binary's version to a branch name.
strict=0
case "$tag" in
    v[0-9]*) strict=1 ;;
esac

# Check one binary prints a version, and (on a real tag) that it is the tag's.
# `zorp 0.8.0` -> field 2 is the number, compared exactly so v0.8.1 does not
# accept 0.8.10.
check_version() {
    name="$1"
    bin="$2"
    got="$("$bin" --version 2>&1 || true)"
    echo "$name: $got"
    # shellcheck disable=SC2086
    set -- $got
    v="${2:-}"
    [ -n "$v" ] || fail "$name printed no version"
    if [ "$strict" = "1" ] && [ "$v" != "${tag#v}" ]; then
        fail "$name version '$v' is not '${tag#v}'"
    fi
}

echo "== 1. the shipped binaries report a version =="
check_version zorp "$core"
check_version zorp-agent "$agent"

echo "== 2. --help runs on each binary =="
"$core" --help >/dev/null || fail "zorp --help exited non-zero"
"$agent" --help >/dev/null || fail "zorp-agent --help exited non-zero"

echo "== 3. zorp-web answers /api/health =="
# A port nothing else on a clean runner uses. zorp-web's CLI binds this port
# and exits with a Bind error if it is taken (unlike the desktop bridge, it
# has no ephemeral fallback), so if the server is not alive shortly after
# launch, the cause is almost certainly the port, and this says so rather
# than spending the whole poll curling a dead socket.
port=7788
log="${TMPDIR:-/tmp}/zorp-web-smoke.log"
"$web" --port "$port" >"$log" 2>&1 &
web_pid=$!
health=""
i=0
while [ "$i" -lt 30 ]; do
    if ! kill -0 "$web_pid" 2>/dev/null; then
        echo "--- zorp-web log ---"; cat "$log" 2>/dev/null || true
        fail "zorp-web exited before answering (port $port already in use?)"
    fi
    if health="$(curl -fsS "http://127.0.0.1:$port/api/health" 2>/dev/null)"; then
        break
    fi
    i=$((i + 1))
    sleep 1
done
# Stop the server without blocking: a plain `wait` on a native Windows process
# that `kill` did not actually stop (MSYS) would hang until the job timeout.
kill "$web_pid" 2>/dev/null || true
echo "health: ${health:-<no response>}"
case "$health" in
    *'"status"'*'"ok"'*) : ;;
    *) fail "zorp-web /api/health did not report ok (log in $log)" ;;
esac

echo "== 4. a simple model interaction =="
# Only run when the caller has configured a provider, because a real turn
# needs a model endpoint. ZORP_SMOKE_PROVIDER=1 is the caller saying "I have
# set ZORP_API_KEY and ZORP_BASE_URL, go". With nothing set this is reported
# skipped rather than faked.
if [ "${ZORP_SMOKE_PROVIDER:-}" = "1" ]; then
    out="$("$agent" --yes "reply with the single word: ok" 2>&1 || true)"
    echo "$out" | tail -3
    # Match `ok` as a whole word, not as a substring, so diagnostics
    # containing "looking", "token" and the like do not pass for a reply.
    if printf '%s' "$out" | grep -iqw ok; then
        echo "model interaction: ok"
    else
        fail "model interaction produced no recognizable reply"
    fi
else
    echo "model interaction: SKIPPED (no provider configured; set"
    echo "ZORP_SMOKE_PROVIDER=1 with ZORP_API_KEY and ZORP_BASE_URL to run it)"
fi

echo "release smoke OK for $tag"
