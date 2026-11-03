#!/usr/bin/env bash
# Disposable Linux builder only: public digest-pinned engine/model, synthetic
# inference, no production profiles or user HOME. The container owns cleanup.
set -euo pipefail
repo_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
[[ "$(uname -s)" == Linux ]] || { echo 'Linux AI smoke requires Linux' >&2; exit 1; }
task_dir="$(mktemp -d /tmp/consolecrypt-linux-ai.XXXXXX)"
cd "$repo_dir/client/flutter"
dart run test/ai/local/local_ai_smoke.dart --run --directory="$task_dir" --request-timeout-seconds=240
mkdir -p "$repo_dir/dist/linux/acceptance"
python3 - "$repo_dir/dist/linux/acceptance/local-ai-smoke.json" <<'PY'
import json
from pathlib import Path
import sys
Path(sys.argv[1]).write_text(json.dumps({
    'schema': 1, 'result': 'passed', 'platform': 'linux-x64', 'runtime': 'b11739',
    'checks': ['archive_and_model_hashes', 'inference', 'token_authentication',
               'single_flight_start', 'active_lease', 'idle_unload'],
}, indent=2) + '\n', encoding='utf-8')
PY
