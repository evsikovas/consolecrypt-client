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
if [[ "$MODE" == release ]]; then
  # Release apps are universal (Apple Silicon + Intel).
  rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null
fi

OUT="$REPO/dist/macos"
mkdir -p "$OUT"

echo "==> Flutter app ($MODE)"
cd "$SRC/client/flutter"
BUILD_VERSION="$(python3 "$REPO/client/scripts/bump-version.py" --root "$REPO" --source-root "$SRC")"
echo "==> version $BUILD_VERSION"
flutter pub get
DEFINES=()
[[ "$MOCK" == 1 ]] && DEFINES+=(--dart-define=CC_MOCK=true)
flutter build macos "--$MODE" ${DEFINES[@]+"${DEFINES[@]}"}
APP_DIR="build/macos/Build/Products/$( [[ $MODE == release ]] && echo Release || echo Debug )"
rm -rf "$OUT/ConsoleCrypt.app"
ditto "$APP_DIR/ConsoleCrypt.app" "$OUT/ConsoleCrypt.app"
if [[ -f "$SRC/LICENSE" ]]; then
  cp "$SRC/LICENSE" "$OUT/ConsoleCrypt.app/Contents/Resources/LICENSE"
else
  # Historical commits retain the licenses originally published with them.
  cp "$SRC/LICENSE-MIT" "$SRC/LICENSE-APACHE" "$OUT/ConsoleCrypt.app/Contents/Resources/"
fi

if [[ -n "$SIGN_IDENTITY" ]]; then
  echo "==> signing with a stable identity"
  python3 "$SRC/client/scripts/sign-macos.py" --identity "$SIGN_IDENTITY" --app "$OUT/ConsoleCrypt.app"
else
  # The added license changes the resource seal of Flutter's ad-hoc bundle.
  codesign --force --sign - --preserve-metadata=identifier,entitlements "$OUT/ConsoleCrypt.app"
fi
codesign --verify --deep --strict "$OUT/ConsoleCrypt.app"

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
