#!/usr/bin/env bash
# Build the ConsoleCrypt desktop client (Flutter UI + Rust core) and the
# `consolecrypt` CLI for macOS.
#
# Usage:
#   client/scripts/build-macos.sh                 # release build of the working tree
#   client/scripts/build-macos.sh --debug         # faster debug build
#   client/scripts/build-macos.sh --commit HEAD   # build a clean checkout of a commit
#   client/scripts/build-macos.sh --mock          # UI on the in-memory mock backend
#   client/scripts/build-macos.sh --no-cli        # skip the CLI
#   CC_CODESIGN_IDENTITY='Developer ID Application: Name (TEAM)' client/scripts/build-macos.sh --no-cli
#
# Output: dist/macos/ConsoleCrypt.app, ConsoleCrypt-macos.zip, ConsoleCrypt.dmg,
#         consolecrypt (CLI)
#
# Author: Alexander Evsikov <i@evsikov.net>
set -euo pipefail

MODE=release
COMMIT=""
MOCK=0
CLI=1
SIGN_IDENTITY="${CC_CODESIGN_IDENTITY:-}"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --debug) MODE=debug ;;
    --release) MODE=release ;;
    --commit) COMMIT="${2:?--commit needs a revision}"; shift ;;
    --mock) MOCK=1 ;;
    --no-cli) CLI=0 ;;
    --sign-identity) SIGN_IDENTITY="${2:?--sign-identity needs a certificate name or SHA-1}"; shift ;;
    -h|--help) sed -n '2,16p' "$0"; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
  shift
done

export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8          # CocoaPods needs UTF-8
export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SRC="$REPO"
TMP_WT=""
if [[ -n "$COMMIT" ]]; then
  TMP_WT="$(mktemp -d "${TMPDIR:-/tmp}/consolecrypt-build.XXXXXX")"
  git -C "$REPO" worktree add --detach "$TMP_WT" "$COMMIT" >/dev/null
  SRC="$TMP_WT"
  trap 'git -C "$REPO" worktree remove --force "$TMP_WT" >/dev/null 2>&1 || true' EXIT
  echo "==> building commit $(git -C "$SRC" rev-parse --short HEAD) in $SRC"
fi

for tool in flutter cargo rustup xcodebuild pod python3; do
  command -v "$tool" >/dev/null || { echo "missing: $tool (see README → Building)" >&2; exit 1; }
done
if [[ -n "$SIGN_IDENTITY" ]]; then
  # Fail early if no matching certificate/private-key identity is available.
  python3 "$SRC/client/scripts/sign-macos.py" --identity "$SIGN_IDENTITY" --check
fi
if [[ -d "$SRC/client/rust/rdp-core" ]]; then
  python3 "$SRC/client/scripts/verify-release-identity.py" --root "$SRC"
fi
if [[ "$MODE" == release ]]; then
  export FLUTTER_MACOS_ARM64_ONLY=false
  # Release apps are universal (Apple Silicon + Intel).
  rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null
fi

OUT="$REPO/dist/macos"
mkdir -p "$OUT"

echo "==> Flutter app ($MODE)"
cd "$SRC/client/flutter"
BUILD_VERSION="$(python3 "$REPO/client/scripts/bump-version.py" --root "$REPO" --source-root "$SRC")"
echo "==> version $BUILD_VERSION"
python3 -m unittest discover -s "$SRC/client/scripts" -p 'test_macos_updates.py' -v
python3 -m unittest discover -s "$SRC/client/scripts" -p 'test_macos_processes.py' -v
flutter pub get
# CocoaPods' post-install hook reads the engine before `flutter build` can
# populate it. Fresh hosted runners have only the SDK/test engine at this point.
flutter precache --macos
# Refresh local RDP zlib pod linkage even if Flutter cached plugin metadata.
(cd macos && pod install)
DEFINES=()
[[ "$MOCK" == 1 ]] && DEFINES+=(--dart-define=CC_MOCK=true)
flutter build macos "--$MODE" ${DEFINES[@]+"${DEFINES[@]}"}
APP_DIR="build/macos/Build/Products/$( [[ $MODE == release ]] && echo Release || echo Debug )"
BUILT_APP="$APP_DIR/ConsoleCrypt.app"
[[ "$(/usr/libexec/PlistBuddy -c 'Print CFBundleIdentifier' "$BUILT_APP/Contents/Info.plist")" == io.consolecrypt.consolecrypt ]] || { echo 'Refusing a non-production macOS bundle.' >&2; exit 1; }
ACTUAL_VERSION="$(/usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' "$BUILT_APP/Contents/Info.plist")"
ACTUAL_BUILD="$(/usr/libexec/PlistBuddy -c 'Print CFBundleVersion' "$BUILT_APP/Contents/Info.plist")"
[[ "$ACTUAL_VERSION+$ACTUAL_BUILD" == "$BUILD_VERSION" ]] || { echo 'Native macOS metadata does not match the reserved build.' >&2; exit 1; }
rm -rf "$OUT/ConsoleCrypt.app"
ditto "$BUILT_APP" "$OUT/ConsoleCrypt.app"
if [[ -f "$SRC/LICENSE" ]]; then
  cp "$SRC/LICENSE" "$OUT/ConsoleCrypt.app/Contents/Resources/LICENSE"
else
  # Historical commits retain the licenses originally published with them.
  cp "$SRC/LICENSE-MIT" "$SRC/LICENSE-APACHE" "$OUT/ConsoleCrypt.app/Contents/Resources/"
fi

if [[ -d "$SRC/client/rust/rdp-core" ]]; then
  cp "$SRC/client/rust/rdp-core/THIRD_PARTY_NOTICES.txt" "$OUT/ConsoleCrypt.app/Contents/Resources/RDP-THIRD-PARTY-NOTICES.txt"
fi

LOCAL_AI_BUNDLED=0
if [[ -f "$SRC/client/scripts/bundle-macos-local-ai.py" ]]; then
  echo "==> verified local AI helpers (Apple Silicon + Intel)"
  python3 "$SRC/client/scripts/bundle-macos-local-ai.py" --app "$OUT/ConsoleCrypt.app"
  LOCAL_AI_BUNDLED=1
fi

if [[ -n "$SIGN_IDENTITY" ]]; then
  echo "==> signing with a stable identity"
  python3 "$SRC/client/scripts/sign-macos.py" --identity "$SIGN_IDENTITY" --app "$OUT/ConsoleCrypt.app"
else
  if [[ "$LOCAL_AI_BUNDLED" == 1 ]]; then
    python3 "$SRC/client/scripts/sign-macos.py" --adhoc --app "$OUT/ConsoleCrypt.app"
  else
    # Historical commits predate the nested helper/signing support.
    codesign --force --sign - --preserve-metadata=identifier,entitlements "$OUT/ConsoleCrypt.app"
  fi
fi
codesign --verify --deep --strict "$OUT/ConsoleCrypt.app"
if [[ "$LOCAL_AI_BUNDLED" == 1 ]]; then
  SMOKE_ARGS=(--app "$OUT/ConsoleCrypt.app")
  [[ -n "$SIGN_IDENTITY" ]] && SMOKE_ARGS+=(--identity "$SIGN_IDENTITY")
  if [[ -f "$SRC/client/scripts/smoke-macos-processes.py" ]]; then
    echo "==> packaged local zsh and AI helper (isolated process smoke)"
    python3 "$SRC/client/scripts/smoke-macos-processes.py" "${SMOKE_ARGS[@]}"
  else
    # --commit can reproduce a historical release with its original policy.
    python3 "$SRC/client/scripts/smoke-local-ai-sandbox.py" "${SMOKE_ARGS[@]}"
  fi
fi

echo "==> packaging"
rm -f "$OUT/ConsoleCrypt-macos.zip" "$OUT/ConsoleCrypt.dmg"
ditto -c -k --keepParent "$OUT/ConsoleCrypt.app" "$OUT/ConsoleCrypt-macos.zip"
if [[ "$MODE" == release ]]; then
  bash "$SRC/client/scripts/package-macos.sh" "$OUT/ConsoleCrypt.app" "$OUT/ConsoleCrypt.dmg"
fi

if [[ "$CLI" == 1 ]]; then
  echo "==> consolecrypt CLI"
  cd "$SRC/client/rust"
  CARGO_TARGET_DIR="$SRC/client/rust/target/dist" cargo build --release -p cc-cli
  cp "$SRC/client/rust/target/dist/release/consolecrypt" "$OUT/consolecrypt"
fi

printf '%s\n' "$BUILD_VERSION" > "$OUT/ConsoleCrypt.version"
echo
echo "Done → $OUT"
ls -lh "$OUT"
echo
echo "Run:  open \"$OUT/ConsoleCrypt.app\""
if [[ -n "$SIGN_IDENTITY" ]]; then
  echo "Signed with a persistent identity. Keep using this identity for updates."
  echo "Public distribution additionally needs Apple notarization."
else
  echo "Note: this build is ad-hoc signed; macOS may ask for keychain access after"
  echo "      each rebuild. Set CC_CODESIGN_IDENTITY to an existing certificate"
  echo "      (or pass --sign-identity) to keep a stable signature across updates."
  echo "      Install in Applications before opening. The prompt needs the login"
  echo "      keychain password, not the vault passphrase. Never delete the keychain."
fi
