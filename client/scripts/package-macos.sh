#!/usr/bin/env bash
# Package an existing app, including Applications and a Retina background.
set -euo pipefail
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
APP="${1:-$REPO/dist/macos/ConsoleCrypt.app}"
DEST="${2:-$REPO/dist/macos/ConsoleCrypt.dmg}"
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
# dmgbuild combines 1x/2x representations into a HiDPI TIFF, records the
# background alias and icon positions, and detaches its temporary volume.
"$DMGBUILD" -s "$ASSETS/dmg-settings.py" -D "app=$APP" \
  -D "background=$STAGING/background.png" "ConsoleCrypt" "$DEST"
hdiutil verify "$DEST" >/dev/null
(cd "$(dirname "$DEST")" && shasum -a 256 "$(basename "$DEST")" > "$(basename "$DEST").sha256")
