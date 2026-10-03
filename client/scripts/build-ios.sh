#!/usr/bin/env bash
# iOS preview: Simulator needs no Apple account. Device output is unsigned;
# signing/installing on an iPhone must be done in Xcode with a Personal Team.
set -euo pipefail
client_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
repo_dir="$(cd -- "$client_dir/.." && pwd)"
if [[ -d "$client_dir/rust/rdp-core" ]]; then
  python3 "$client_dir/scripts/verify-release-identity.py" --root "$repo_dir"
fi
mode="${1:---simulator}"
if [[ "$mode" != '--simulator' && "$mode" != '--device' ]]; then
  echo 'Usage: build-ios.sh [--simulator|--device]' >&2
  exit 2
fi
if [[ "$(uname -s)" != Darwin ]]; then
  echo 'iOS builds require macOS and Xcode.' >&2
  exit 1
fi
xcrun --sdk iphonesimulator --show-sdk-path >/dev/null
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_INCREMENTAL=0
export IPHONEOS_DEPLOYMENT_TARGET=15.0
if [[ "$mode" == '--simulator' ]]; then
  rustup target add aarch64-apple-ios-sim x86_64-apple-ios
  flutter_args=(--simulator --debug)
  app='build/ios/iphonesimulator/Runner.app'
  platform='simulator-universal'
else
  rustup target add aarch64-apple-ios
  flutter_args=(--release --no-codesign)
  app='build/ios/iphoneos/Runner.app'
  platform='device-arm64-unsigned'
fi
cd -- "$client_dir/flutter"
# Every attempted native build consumes a unique number, including failures.
build_version="$(python3 "$client_dir/scripts/bump-version.py" --root "$repo_dir")"
echo "==> iOS $platform $build_version"
flutter build ios "${flutter_args[@]}" --dart-define=CC_MOCK=false
actual_version="$(/usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' "$app/Info.plist")"
actual_build="$(/usr/libexec/PlistBuddy -c 'Print CFBundleVersion' "$app/Info.plist")"
if [[ "$actual_version+$actual_build" != "$build_version" ]]; then
  echo 'Native iOS bundle version does not match the reserved version.' >&2
  exit 1
fi
[[ "$(/usr/libexec/PlistBuddy -c 'Print CFBundleIdentifier' "$app/Info.plist")" == io.consolecrypt.consolecrypt ]] || { echo 'Refusing a non-production iOS bundle.' >&2; exit 1; }
if [[ -d "$client_dir/rust/rdp-core" ]]; then
  cmp "$client_dir/rust/rdp-core/THIRD_PARTY_NOTICES.txt" "$app/Frameworks/App.framework/flutter_assets/assets/licenses/RDP-THIRD-PARTY-NOTICES.txt"
fi
out_dir="$repo_dir/dist/ios"
mkdir -p -- "$out_dir"
archive="$out_dir/ConsoleCrypt-$build_version-ios-$platform.zip"
ditto -c -k --sequesterRsrc --keepParent "$app" "$archive"
printf '%s\n' "$build_version" > "$out_dir/ConsoleCrypt-$platform.version"
shasum -a 256 "$archive" > "$archive.sha256"
echo "$archive"
if [[ "$mode" == '--device' ]]; then
  echo 'This is an unsigned development bundle, not an installable IPA.'
fi
