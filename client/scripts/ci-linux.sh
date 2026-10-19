#!/usr/bin/env bash
# Build in an isolated Linux container on a dedicated Docker-capable runner.
set -euo pipefail
repo_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
cd -- "$repo_dir"
command -v docker >/dev/null
command -v python3 >/dev/null
docker info >/dev/null
# ARC's Docker sidecar shares _work, not the runner container's /tmp.
# RUNNER_TEMP lives on that shared volume; a standalone runner keeps /tmp.
LABELS=()
if [[ "${GITHUB_ACTIONS:-}" == true ]]; then
  task_dir="$(python3 client/scripts/cleanup-linux-job.py --workspace "$repo_dir" --prepare)"
  LABELS=(--label "io.consolecrypt.ci.repository=$GITHUB_REPOSITORY"
    --label "io.consolecrypt.ci.run=$GITHUB_RUN_ID"
    --label "io.consolecrypt.ci.attempt=$GITHUB_RUN_ATTEMPT"
    --label "io.consolecrypt.ci.job=$GITHUB_JOB")
else
  task_dir="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/consolecrypt-linux-ci.XXXXXX")"
fi
container_name="${task_dir##*/}"
finish() {
  local status=$?
  trap - EXIT
  if [[ "${GITHUB_ACTIONS:-}" == true ]]; then
    # Stop only compute; retain scratch even on failure. The workflow repeats
    # this independently if cancellation kills this shell.
    if ! python3 client/scripts/cleanup-linux-job.py --workspace "$repo_dir"; then
      if [[ "$status" -eq 0 ]]; then status=1; fi
    fi
    exit "$status"
  fi
  # Stop only this task's uniquely named container if docker run was interrupted.
  # A successful --rm run is already gone; never prune shared images or volumes.
  docker rm --force "$container_name" >/dev/null 2>&1 || true
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
trap 'exit 130' INT
trap 'exit 143' TERM
python3 client/scripts/stage-linux-source.py --output "$task_dir/source"
docker build --platform linux/amd64 -t consolecrypt-linux-builder:3.47.5-rust1.98 client/ci/linux
docker run --rm --name "$container_name" ${LABELS[@]+"${LABELS[@]}"} --platform linux/amd64 \
  --env "CC_BUILD_NUMBER=${CC_BUILD_NUMBER:-${CI_JOB_ID:?Set CC_BUILD_NUMBER for a native CI build}}" \
  --env "CC_OUTPUT_UID=$(id -u)" --env "CC_OUTPUT_GID=$(id -g)" \
  --mount "type=bind,source=$task_dir/source,target=/work" \
  consolecrypt-linux-builder:3.47.5-rust1.98 \
  bash -c 'trap '"'"'chown -R "$CC_OUTPUT_UID:$CC_OUTPUT_GID" /work'"'"' EXIT; python3 client/scripts/verify-release-identity.py && python3 -m unittest discover -s client/scripts -p test_release_identity.py -v && python3 -m unittest discover -s client/scripts -p test_linux_packaging.py -v && python3 -m unittest discover -s client/scripts/tests -p test_linux_native.py -v && python3 -m unittest discover -s client/scripts/tests -p test_linux_ci.py -v && CC_LINUX_DISPOSABLE_BUILDER=1 python3 client/scripts/test-linux-native.py --compiler-diagnostics --receipts /work/dist/linux/acceptance && bash client/scripts/build-linux.sh'
python3 client/scripts/export-linux-artifacts.py "$task_dir/source" "$repo_dir"
