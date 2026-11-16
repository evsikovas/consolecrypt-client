"""Observe only the fixed hosted Ubuntu ARM interpreter; NEVER approve its pin.
No package install, Cargo, service, fixture authority or production action.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import stat
import sys


def observe(output):
    assert sys.platform == 'linux' and os.uname().machine == 'aarch64' and sys.version_info[:2] == (3,12)
    assert os.environ.get('GITHUB_ACTIONS') == 'true' and os.environ.get('RUNNER_ARCH') == 'ARM64'
    assert os.environ.get('RUNNER_ENVIRONMENT') == 'github-hosted'
    release = {}
    for line in Path('/etc/os-release').read_text().splitlines():
        if '=' in line:
            name,value = line.split('=',1);release[name] = value.strip('"')
    assert release['ID'] == 'ubuntu' and release['VERSION_ID'] == '24.04'
    path = Path('/usr/bin/python3.12')
    named = path.lstat()
    assert stat.S_ISREG(named.st_mode) and named.st_uid == named.st_gid == 0
    assert stat.S_IMODE(named.st_mode) == 0o755 and named.st_nlink == 1 and 64 <= named.st_size <= 32*1024**2
    with path.open('rb') as stream:
        before = os.fstat(stream.fileno())
        header = stream.read(64);stream.seek(0)
        assert header[:7] == b'\x7fELF\x02\x01\x01' and int.from_bytes(header[18:20],'little') == 183
        sha = hashlib.file_digest(stream,'sha256').hexdigest()
        stamp = lambda m: (m.st_dev,m.st_ino,m.st_mode,m.st_uid,m.st_gid,m.st_nlink,m.st_size,m.st_mtime_ns,m.st_ctime_ns)
        assert stamp(named) == stamp(before) == stamp(os.fstat(stream.fileno())) == stamp(path.lstat())
    value = dict(candidateOnly=True,independentlyReviewed=False,architecture='aarch64',elfMachine=183,
        osRelease=dict(id=release['ID'],version=release['VERSION_ID']),pythonVersion=list(sys.version_info[:3]),
        pin=dict(format='cc-tunnel-supervisor-runtime/v1',pythonExecutable=str(path),pythonSha256=sha),
        installedAcceptanceExecuted=False,serviceStarted=False)
    # Deliberately NOT the product pin schema: build/host deny this whole
    # document until an independent reviewer selects the public inner pin.
    with output.open('x',encoding='utf-8') as stream:
        stream.write(json.dumps(value,sort_keys=True,indent=2)+'\n')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--observe-only',action='store_true',required=True)
    parser.add_argument('--output',type=Path,required=True)
    args = parser.parse_args()
    assert args.output.is_absolute() and args.output.parent.resolve(strict=True) == args.output.parent
    observe(args.output)
