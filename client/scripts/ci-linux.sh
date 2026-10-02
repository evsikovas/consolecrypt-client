#!/usr/bin/env bash
# Reuse the registered Mac runner's Docker, with isolated Linux SDK/source state.
set -euo pipefail
repo_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
cd -- "$repo_dir"
command -v docker >/dev/null
command -v python3 >/dev/null
docker info >/dev/null
task_dir="$(mktemp -d "${TMPDIR:-/tmp}/consolecrypt-linux-ci.XXXXXX")"
trap 'rm -rf -- "$task_dir"' EXIT
python3 client/scripts/stage-linux-source.py --output "$task_dir/source"
docker build --platform linux/amd64 -t consolecrypt-linux-builder:3.47.5-rust1.98 client/ci/linux
docker run --rm --platform linux/amd64 \
  --env "CC_BUILD_NUMBER=${CI_JOB_ID:?Run this entry point as a GitLab native build job}" \
  --mount "type=bind,source=$task_dir/source,target=/work" \
  consolecrypt-linux-builder:3.47.5-rust1.98 \
  bash -c 'python3 -m unittest discover -s client/scripts -p test_linux_packaging.py -v && python3 -m unittest discover -s client/scripts/tests -p test_linux_native.py -v && CC_LINUX_DISPOSABLE_BUILDER=1 python3 client/scripts/test-linux-native.py --receipts /work/dist/linux/acceptance && bash client/scripts/build-linux.sh'
mkdir -p dist/linux
cp "$task_dir/source/dist/linux/"*.deb "$task_dir/source/dist/linux/"*.rpm \
  "$task_dir/source/dist/linux/"*.SHA256SUMS "$task_dir/source/dist/linux/"*.json \
  "$task_dir/source/dist/linux/ConsoleCrypt.version" dist/linux/
mkdir -p dist/linux/acceptance
cp "$task_dir/source/dist/linux/acceptance/linux-ffi-integration.json" \
  dist/linux/acceptance/
python3 - "$repo_dir" <<'PY'
import json, os, pathlib, subprocess, sys
root = pathlib.Path(sys.argv[1])
version = (root/'dist/linux/ConsoleCrypt.version').read_text().strip()
receipt = json.loads((root/f'dist/linux/ConsoleCrypt-{version}-linux-x64.json').read_text())
receipt['source_commit'] = subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()
receipt['ci_job_id'] = int(os.environ['CI_JOB_ID'])
(root/f'dist/linux/ConsoleCrypt-{version}-linux-x64.json').write_text(json.dumps(receipt,indent=2)+'\n')
PY
