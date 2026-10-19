#!/usr/bin/env bash
# Source this in the setup step; exported GitHub paths apply to later steps.
set -euo pipefail
export PATH="/opt/homebrew/bin:/usr/local/bin:$HOME/.cargo/bin:$PATH"
if ! command -v flutter >/dev/null; then
  cc_flutter_sdk="${RUNNER_TOOL_CACHE:?}/consolecrypt-flutter-3.47.5"
  if [[ ! -d "$cc_flutter_sdk/.git" ]]; then
    git clone --depth 1 --branch 3.47.5 https://github.com/flutter/flutter.git "$cc_flutter_sdk"
  fi
  [[ "$(git -C "$cc_flutter_sdk" rev-parse HEAD)" == 6a19cca56475dbfba1478ee68d7bd0c2ef891da1 ]]
  export PATH="$cc_flutter_sdk/bin:$PATH"
  echo "$cc_flutter_sdk/bin" >> "$GITHUB_PATH"
fi
printf '%s\n' /opt/homebrew/bin /usr/local/bin "$HOME/.cargo/bin" >> "$GITHUB_PATH"
if [[ "${1:-}" == --xcode ]]; then
  if [[ -d /Applications/Xcode.app/Contents/Developer ]]; then
    export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
    echo "DEVELOPER_DIR=$DEVELOPER_DIR" >> "$GITHUB_ENV"
  fi
  xcodebuild -version
  command -v pod >/dev/null
fi
flutter --version
rustup show active-toolchain
