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

# The image holds Zorp.app beside a link to /Applications, so opening it
# shows the usual "drag the app onto Applications" window. Without the
# link a person sees the app alone and has nowhere obvious to put it.
STAGE_DIR="${DMG_DIR}/stage"
RW_DMG="${DMG_DIR}/Zorp-rw.dmg"
rm -rf "${STAGE_DIR}" "${RW_DMG}"
mkdir -p "${STAGE_DIR}"
cp -R "${APP_PATH}" "${STAGE_DIR}/Zorp.app"
ln -s /Applications "${STAGE_DIR}/Applications"

hdiutil create -volname "Zorp" \
  -srcfolder "${STAGE_DIR}" \
  -ov -format UDRW \
  "${RW_DMG}"

# Lay the window out as two big icons side by side, app on the left. This
# goes through Finder, which a headless machine may refuse or leave
# waiting on an automation prompt, so it gets a minute, and a failure here
# leaves the default layout with the link still in it.
MOUNT_DIR="$(hdiutil attach -readwrite -noverify -noautoopen "${RW_DMG}" |
  awk -F '\t' '/\/Volumes\// { print $NF; exit }')"
if [ -n "${MOUNT_DIR}" ]; then
  VOLUME_NAME="$(basename "${MOUNT_DIR}")"
  if ! perl -e 'alarm 60; exec @ARGV' osascript <<APPLESCRIPT
tell application "Finder"
  tell disk "${VOLUME_NAME}"
    open
    set current view of container window to icon view
    set toolbar visible of container window to false
    set statusbar visible of container window to false
    set the bounds of container window to {200, 120, 740, 460}
    set viewOptions to the icon view options of container window
    set arrangement of viewOptions to not arranged
    set icon size of viewOptions to 128
    set position of item "Zorp.app" of container window to {140, 160}
    set position of item "Applications" of container window to {400, 160}
    update without registering applications
    delay 1
    close
  end tell
end tell
APPLESCRIPT
  then
    echo "warning: Finder did not lay out the DMG window; using the default layout"
  fi
  sync
  hdiutil detach "${MOUNT_DIR}" || hdiutil detach -force "${MOUNT_DIR}"
fi

hdiutil convert "${RW_DMG}" -format UDZO -o "${DMG_PATH}"
rm -rf "${STAGE_DIR}" "${RW_DMG}"

echo "==> DMG successfully created at ${DMG_PATH}"
