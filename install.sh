#!/usr/bin/env bash
set -euo pipefail

# Installs `zorp` and `zorp-agent`.
#
# Prefers a prebuilt binary from the latest GitHub release, so installing
# does not require a Rust toolchain. Falls back to building from source when
# no prebuilt binary fits this platform, or when ZORP_INSTALL_FROM_SOURCE=1.
#
# Note: prebuilt binaries carry the default feature set. The research
# capabilities (validate, investigate, co-write, deliver) are behind the
# `research` feature and still need a source build. See README.md.

REPO="${ZORP_REPO:-aviskaar/zorp}"
INSTALL_DIR="${ZORP_INSTALL_DIR:-$HOME/.local/bin}"

say() { printf '%s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

detect_target() {
    local os arch
    os="$(uname -s)"
    arch="$(uname -m)"
    case "$os" in
        Darwin) os="apple-darwin" ;;
        Linux)  os="unknown-linux-gnu" ;;
        *)      return 1 ;;
    esac
    case "$arch" in
        x86_64|amd64) arch="x86_64" ;;
        arm64|aarch64) arch="aarch64" ;;
        *) return 1 ;;
    esac
    printf '%s-%s' "$arch" "$os"
}

build_from_source() {
    command -v cargo >/dev/null 2>&1 || die \
        "no prebuilt binary for this platform and cargo is not installed. Install Rust from https://rustup.rs and re-run."
    say "Building the zorp binaries from source..."
    # Only the two binaries that get installed. Building the whole workspace
    # would also compile zorp-track (bundled DuckDB) and zorp-eval, neither of
    # which is installed.
    if [ -f Cargo.lock ]; then
        cargo build --release --locked -p zorp -p zorp-agent -p zorp-web
    else
        cargo build --release -p zorp -p zorp-agent -p zorp-web
    fi
    BIN_SRC="target/release"
}

download_release() {
    local target="$1" tmp url base
    command -v curl >/dev/null 2>&1 || return 1
    tmp="$(mktemp -d)"
    # Resolve the tag first so the archive name and the download agree even
    # if a new release lands mid-install.
    local tag
    tag="$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" 2>/dev/null \
        | sed -n 's/.*"tag_name":[[:space:]]*"\([^"]*\)".*/\1/p' | head -1)" || return 1
    [ -n "$tag" ] || return 1

    base="zorp-$tag-$target"
    url="https://github.com/$REPO/releases/download/$tag/$base.tar.gz"
    say "Downloading $base..."
    curl -fsSL "$url" -o "$tmp/$base.tar.gz" || return 1

    # Verify when a checksum is published. A silent skip would defeat the
    # point of publishing one, so say which way it went.
    if curl -fsSL "$url.sha256" -o "$tmp/$base.tar.gz.sha256" 2>/dev/null; then
        local expected actual
        expected="$(awk '{print $1}' "$tmp/$base.tar.gz.sha256")"
        if command -v shasum >/dev/null 2>&1; then
            actual="$(shasum -a 256 "$tmp/$base.tar.gz" | awk '{print $1}')"
        elif command -v sha256sum >/dev/null 2>&1; then
            actual="$(sha256sum "$tmp/$base.tar.gz" | awk '{print $1}')"
        else
            actual=""
        fi
        if [ -n "$actual" ]; then
            [ "$expected" = "$actual" ] || die "checksum mismatch for $base.tar.gz"
            say "Checksum verified."
        else
            say "No sha256 tool found; skipping checksum verification."
        fi
    else
        say "No published checksum for this asset; skipping verification."
    fi

    tar -xzf "$tmp/$base.tar.gz" -C "$tmp" || return 1
    BIN_SRC="$tmp/$base"
    [ -x "$BIN_SRC/zorp" ] && [ -x "$BIN_SRC/zorp-agent" ]
}

BIN_SRC=""
if [ "${ZORP_INSTALL_FROM_SOURCE:-0}" = "1" ]; then
    build_from_source
else
    if target="$(detect_target)" && download_release "$target"; then
        say "Using prebuilt binaries for $target."
    else
        say "No prebuilt binary available; falling back to a source build."
        build_from_source
    fi
fi

say "Creating $INSTALL_DIR if it doesn't exist..."
mkdir -p "$INSTALL_DIR"

# install(1) replaces the file atomically, which avoids ETXTBSY when
# upgrading over a binary that is currently running.
say "Installing binaries to $INSTALL_DIR..."
INSTALLED="'zorp', 'zorp-agent'"
install -m 755 "$BIN_SRC/zorp" "$INSTALL_DIR/"
install -m 755 "$BIN_SRC/zorp-agent" "$INSTALL_DIR/"
if [ -x "$BIN_SRC/zorp-web" ]; then
    install -m 755 "$BIN_SRC/zorp-web" "$INSTALL_DIR/"
    INSTALLED="$INSTALLED and 'zorp-web'"
fi
# The web UI is static files the server does not embed. They go beside the
# binaries so `zorp-web` has something to serve.
if [ -d "$BIN_SRC/web" ]; then
    UI_DIR="${ZORP_UI_DIR:-$HOME/.local/share/zorp/web}"
    mkdir -p "$UI_DIR"
    cp -R "$BIN_SRC/web/." "$UI_DIR/"
    say "Installed the web UI to $UI_DIR"
fi

# Name every binary that was actually installed. This said "'zorp' and
# 'zorp-agent'" while also installing zorp-web, so the chat UI, the part
# that makes this usable by someone who does not live in a terminal,
# arrived on the machine without being mentioned.
say "Successfully installed $INSTALLED to $INSTALL_DIR!"
"$INSTALL_DIR/zorp-agent" --version || true
if [ -x "$INSTALL_DIR/zorp-web" ]; then
    say "For the chat UI, run 'zorp-web' and open http://127.0.0.1:7777"
fi

# Check if INSTALL_DIR is in PATH
if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
    echo ""
    echo "======================================================================"
    echo "WARNING: $INSTALL_DIR is not in your PATH."
    echo "To attach these binaries to your PATH, add this line to your ~/.zshrc or ~/.bashrc:"
    echo ""
    echo "export PATH=\"\$PATH:$INSTALL_DIR\""
    echo "======================================================================"
    echo ""
    echo "After adding, run 'source ~/.zshrc' (or your shell config) to apply."
else
    echo "The binaries are already accessible in your PATH."
fi
