#!/usr/bin/env bash
# Native Linux x86-64 release. Run in a graphical Linux development environment
# or client/ci/linux/Dockerfile, never against a user's login keyring in CI.
set -euo pipefail
client_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
repo_dir="$(cd -- "$client_dir/.." && pwd)"
if [[ -d "$client_dir/rust/rdp-core" ]]; then
  python3 "$client_dir/scripts/verify-release-identity.py" --root "$repo_dir"
fi
if [[ "$(uname -s)" != Linux || "$(uname -m)" != x86_64 ]]; then
  echo 'This packaging entry point requires Linux x86-64.' >&2
  exit 1
fi
for tool in flutter cargo clang cmake ninja pkg-config python3 dpkg-deb rpmbuild; do
  command -v "$tool" >/dev/null || { echo "Missing build tool: $tool" >&2; exit 1; }
done
pkg-config --exists gtk+-3.0
# Failed build attempts consume their number, matching every native platform.
build_version="$(python3 "$client_dir/scripts/bump-version.py" --root "$repo_dir")"
echo "==> Linux version $build_version"
export CARGO_INCREMENTAL=0
cd -- "$client_dir/flutter"
flutter pub get
# Debug integration tests share Flutter's generated asset directory with
# release builds. Recreate it so their kernel cannot enter a release package.
rm -rf -- "$client_dir/flutter/build/flutter_assets"
flutter build linux --release --target-platform linux-x64 --dart-define=CC_MOCK=false
python3 "$client_dir/scripts/package-linux.py" --root "$repo_dir" \
  --bundle "$client_dir/flutter/build/linux/x64/release/bundle" \
  --output "$repo_dir/dist/linux" --version "$build_version"
