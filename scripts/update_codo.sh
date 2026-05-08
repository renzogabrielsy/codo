#!/usr/bin/env bash
# update_codo.sh — pull latest from git, rebuild the .app, reinstall it.
#
# Usage:   ./scripts/update_codo.sh [git-ref]
#   git-ref defaults to whatever is checked out (or `main` if you pass it).
#
# This is the still.hobbies-style "pull from git" workflow for codo:
#   1. fast-forward your working copy
#   2. reinstall npm deps in case package.json moved
#   3. run `npm run tauri:build` (release; ~5–10 min cold, ~1–2 min warm)
#   4. drop the new .app into /Applications, replacing the old one
#   5. relaunch
#
# For the proper "click Check for Updates inside the app" flow we want
# `tauri-plugin-updater` v2 + GitHub Releases + an Apple Developer ID.
# That lands in a future step. PROJECT_BRAIN.md §3 / §5 #7+#8 cover it.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP_NAME="codo"
BUILD_OUTPUT="${REPO_ROOT}/src-tauri/target/release/bundle/macos/${APP_NAME}.app"
INSTALL_PATH="/Applications/${APP_NAME}.app"

cd "$REPO_ROOT"

REF="${1:-}"

echo "▶ codo update — repo at: $REPO_ROOT"
echo "▶ HEAD before: $(git rev-parse --short HEAD) ($(git rev-parse --abbrev-ref HEAD))"

if [[ -n "$REF" ]]; then
  echo "▶ checking out $REF"
  git fetch origin
  git checkout "$REF"
fi

echo "▶ git pull --ff-only"
git pull --ff-only

echo "▶ npm install"
npm install --no-audit --no-fund

echo "▶ npm run tauri:build (this is the long one)"
npm run tauri:build

if [[ ! -d "$BUILD_OUTPUT" ]]; then
  echo "✗ build did not produce $BUILD_OUTPUT — aborting"
  exit 1
fi

echo "▶ closing any running ${APP_NAME}"
osascript -e "tell application \"${APP_NAME}\" to quit" 2>/dev/null || true
sleep 1

echo "▶ installing → $INSTALL_PATH"
rm -rf "$INSTALL_PATH"
cp -R "$BUILD_OUTPUT" "$INSTALL_PATH"

# macOS quarantines binaries copied from outside the App Store — strip the
# attribute so Renzo doesn't see "developer not verified" every relaunch.
xattr -dr com.apple.quarantine "$INSTALL_PATH" 2>/dev/null || true

echo "▶ relaunching"
open -a "$INSTALL_PATH"

echo "✓ codo updated to $(git rev-parse --short HEAD); installed at $INSTALL_PATH"
