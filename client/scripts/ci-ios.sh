#!/usr/bin/env bash
# One native build per CI job keeps CI_JOB_ID unique across desktop/iOS jobs.
# A separate, dynamically created Simulator prevents replacing a user's app.
set -euo pipefail
client_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
repo_dir="$(cd -- "$client_dir/.." && pwd)"
mode="${1:---test-only}"
if [[ "$mode" != '--test-only' && "$mode" != '--build-only' ]]; then
  echo 'Usage: ci-ios.sh [--test-only|--build-only]' >&2
  exit 2
fi
export PATH="/opt/homebrew/bin:/usr/local/bin:$HOME/.cargo/bin:$PATH"
export CARGO_INCREMENTAL=0
export IPHONEOS_DEPLOYMENT_TARGET=15.0
if [[ "$(uname -s)" != Darwin ]]; then
  echo 'iOS CI requires macOS, Xcode and an installed iOS Simulator runtime.' >&2
  exit 1
fi
xcrun --sdk iphonesimulator --show-sdk-path >/dev/null
for executable in flutter python3 rustup; do command -v "$executable" >/dev/null; done
if [[ "$mode" == '--test-only' ]]; then
  command -v docker >/dev/null
  docker info >/dev/null
fi
task_dir="$(mktemp -d "${TMPDIR:-/tmp}/consolecrypt-ios-ci.XXXXXX")"
simulator_id=''
fixture_pid=''
cleanup() {
  trap - EXIT
  if [[ -n "$fixture_pid" ]]; then
    kill "$fixture_pid" 2>/dev/null || true
    wait "$fixture_pid" 2>/dev/null || true
  fi
  if [[ -f "$task_dir/fixture-container" ]]; then
    docker rm -f "$(cat "$task_dir/fixture-container")" >/dev/null 2>&1 || true
  fi
  if [[ -f "$task_dir/fixture-network" ]]; then
    docker network rm "$(cat "$task_dir/fixture-network")" >/dev/null 2>&1 || true
  fi
  if [[ -n "$simulator_id" ]]; then
    xcrun simctl shutdown "$simulator_id" >/dev/null 2>&1 || true
    xcrun simctl delete "$simulator_id" >/dev/null 2>&1 || true
  fi
  rm -rf -- "$task_dir"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
xcrun simctl list runtimes -j > "$task_dir/runtimes.json"
xcrun simctl list devicetypes -j > "$task_dir/types.json"
python3 - "$task_dir" <<'PY'
import json
from pathlib import Path
import sys
root = Path(sys.argv[1])
runtimes = [item for item in json.loads((root/'runtimes.json').read_text())['runtimes']
            if item.get('isAvailable') and item['identifier'].startswith('com.apple.CoreSimulator.SimRuntime.iOS-')]
if not runtimes:
    raise SystemExit('Install an iOS Simulator runtime in Xcode before running iOS CI.')
runtime = max(runtimes, key=lambda item: tuple(map(int, item['version'].split('.'))))
# Runtime compatibility is checked by simctl create; prefer the latest iPhone
# type whose supported numeric runtime range includes the selected runtime.
version = tuple(map(int, runtime['version'].split('.')))
numeric = (version[0] << 16) + ((version[1] if len(version) > 1 else 0) << 8)
supported = {item['identifier'] for item in runtime.get('supportedDeviceTypes', [])}
types = [item for item in json.loads((root/'types.json').read_text())['devicetypes']
         if item.get('productFamily') == 'iPhone'
         and item.get('minRuntimeVersion', 0) <= numeric <= item.get('maxRuntimeVersion', 0xffffffff)
         and (not supported or item['identifier'] in supported)]
if not types:
    raise SystemExit('No compatible iPhone Simulator device type is installed.')
(root/'runtime').write_text(runtime['identifier'])
(root/'device-type').write_text(types[0]['identifier'])
PY
simulator_id="$(xcrun simctl create "ConsoleCrypt-CI-${CI_JOB_ID:-local}-$(date +%s)" "$(cat "$task_dir/device-type")" "$(cat "$task_dir/runtime")")"
export CI_TEST_DEVICES="$simulator_id"
xcrun simctl boot "$simulator_id"
xcrun simctl bootstatus "$simulator_id" -b >/dev/null
rustup target add aarch64-apple-ios-sim x86_64-apple-ios
cd -- "$client_dir/flutter"
flutter pub get
if [[ "$mode" == '--test-only' ]]; then
  fixture_source="$client_dir/rust/ssh-core/tests/docker"
  fixture_hash="$(python3 - "$fixture_source" <<'PY'
import hashlib
from pathlib import Path
import sys
root = Path(sys.argv[1])
digest = hashlib.sha256()
for name in ['Dockerfile', 'entrypoint.sh', 'sshd_config']:
    digest.update(name.encode())
    digest.update(root.joinpath(name).read_bytes())
print(digest.hexdigest()[:16])
PY
)"
  image="consolecrypt-ios-ssh-fixture:$fixture_hash"
  # Reuse only an image keyed by the exact fixture sources. This avoids
  # unnecessary registry metadata requests on repeated local/CI regressions.
  if ! docker image inspect "$image" >/dev/null 2>&1; then
    docker build -q -t "$image" "$fixture_source" >/dev/null
  fi
  # Runtime-generated test password stays in this process and Docker's
  # ephemeral environment. Neither command arguments nor Dart defines carry it.
  python3 - "$task_dir" "$image" <<'PY' &
import http.server
import json
import os
from pathlib import Path
import secrets
import signal
import socket
import subprocess
import sys
import time

root, image = Path(sys.argv[1]), sys.argv[2]
container = 'consolecrypt-ios-it-' + secrets.token_hex(8)
network = container + '-network'
root.joinpath('fixture-container').write_text(container)
root.joinpath('fixture-network').write_text(network)
password = secrets.token_urlsafe(32)
env = dict(os.environ, CC_IT_PASSWORD=password)
def docker(*args):
    return subprocess.run(['docker', *args], env=env, check=True,
                          stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True).stdout.strip()
try:
    # A dedicated bridge and a loopback-only published port isolate the
    # fixture from other test containers and external incoming connections.
    docker('network', 'create', '--label', 'consolecrypt.fixture=ios', network)
    with socket.socket() as reservation:
        reservation.bind(('127.0.0.1', 0))
        port = reservation.getsockname()[1]
    docker('run', '-d', '--name', container, '--label', 'consolecrypt.fixture=ios',
           '--network', network, '-p', f'127.0.0.1:{port}:22', '-e', 'CC_IT_PASSWORD', image)
    def ready():
        for _ in range(100):
            try:
                with socket.create_connection(('127.0.0.1', port), timeout=1) as connection:
                    if connection.recv(64).startswith(b'SSH-'): return
            except OSError: pass
            time.sleep(.1)
        raise RuntimeError('Generated SSH fixture did not start.')
    ready()
    fingerprint = docker('exec', container, 'ssh-keygen', '-lf',
                         '/etc/ssh/ssh_host_ed25519_key.pub', '-E', 'sha256').split()[1]
    config = {'port': port, 'username': 'tester', 'password': password,
              'fingerprint': fingerprint}
    class Control(http.server.BaseHTTPRequestHandler):
        def log_message(self, *args): pass
        def do_GET(self):
            if self.path != '/config': self.send_error(404); return
            body = json.dumps(config).encode()
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Cache-Control', 'no-store')
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        def do_POST(self):
            if self.path == '/stop': docker('stop', '-t', '1', container)
            elif self.path == '/start': docker('start', container); ready()
            else: self.send_error(404); return
            self.send_response(204)
            self.end_headers()
    server = http.server.HTTPServer(('127.0.0.1', 0), Control)
    root.joinpath('fixture-port').write_text(str(server.server_port))
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(0))
    server.serve_forever()
finally:
    subprocess.run(['docker', 'rm', '-f', container], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    subprocess.run(['docker', 'network', 'rm', network], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
PY
  fixture_pid=$!
  for ((attempt=0; attempt<120; attempt++)); do
    [[ -f "$task_dir/fixture-port" ]] && break
    kill -0 "$fixture_pid" 2>/dev/null || { echo 'Generated SSH fixture failed to start.' >&2; exit 1; }
    sleep 0.5
  done
  [[ -f "$task_dir/fixture-port" ]] || { echo 'Generated SSH fixture timed out.' >&2; exit 1; }
  export CC_BUILD_NUMBER="${CI_JOB_ID:-${CC_BUILD_NUMBER:-}}"
  [[ -n "$CC_BUILD_NUMBER" ]] || unset CC_BUILD_NUMBER
  test_version="$(python3 "$client_dir/scripts/bump-version.py" --root "$repo_dir")"
  echo "==> iOS native regression $test_version"
  flutter test integration_test/ios_terminal_test.dart -d "$CI_TEST_DEVICES" \
    --dart-define=CC_MOCK=false --dart-define="CC_IOS_FIXTURE_CONTROL_PORT=$(cat "$task_dir/fixture-port")"
  app='build/ios/iphonesimulator/Runner.app'
  actual_version="$(/usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' "$app/Info.plist")"
  actual_build="$(/usr/libexec/PlistBuddy -c 'Print CFBundleVersion' "$app/Info.plist")"
  if [[ "$actual_version+$actual_build" != "$test_version" ]]; then
    echo 'Native iOS regression bundle version differs from its reserved number.' >&2
    exit 1
  fi
else
  export CC_BUILD_NUMBER="${CI_JOB_ID:-${CC_BUILD_NUMBER:-}}"
  [[ -n "$CC_BUILD_NUMBER" ]] || unset CC_BUILD_NUMBER
  bash "$client_dir/scripts/build-ios.sh" --simulator
  app='build/ios/iphonesimulator/Runner.app'
  xcrun simctl install "$simulator_id" "$app"
  xcrun simctl launch "$simulator_id" io.consolecrypt.consolecrypt
fi
