#!/usr/bin/env bash
# Build in an isolated Linux container on a dedicated Docker-capable runner.
set -euo pipefail
repo_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
cd -- "$repo_dir"
command -v docker >/dev/null
command -v python3 >/dev/null
docker info >/dev/null
task_dir="$(mktemp -d "${TMPDIR:-/tmp}/consolecrypt-linux-ci.XXXXXX")"
finish() {
  local status=$?
  trap - EXIT
  # This helper emits a closed-schema, redacted receipt. Retain only that
  # JSON on failed tests; never export the test HOME, keyring or raw output.
  if ! python3 client/scripts/retain-linux-receipt.py "$task_dir/source" "$repo_dir"; then
    if [[ "$status" -eq 0 ]]; then status=1; fi
  fi
  if ! rm -rf -- "$task_dir"; then
    if [[ "$status" -eq 0 ]]; then status=1; fi
  fi
  exit "$status"
}
trap finish EXIT
python3 client/scripts/stage-linux-source.py --output "$task_dir/source"
docker build --platform linux/amd64 -t consolecrypt-linux-builder:3.47.5-rust1.98 client/ci/linux
docker run --rm --platform linux/amd64 \
  --env "CC_BUILD_NUMBER=${CC_BUILD_NUMBER:-${CI_JOB_ID:?Set CC_BUILD_NUMBER for a native CI build}}" \
  --mount "type=bind,source=$task_dir/source,target=/work" \
  consolecrypt-linux-builder:3.47.5-rust1.98 \
  bash -c 'python3 client/scripts/verify-release-identity.py && python3 -m unittest discover -s client/scripts -p test_release_identity.py -v && python3 -m unittest discover -s client/scripts -p test_linux_packaging.py -v && python3 -m unittest discover -s client/scripts/tests -p test_linux_native.py -v && python3 -m unittest discover -s client/scripts/tests -p test_linux_ci.py -v && CC_LINUX_DISPOSABLE_BUILDER=1 python3 client/scripts/test-linux-native.py --compiler-diagnostics --receipts /work/dist/linux/acceptance && bash client/scripts/build-linux.sh'
python3 client/scripts/export-linux-artifacts.py "$task_dir/source" "$repo_dir"
