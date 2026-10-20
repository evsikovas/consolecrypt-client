#!/usr/bin/env bash
# Isolated simulator/API fixture; diagnostic tags never publish an installer.
set -euo pipefail
client_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"
export CARGO_INCREMENTAL=0
export IPHONEOS_DEPLOYMENT_TARGET=15.0
xcrun --sdk iphonesimulator --show-sdk-path >/dev/null
task_dir="$(mktemp -d "${TMPDIR:-/tmp}/consolecrypt-kube-ios.XXXXXX")"
chmod 700 "$task_dir"
simulator_id=''
finish() {
  # Stop only this test's simulator. Retain diagnostics after failed attempts.
  if [[ -n "$simulator_id" ]]; then xcrun simctl shutdown "$simulator_id" >/dev/null 2>&1 || true; fi
}
trap finish EXIT
xcrun simctl list runtimes -j > "$task_dir/runtimes.json"
xcrun simctl list devicetypes -j > "$task_dir/types.json"
python3 - "$task_dir" <<'PY'
import json
from pathlib import Path
import sys
root = Path(sys.argv[1])
runtimes = [r for r in json.loads((root/'runtimes.json').read_text())['runtimes']
            if r.get('isAvailable') and '.iOS-' in r['identifier']]
if not runtimes:
    raise SystemExit('No installed iOS simulator runtime')
runtime = max(runtimes, key=lambda r: tuple(map(int, r['version'].split('.'))))
version = tuple(map(int, runtime['version'].split('.')))
numeric = (version[0] << 16) + ((version[1] if len(version) > 1 else 0) << 8)
supported = {t['identifier'] for t in runtime.get('supportedDeviceTypes', [])}
types = [t for t in json.loads((root/'types.json').read_text())['devicetypes']
         if t.get('productFamily') == 'iPhone' and
         t.get('minRuntimeVersion', 0) <= numeric <= t.get('maxRuntimeVersion', 0xffffffff) and
         (not supported or t['identifier'] in supported)]
if not types:
    raise SystemExit('No compatible iPhone simulator type')
(root/'runtime').write_text(runtime['identifier'])
(root/'type').write_text(types[0]['identifier'])
(root/'tls.cnf').write_text('''[req]
distinguished_name = dn
x509_extensions = tls
prompt = no
[dn]
CN = consolecrypt-kubernetes-fixture
[tls]
subjectAltName = IP:127.0.0.1
basicConstraints = critical,CA:FALSE
keyUsage = critical,digitalSignature
extendedKeyUsage = serverAuth
''')
PY
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -days 2 \
  -config "$task_dir/tls.cnf" -keyout "$task_dir/key.pem" -out "$task_dir/cert.pem" 2>/dev/null
python3 - "$task_dir" <<'PY'
import json
from pathlib import Path
import sys
root = Path(sys.argv[1])
(root/'defines.json').write_text(json.dumps({
    'CC_KUBE_TLS_CERT': (root/'cert.pem').read_text(),
    'CC_KUBE_TLS_KEY': (root/'key.pem').read_text(),
}))
(root/'defines.json').chmod(0o600)
PY
simulator_id="$(xcrun simctl create "ConsoleCrypt-Kubernetes-${GITHUB_RUN_ID:-local}-$(date +%s)" "$(cat "$task_dir/type")" "$(cat "$task_dir/runtime")")"
printf '%s\n' "$simulator_id" > "$task_dir/simulator-id"
xcrun simctl boot "$simulator_id"
xcrun simctl bootstatus "$simulator_id" -b >/dev/null
rustup target add aarch64-apple-ios-sim x86_64-apple-ios
cd "$client_dir/flutter"
flutter pub get
flutter test integration_test/kubernetes_ios_test.dart -d "$simulator_id" \
  --dart-define=CC_MOCK=false --dart-define=CC_KUBERNETES=true \
  --dart-define-from-file="$task_dir/defines.json"
