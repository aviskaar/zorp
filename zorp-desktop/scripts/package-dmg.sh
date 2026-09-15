#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
VERSION=$(grep -m1 'version =' "${REPO_ROOT}/zorp-desktop/bridge/Cargo.toml" | cut -d '"' -f2)
SWIFT_DIR="${REPO_ROOT}/zorp-desktop/Zorp"
EXPORT_DIR="${REPO_ROOT}/zorp-desktop/build/export"
APP_PATH="${EXPORT_DIR}/Zorp.app"
DMG_DIR="${REPO_ROOT}/zorp-desktop/build"
DMG_PATH="${DMG_DIR}/Zorp-v${VERSION}-universal.dmg"

echo "==> Ensuring universal C bridge is built..."
if [ ! -f "${SWIFT_DIR}/Frameworks/libzorp_desktop_bridge.a" ]; then
  bash "${SCRIPT_DIR}/build-bridge.sh"
fi

echo "==> Building universal Zorp binaries..."
cd "${SWIFT_DIR}"

# Build arm64
echo "==> Building arm64 release binary..."
swift build -c release --triple arm64-apple-macosx --product Zorp

# Build x86_64
echo "==> Building x86_64 release binary..."
swift build -c release --triple x86_64-apple-macosx --product Zorp

echo "==> Assembling Zorp.app bundle..."
rm -rf "${APP_PATH}"
mkdir -p "${APP_PATH}/Contents/MacOS"
mkdir -p "${APP_PATH}/Contents/Resources"

# Lipo create universal binary
echo "==> Combining binaries into universal executable..."
lipo -create \
  "${SWIFT_DIR}/.build/arm64-apple-macosx/release/Zorp" \
  "${SWIFT_DIR}/.build/x86_64-apple-macosx/release/Zorp" \
  -output "${APP_PATH}/Contents/MacOS/Zorp"
chmod +x "${APP_PATH}/Contents/MacOS/Zorp"

# Copy Info.plist
cp "${SWIFT_DIR}/Info.plist" "${APP_PATH}/Contents/Info.plist"

# Copy AppIcon if available
if [ -f "${REPO_ROOT}/zorp-desktop/icons/icon.icns" ]; then
  cp "${REPO_ROOT}/zorp-desktop/icons/icon.icns" "${APP_PATH}/Contents/Resources/AppIcon.icns"
fi

# Ad-hoc code signing for macOS Gatekeeper compatibility
echo "==> Signing Zorp.app (ad-hoc)..."
codesign --force --deep --sign - "${APP_PATH}"

# Verify universal app binary
echo "==> Verifying binary architectures..."
lipo -info "${APP_PATH}/Contents/MacOS/Zorp"

echo "==> Packaging Zorp.app into ${DMG_PATH}..."
mkdir -p "${DMG_DIR}"

if [ -f "${DMG_PATH}" ]; then
  rm "${DMG_PATH}"
fi

hdiutil create -volname "Zorp" \
  -srcfolder "${APP_PATH}" \
  -ov -format UDZO \
  "${DMG_PATH}"

echo "==> DMG successfully created at ${DMG_PATH}"
