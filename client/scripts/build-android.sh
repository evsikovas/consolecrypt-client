#!/usr/bin/env bash
# Local ARM64 preview for Pixel and other 64-bit Android 11+ phones.
set -euo pipefail
client_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
repo_dir="$(cd -- "$client_dir/.." && pwd)"
if [[ -d "$client_dir/rust/rdp-core" ]]; then
  python3 "$client_dir/scripts/verify-release-identity.py" --root "$repo_dir"
fi
export PATH="$HOME/.cargo/bin:$PATH"
export ANDROID_HOME="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$HOME/Library/Android/sdk}}"
export CARGO_INCREMENTAL=0
if [[ ! -d "$ANDROID_HOME/platforms/android-36" ]]; then
  echo 'Install Android SDK platform 36, build-tools 36.0.0, NDK 28.2.13676358 and CMake 3.22.1 first.' >&2
  exit 1
fi
export PATH="$ANDROID_HOME/cmake/3.22.1/bin:$PATH"
# Reuse the existing preview signer; never generate a replacement key silently.
signer_home="${ANDROID_USER_HOME:-$HOME/.android}"
[[ -f "$signer_home/debug.keystore" ]] || { echo 'Existing Android preview signer is required; no key is created.' >&2; exit 1; }
cd -- "$client_dir/flutter"
build_version="$(python3 "$client_dir/scripts/bump-version.py" --root "$repo_dir")"
echo "==> version $build_version"
flutter build apk --release --target-platform android-arm64 --dart-define=CC_MOCK=false
apk='build/app/outputs/flutter-apk/app-release.apk'
"$ANDROID_HOME/build-tools/36.0.0/apksigner" verify --verbose "$apk"
"$ANDROID_HOME/build-tools/36.0.0/zipalign" -c -P 16 4 "$apk"
# Check every native library, including Flutter, SQLCipher/OpenSSL in the
# Rust library, and the Dart AOT image. No 4 KiB-only ELF may ship.
if [[ -d "$client_dir/rust/rdp-core" ]]; then
  python3 "$client_dir/scripts/verify-android-apk.py" "$apk" "$client_dir/rust/rdp-core/THIRD_PARTY_NOTICES.txt"
else
  python3 "$client_dir/scripts/verify-android-apk.py" "$apk"
fi
mkdir -p -- "$repo_dir/dist/android"
# Replace the directory entry, never truncate an existing inode: the latest
# alias may be hardlinked to a retained release. Stage on the same filesystem
# so a failed copy keeps the previous alias intact and rename is atomic.
apk_destination="$repo_dir/dist/android/ConsoleCrypt-android-arm64.apk"
apk_temporary="$(mktemp "$apk_destination.tmp.XXXXXX")"
trap 'rm -f -- "$apk_temporary"' EXIT
cp -- "$apk" "$apk_temporary"
mv -f -- "$apk_temporary" "$apk_destination"
trap - EXIT
printf '%s\n' "$build_version" > "$repo_dir/dist/android/ConsoleCrypt.version"
(cd -- "$repo_dir/dist/android" && shasum -a 256 ConsoleCrypt-android-arm64.apk > ConsoleCrypt-android-arm64.apk.sha256)
echo "$repo_dir/dist/android/ConsoleCrypt-android-arm64.apk"
