#!/usr/bin/env bash
# Private release keys are supplied by the operator/CI, never generated here.
set -euo pipefail
repo_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
: "${CC_ANDROID_APP_SIGNING_PROPERTIES:?Private app signing properties are required}"
: "${CC_ANDROID_UPLOAD_SIGNING_PROPERTIES:?Private upload signing properties are required}"
: "${ANDROID_HOME:?Android SDK is required}"
[[ -f "$CC_ANDROID_APP_SIGNING_PROPERTIES" && -f "$CC_ANDROID_UPLOAD_SIGNING_PROPERTIES" ]]
python3 "$repo_dir/client/scripts/verify-release-identity.py" --root "$repo_dir"
version="$(python3 "$repo_dir/client/scripts/bump-version.py" --root "$repo_dir")"
destination="$repo_dir/dist/android/rustore-$version"
[[ ! -e "$destination" ]] || { echo 'Output already exists; refusing to replace it.' >&2; exit 1; }
mkdir -p "$destination"
cd "$repo_dir/client/flutter"
export CC_ANDROID_SIGNING_PROPERTIES="$CC_ANDROID_UPLOAD_SIGNING_PROPERTIES"
flutter build appbundle --release --flavor rustore --target-platform android-arm64 --dart-define=CC_MOCK=false
cp build/app/outputs/bundle/rustoreRelease/app-rustore-release.aab "$destination/ConsoleCrypt-$version-rustore-arm64.aab"
export CC_ANDROID_SIGNING_PROPERTIES="$CC_ANDROID_APP_SIGNING_PROPERTIES"
flutter build apk --release --flavor rustore --target-platform android-arm64 --dart-define=CC_MOCK=false
apk="build/app/outputs/flutter-apk/app-rustore-release.apk"
"$ANDROID_HOME/build-tools/36.0.0/apksigner" verify --verbose "$apk"
"$ANDROID_HOME/build-tools/36.0.0/zipalign" -c -P 16 4 "$apk"
python3 "$repo_dir/client/scripts/verify-android-apk.py" "$apk" "$repo_dir/client/rust/rdp-core/THIRD_PARTY_NOTICES.txt"
permissions="$("$ANDROID_HOME/build-tools/36.0.0/aapt2" dump permissions "$apk")"
if [[ "$permissions" == *REQUEST_INSTALL_PACKAGES* ]]; then
  echo 'RuStore APK must not request package installation' >&2; exit 1
fi
cp "$apk" "$destination/ConsoleCrypt-$version-rustore-arm64.apk"
printf '%s\n' "$version" > "$destination/ConsoleCrypt-rustore.version"
(cd "$destination" && shasum -a 256 ./*.apk ./*.aab > SHA256SUMS)
if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
  printf 'path=%s\n' "$destination" >> "$GITHUB_OUTPUT"
fi
printf '%s\n' "$destination"
