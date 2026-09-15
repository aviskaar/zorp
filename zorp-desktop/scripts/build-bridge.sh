#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
BRIDGE_DIR="${REPO_ROOT}/zorp-desktop/bridge"
OUTPUT_DIR="${REPO_ROOT}/zorp-desktop/Zorp/Frameworks"
INCLUDE_DIR="${REPO_ROOT}/zorp-desktop/Zorp/Bridge"

mkdir -p "${OUTPUT_DIR}" "${INCLUDE_DIR}"

echo "==> Building zorp-desktop-bridge for aarch64-apple-darwin..."
cargo build --manifest-path "${BRIDGE_DIR}/Cargo.toml" --release --target aarch64-apple-darwin

echo "==> Building zorp-desktop-bridge for x86_64-apple-darwin..."
cargo build --manifest-path "${BRIDGE_DIR}/Cargo.toml" --release --target x86_64-apple-darwin

echo "==> Creating universal static library using lipo..."
lipo -create \
  "${REPO_ROOT}/zorp-desktop/target/aarch64-apple-darwin/release/libzorp_desktop_bridge.a" \
  "${REPO_ROOT}/zorp-desktop/target/x86_64-apple-darwin/release/libzorp_desktop_bridge.a" \
  -output "${OUTPUT_DIR}/libzorp_desktop_bridge.a"

echo "==> Copying C header..."
cp "${BRIDGE_DIR}/include/zorp_bridge.h" "${INCLUDE_DIR}/zorp_bridge.h"

echo "==> Verifying universal static library architectures..."
lipo -info "${OUTPUT_DIR}/libzorp_desktop_bridge.a"

echo "==> Universal bridge library successfully created at ${OUTPUT_DIR}/libzorp_desktop_bridge.a"
