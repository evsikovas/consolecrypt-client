#!/usr/bin/env bash
# Package an existing app, including Applications and a Retina background.
set -euo pipefail
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
APP="${1:-$REPO/dist/macos/ConsoleCrypt.app}"
DEST="${2:-$REPO/dist/macos/ConsoleCrypt.dmg}"
SIGN_IDENTITY="${3-${CC_CODESIGN_IDENTITY:-}}"
[[ "$(basename "$APP")" == ConsoleCrypt.app ]] || { echo 'Expected the stable application bundle.' >&2; exit 1; }
[[ "$(/usr/libexec/PlistBuddy -c 'Print CFBundleIdentifier' "$APP/Contents/Info.plist")" == io.consolecrypt.consolecrypt ]] || { echo 'Refusing to package a development bundle as stable.' >&2; exit 1; }
ASSETS="$REPO/client/packaging/macos"
TOOLS="$REPO/dist/macos/.packaging-tools"
if [[ -z "${DMGBUILD:-}" ]]; then
  if [[ ! -x "$TOOLS/bin/dmgbuild" ]]; then
    python3 -m venv "$TOOLS"
    "$TOOLS/bin/pip" install --disable-pip-version-check -r "$ASSETS/requirements.txt"
  fi
  DMGBUILD="$TOOLS/bin/dmgbuild"
fi
STAGING="$(mktemp -d "${TMPDIR:-/tmp}/consolecrypt-dmg.XXXXXX")"
trap 'rm -rf "$STAGING"' EXIT
# Build-time resizing only; keep the generated source illustration intact.
sips -z 512 768 "$ASSETS/installer-background.png" --out "$STAGING/background.png" >/dev/null
cp "$ASSETS/installer-background.png" "$STAGING/background@2x.png"
# Small separate Finder/Dock launcher. The updater ZIP still contains the
# original application bundle, and both apps use the same terminal executable.
TERMINAL="$STAGING/ConsoleCrypt Terminal.app"
CONTENTS="$TERMINAL/Contents"
mkdir -p "$CONTENTS/MacOS" "$CONTENTS/Resources"
cp "$ASSETS/TerminalLauncher.plist" "$CONTENTS/Info.plist"
if [[ -f "$APP/Contents/Resources/AppIcon.icns" ]]; then
  cp "$APP/Contents/Resources/AppIcon.icns" "$CONTENTS/Resources/"
fi
for key in CFBundleShortVersionString CFBundleVersion; do
  value="$(/usr/libexec/PlistBuddy -c "Print $key" "$APP/Contents/Info.plist")"
  plutil -insert "$key" -string "$value" "$CONTENTS/Info.plist"
done
xcrun clang -fobjc-arc -framework AppKit -arch arm64 -arch x86_64 \
  -mmacosx-version-min=12.0 "$ASSETS/TerminalLauncher.m" -o "$CONTENTS/MacOS/consolecrypt-terminal"
lipo -verify_arch arm64 x86_64 "$CONTENTS/MacOS/consolecrypt-terminal"
if [[ -n "$SIGN_IDENTITY" ]]; then
  python3 "$REPO/client/scripts/sign-macos.py" --identity "$SIGN_IDENTITY" --app "$TERMINAL"
else
  python3 "$REPO/client/scripts/sign-macos.py" --adhoc --app "$TERMINAL"
fi
# dmgbuild combines 1x/2x representations into a HiDPI TIFF, records the
# background alias and icon positions, and detaches its temporary volume.
"$DMGBUILD" -s "$ASSETS/dmg-settings.py" -D "app=$APP" -D "terminal=$TERMINAL" \
  -D "background=$STAGING/background.png" "ConsoleCrypt" "$DEST"
hdiutil verify "$DEST" >/dev/null
(cd "$(dirname "$DEST")" && shasum -a 256 "$(basename "$DEST")" > "$(basename "$DEST").sha256")
