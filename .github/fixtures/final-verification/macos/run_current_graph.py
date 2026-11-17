"""Opt-in current DLL graph proof: own Memory profile/loopback, no app build."""
import argparse
import hashlib
import json
import os
import shutil
import signal
import sys
import tempfile
from pathlib import Path
import subprocess
import time

HERE = Path(__file__).resolve().parent
ROOT = Path(os.environ.get('RUNNER_TEMP', tempfile.gettempdir())).resolve(strict=True)
ABI = 1141874734
APPROVED_SHA = None  # Bind only after the actual final application build.
parser = argparse.ArgumentParser()
parser.add_argument('--root', required=True)
parser.add_argument('--dll', required=True)
parser.add_argument('--expected-sha', required=True)
parser.add_argument('--attempt', type=int, default=1)
args = parser.parse_args()
assert type(APPROVED_SHA) is str and len(APPROVED_SHA) == 64 and all(c in '0123456789abcdef' for c in APPROVED_SHA)
assert args.expected_sha == APPROVED_SHA
CLIENT = Path(args.root).resolve(strict=True)
assert sys.platform == 'darwin' and sys.version_info >= (3, 13)
assert hasattr(os, 'waitid') and hasattr(os, 'WNOWAIT')
if os.environ.get('GITHUB_WORKSPACE'):
    assert CLIENT == Path(os.environ['GITHUB_WORKSPACE']).resolve(strict=True)
assert (CLIENT / 'client/flutter/pubspec.yaml').is_file()
FLUTTER = CLIENT / 'client/flutter'
assert 1 <= args.attempt <= 9
sha = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
dll = Path(args.dll).resolve(strict=True)
assert dll == (CLIENT / 'dist/macos/ConsoleCrypt.app/Contents/Frameworks/cc_bridge.framework/Versions/A/cc_bridge').resolve(strict=True)
assert sha(dll) == args.expected_sha
assert f'rustContentHash => {ABI};' in (FLUTTER / 'lib/src/rust/frb_generated.dart').read_text(encoding='utf-8')
test = HERE / 'current_graph_test.dart'
prefix = f'monitoring-final-macos-packaged-graph-ffi-attempt{args.attempt}'
log = ROOT / f'{prefix}.log'
codefile = ROOT / f'{prefix}.exit'
assert not log.exists() and not codefile.exists(), 'Earlier attempts must be preserved.'
version = (CLIENT / 'dist/macos/ConsoleCrypt.version').read_text().strip()
assert version == '1.2.0+67027', 'Use the ONE reviewed Windows/Mac version reservation'
flutter_command = shutil.which('flutter')
assert flutter_command is not None
sdk_bin = Path(flutter_command).resolve(strict=True).parent
dart = sdk_bin / 'cache/dart-sdk/bin/dart'
flutter_tools = sdk_bin / 'cache/flutter_tools.snapshot'
assert dart.is_file() and os.access(dart, os.X_OK) and flutter_tools.is_file()
command = [str(dart), str(flutter_tools), 'test', str(test),
           '--no-pub', '--reporter', 'expanded', '--concurrency', '1']

# Keep the original leader unreaped while retiring ONLY its owned process group.
# This is the POSIX counterpart of the existing owner timeout tree cleanup.
def group_members(group, cleanup_end):
    remaining = cleanup_end - time.monotonic()
    assert remaining > 0, 'Original owned process group cleanup deadline'
    done = subprocess.run(['/bin/ps', '-axo', 'pid=,pgid=,uid='],
                          capture_output=True, timeout=min(5, remaining), check=True)
    assert time.monotonic() <= cleanup_end, 'Original owned process group cleanup deadline'
    assert len(done.stdout) <= 4*1024*1024 and len(done.stderr) <= 4096
    lines = done.stdout.decode('ascii').splitlines(); assert len(lines) <= 32768
    rows = [tuple(map(int, line.split())) for line in lines if line.strip()]
    assert all(len(row) == 3 for row in rows)
    members = []
    for pid, pgid, uid in rows:
        if pgid == group:
            assert uid == os.geteuid(), 'foreign group UID'
            members.append(pid)
    assert len(members) <= 64
    return set(members)

proof_root = Path(tempfile.mkdtemp(prefix='monitoring-final-packaged-g2-', dir=ROOT))
assert proof_root.parent == ROOT and not proof_root.is_symlink()
assert proof_root.stat().st_uid == os.geteuid() and proof_root.stat().st_mode & 0o777 == 0o700
env = dict(os.environ, CC_OWNED_MONITORING_ROOT=str(proof_root), CC_OWNED_MONITORING_DLL=str(dll), CC_OWNED_MONITORING_DLL_SHA256=args.expected_sha,
           CI='true', FLUTTER_SUPPRESS_ANALYTICS='true', FLUTTER_SKIP_UPDATE_CHECK='true', DART_SUPPRESS_ANALYTICS='true')
started = time.monotonic()
timed_out = False
joined_group = False
original_code = None
with log.open('x', encoding='utf-8') as output:
    child = subprocess.Popen(command, cwd=FLUTTER, env=env, stdout=output,
                             stderr=subprocess.STDOUT, start_new_session=True)
    try:
        assert os.getpgid(child.pid) == child.pid
        until = started + 210
        while time.monotonic() < until:
            status = os.waitid(os.P_PID, child.pid, os.WEXITED|os.WNOHANG|os.WNOWAIT)
            if status is not None:
                original_code = status.si_status if status.si_code == os.CLD_EXITED else -status.si_status
                break
            time.sleep(.05)
        else:
            timed_out = True
    finally:
        cleanup_end = time.monotonic() + 15
        # WNOWAIT retains the leader identity, so its PGID cannot be reused.
        status = os.waitid(os.P_PID, child.pid, os.WEXITED|os.WNOHANG|os.WNOWAIT)
        if status is None or group_members(child.pid, cleanup_end) != {child.pid}:
            try: os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError: pass
        while True:
            assert time.monotonic() < cleanup_end, 'Original owned process group did not join'
            status = os.waitid(os.P_PID, child.pid, os.WEXITED|os.WNOHANG|os.WNOWAIT)
            if status is not None and group_members(child.pid, cleanup_end) == {child.pid}: break
            time.sleep(min(.05, max(0, cleanup_end-time.monotonic())))
        remaining = cleanup_end - time.monotonic()
        assert remaining > 0, 'Original owned process group cleanup deadline'
        actual_code = child.wait(timeout=min(1, remaining))
        assert not group_members(child.pid, cleanup_end)
        joined_group = True
        if original_code is not None: assert actual_code == original_code
        code = 124 if timed_out else actual_code
report = None
for line in log.read_text(encoding='utf-8', errors='replace').splitlines():
    marker = 'OWNED_CURRENT_MONITORING_GRAPH_PROOF '
    if marker in line:
        report = json.loads(line.split(marker, 1)[1])
receipt = dict(exitCode=code, timedOut=timed_out, seconds=round(time.monotonic()-started, 3),
               nativeSha256=sha(dll), abi=ABI, testSha256=sha(test), logSha256=sha(log),
               childProof=report, remainingOwnedFixtures=[p.name for p in proof_root.glob('owned-*') if p.is_dir()],
               ownedProcessGroupJoined=joined_group, originalChildExitCode=original_code,
               actualHostArchitecture=os.uname().machine, otherUniversalSliceExecuted=False,
               applicationBuild=False, applicationDistribution=False)
codefile.write_text(f'{code}\n', encoding='utf-8')
(ROOT / f'{prefix}-receipt.json').write_text(json.dumps(receipt, indent=2)+'\n', encoding='utf-8')
assert not receipt['remainingOwnedFixtures'], 'Original G2 fixture cleanup did not complete'
proof_root.rmdir()
assert not proof_root.exists()
print(json.dumps(receipt))
print(log.read_text(encoding='utf-8', errors='replace')[-6000:])
raise SystemExit(code)
