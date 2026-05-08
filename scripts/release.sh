#!/usr/bin/env bash
# release.sh — local-only release pipeline for codo.
#
# Usage:    ./scripts/release.sh 0.0.2
#           ./scripts/release.sh 0.0.2 --notes "release notes go here"
#
# Replaces the GitHub Actions workflow because codo is single-operator on a
# single Mac and CI overhead isn't worth it. If/when we grow a team or want
# Windows/Linux artifacts, resurrect .github/workflows/release.yml.
#
# What it does:
#   1. Verify clean working tree on `main` (or whichever branch you want to
#      release from — we don't enforce `main` because Renzo's not on it yet).
#   2. Bump version in src-tauri/Cargo.toml AND src-tauri/tauri.conf.json
#      to match (the updater compares against tauri.conf.json's version).
#   3. Commit the bump and tag it `vX.Y.Z`.
#   4. `npm run tauri:build` — produces .app, .dmg, .app.tar.gz, .app.tar.gz.sig.
#   5. Generate latest.json from the build outputs (the updater manifest).
#   6. `gh release create` with all artifacts as a DRAFT release.
#   7. Push tag + the bump commit to origin.
#   8. Print the draft release URL — open it, edit notes, click Publish.
#
# Required env (already set up locally for Renzo):
#   ~/.tauri/codo.key  (private signing key, never committed)
#   The passphrase for that key — passed via TAURI_SIGNING_PRIVATE_KEY_PASSWORD
#   env var or a file at ~/.tauri/codo.key.passphrase.
#
# Required tools:  gh  cargo  npm

set -euo pipefail

# ---------------------------------------------------------------------------
# Argument parsing.
# ---------------------------------------------------------------------------

if [[ $# -lt 1 ]]; then
  echo "usage: $0 <version> [--notes \"…\"]" >&2
  echo "       e.g. $0 0.0.2" >&2
  exit 2
fi

VERSION="$1"; shift
NOTES=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --notes) NOTES="$2"; shift 2 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "✗ version must be semver X.Y.Z (got: $VERSION)" >&2
  exit 2
fi
TAG="v${VERSION}"

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

# ---------------------------------------------------------------------------
# Sanity checks.
# ---------------------------------------------------------------------------

echo "▶ checking working tree"
if [[ -n "$(git status --porcelain)" ]]; then
  echo "✗ working tree is not clean. commit or stash first." >&2
  git status --short
  exit 1
fi

echo "▶ checking tag $TAG isn't already used"
if git rev-parse -q --verify "refs/tags/$TAG" > /dev/null; then
  echo "✗ tag $TAG already exists locally. delete it or pick another version." >&2
  exit 1
fi
if git ls-remote --exit-code --tags origin "$TAG" >/dev/null 2>&1; then
  echo "✗ tag $TAG already exists on origin." >&2
  exit 1
fi

echo "▶ checking signing key"
if [[ ! -f "$HOME/.tauri/codo.key" ]]; then
  echo "✗ ~/.tauri/codo.key not found. Run:  npx tauri signer generate -w ~/.tauri/codo.key" >&2
  exit 1
fi

# Resolve the signing passphrase.
if [[ -z "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}" ]]; then
  if [[ -f "$HOME/.tauri/codo.key.passphrase" ]]; then
    export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(cat "$HOME/.tauri/codo.key.passphrase")"
  else
    echo "✗ TAURI_SIGNING_PRIVATE_KEY_PASSWORD env not set, and ~/.tauri/codo.key.passphrase not found." >&2
    echo "   run:  echo 'YOUR_PASSPHRASE' > ~/.tauri/codo.key.passphrase  &&  chmod 600 ~/.tauri/codo.key.passphrase" >&2
    exit 1
  fi
fi
export TAURI_SIGNING_PRIVATE_KEY="$(cat "$HOME/.tauri/codo.key")"

if ! command -v gh >/dev/null; then
  echo "✗ gh (GitHub CLI) not found. install it:  brew install gh  &&  gh auth login" >&2
  exit 1
fi

# ---------------------------------------------------------------------------
# Bump version in lockstep.
# ---------------------------------------------------------------------------

echo "▶ bumping version to $VERSION in Cargo.toml + tauri.conf.json"

# tauri.conf.json — uses double quotes; replace the "version": "..." line.
python3 - "$REPO_ROOT/src-tauri/tauri.conf.json" "$VERSION" <<'PY'
import json, sys
path, version = sys.argv[1], sys.argv[2]
with open(path, "r") as f:
    data = json.load(f)
data["version"] = version
with open(path, "w") as f:
    json.dump(data, f, indent=2)
    f.write("\n")
PY

# Cargo.toml — first occurrence of `version = "..."` after `[package]`.
python3 - "$REPO_ROOT/src-tauri/Cargo.toml" "$VERSION" <<'PY'
import re, sys
path, version = sys.argv[1], sys.argv[2]
with open(path, "r") as f:
    src = f.read()
# Replace the first version field after [package].
src = re.sub(
    r'(\[package\][^\[]*?\nversion\s*=\s*")[^"]+(")',
    r'\g<1>' + version + r'\g<2>',
    src,
    count=1,
    flags=re.DOTALL,
)
with open(path, "w") as f:
    f.write(src)
PY

# Cargo.lock will get refreshed by the next `cargo` invocation.
( cd src-tauri && cargo update -p codo --offline 2>/dev/null || true )

# ---------------------------------------------------------------------------
# Commit + tag.
# ---------------------------------------------------------------------------

echo "▶ committing version bump"
git add src-tauri/Cargo.toml src-tauri/tauri.conf.json src-tauri/Cargo.lock
git commit -m "chore: release ${TAG}"
git tag "$TAG"

# ---------------------------------------------------------------------------
# Build + sign.
# ---------------------------------------------------------------------------

echo "▶ building (this is the long step — ~5–10 min cold, ~1–2 min warm)"
npm install --no-audit --no-fund
npm run tauri:build

BUNDLE_DIR="$REPO_ROOT/src-tauri/target/release/bundle"
APP_TAR="$(ls "$BUNDLE_DIR/macos/"codo*.app.tar.gz 2>/dev/null | head -1)"
APP_SIG="${APP_TAR}.sig"
APP_DMG="$(ls "$BUNDLE_DIR/dmg/"codo*.dmg 2>/dev/null | head -1)"
APP_BUNDLE="$BUNDLE_DIR/macos/codo.app"

# DMG bundling sometimes fails on this Mac (assistive-access AppleScript inside
# bundle_dmg.sh). The release works without it — only the .app.tar.gz + .sig
# matter for the auto-updater.
if [[ -z "$APP_TAR" || ! -f "$APP_TAR" || ! -f "$APP_SIG" ]]; then
  echo "✗ release artifacts missing. expected:" >&2
  echo "   $APP_TAR" >&2
  echo "   $APP_SIG" >&2
  exit 1
fi

echo "▶ artifacts:"
ls -la "$APP_TAR" "$APP_SIG" 2>/dev/null
[[ -n "$APP_DMG" && -f "$APP_DMG" ]] && ls -la "$APP_DMG" || echo "   (no DMG — not blocking)"

# ---------------------------------------------------------------------------
# Generate latest.json (the updater manifest).
# ---------------------------------------------------------------------------

echo "▶ generating latest.json"
SIG_CONTENTS="$(cat "$APP_SIG")"
ASSET_BASENAME="$(basename "$APP_TAR")"
PUB_DATE="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"

LATEST_JSON="$BUNDLE_DIR/latest.json"
python3 - "$LATEST_JSON" "$VERSION" "$PUB_DATE" "$SIG_CONTENTS" "$ASSET_BASENAME" "$NOTES" <<'PY'
import json, sys
path, version, pub_date, sig, asset_basename, notes = sys.argv[1:]
manifest = {
    "version": f"v{version}",
    "notes": notes or f"codo v{version}",
    "pub_date": pub_date,
    "platforms": {
        "darwin-aarch64": {
            "signature": sig,
            "url": f"https://github.com/renzogabrielsy/codo/releases/download/v{version}/{asset_basename}"
        },
        "darwin-x86_64": {
            "signature": sig,
            "url": f"https://github.com/renzogabrielsy/codo/releases/download/v{version}/{asset_basename}"
        }
    }
}
with open(path, "w") as f:
    json.dump(manifest, f, indent=2)
    f.write("\n")
print(f"wrote {path}")
PY

# ---------------------------------------------------------------------------
# Push tag + commit, create draft GitHub Release.
# ---------------------------------------------------------------------------

echo "▶ pushing branch + tag to origin"
git push origin HEAD
git push origin "$TAG"

echo "▶ creating draft GitHub Release $TAG"
ASSETS=("$APP_TAR" "$APP_SIG" "$LATEST_JSON")
[[ -n "$APP_DMG" && -f "$APP_DMG" ]] && ASSETS+=("$APP_DMG")

NOTES_BODY="${NOTES:-See git log between previous tag and ${TAG} for changes.}"

gh release create "$TAG" \
  --draft \
  --title "codo $TAG" \
  --notes "$NOTES_BODY" \
  "${ASSETS[@]}"

DRAFT_URL="$(gh release view "$TAG" --json url --jq .url)"
echo ""
echo "✓ draft release created: $DRAFT_URL"
echo ""
echo "next steps:"
echo "  1. open the URL above, edit the release notes (they show in the in-app updater UI)"
echo "  2. click Publish"
echo "  3. open codo → Check for updates → it'll see $TAG and self-install"
