"""Read-only fixed-host prerequisites for the explicit administrator role.

No unit start/stop, identity creation, repair, download or Agent admission.
It refuses occupied managed paths and preserves independent external services.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import socket
import stat
import subprocess
import sys


def need(value, code):
    if not value:
        raise ValueError(code)


def root(path, cap):
    path = Path(path)
    for item in (path, *path.parents):
        m = item.lstat()
        need(m.st_uid == 0 and not m.st_mode & 0o022 and not stat.S_ISLNK(m.st_mode), 'root physical custody')
        need(stat.S_ISDIR(m.st_mode) if item != path else stat.S_ISREG(m.st_mode), 'root path type')
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC)
    try:
        m = os.fstat(fd)
        need(stat.S_ISREG(m.st_mode) and m.st_nlink == 1 and 0 < m.st_size <= cap, 'bounded root input')
        data = bytearray()
        while len(data) <= cap:
            part = os.read(fd, min(65536, cap+1-len(data)))
            if not part:
                break
            data.extend(part)
        ident = lambda x: (x.st_dev, x.st_ino, x.st_mode, x.st_uid, x.st_nlink, x.st_size, x.st_mtime_ns, x.st_ctime_ns)
        need(len(data) == m.st_size and ident(m) == ident(os.fstat(fd)) == ident(path.lstat()), 'stable root file')
        return bytes(data)
    finally:
        os.close(fd)


def command(argv):
    # Only fixed systemctl show/version commands below. No shell or caller argv.
    result = subprocess.run(argv, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            timeout=5, check=False, env={'PATH':'/usr/bin:/bin', 'LANG':'C'})
    need(result.returncode in (0, 1, 4) and len(result.stdout) <= 16384 and len(result.stderr) <= 16384,
         'bounded system manager readback')
    return result.stdout.decode('ascii')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--agent-id', required=True)
    parser.add_argument('--worker-sha256', required=True)
    parser.add_argument('--adapter-sha256', required=True)
    args = parser.parse_args()
    need(os.getuid() == os.geteuid() == 0, 'explicit root preparation only')
    need(re.fullmatch('[0-9a-f]{64}', args.worker_sha256), 'expected normal worker digest')
    # Reuse installed original role validation helpers without its main().
    role = Path('/usr/libexec/consolecrypt-agent/enrollment-worker')
    source = root(role, 65536)
    need(re.fullmatch('[0-9a-f]{64}', args.adapter_sha256) and hashlib.sha256(source).hexdigest() in (
        'b74d2f235a44beb32ecd684cd681d69669a9d6003ac717ecd3ecdb6e86e842f3', args.adapter_sha256),
        'existing exact selected role adapter required; custom adapter preserved')
    namespace = {'__name__':'managed_exporter_existing_role', '__file__':str(role)}
    exec(compile(source, str(role), 'exec'), namespace)
    user, group = namespace['service']()
    need(all(user.pw_name not in g.gr_mem or g.gr_gid == group.gr_gid
             for g in __import__('grp').getgrall()), 'no supplementary groups')
    config = json.loads(root('/etc/consolecrypt-agent/enrollment-control.json', 16384))
    need(config == dict(format='cc-agent-enroll-config/v1', agentId=args.agent_id,
        nodeDir='/var/lib/consolecrypt-agent/identity/node', dbDir='/var/lib/consolecrypt-agent/identity/sqlite',
        anchorDir='/var/lib/consolecrypt-agent/checkpoints/enrollment',
        ownerPin='/etc/consolecrypt-agent/enrollment-genesis-owner.json'), 'existing exact enrolled Node config')
    # No new Node key/journal/owner pin is created or changed.
    for name in ('nodeDir', 'dbDir', 'anchorDir'):
        path = Path(config[name])
        for parent in (path, *path.parents):
            m = parent.lstat()
            need(stat.S_ISDIR(m.st_mode) and m.st_uid in (0,user.pw_uid) and not m.st_mode & 0o022,
                 'existing secure identity/journal ancestry')
        need(path.stat().st_uid == user.pw_uid and stat.S_IMODE(path.stat().st_mode) == 0o700,
             'existing private identity/journal directory')
    root(config['ownerPin'], 16384)
    launcher = Path('/opt/consolecrypt-agent-install/opt/consolecrypt-agent/launch.py')
    original = root(launcher, 1048576)
    module_spec = importlib.util.spec_from_file_location('managed_exporter_installed_launch', launcher)
    module = importlib.util.module_from_spec(module_spec)
    module_spec.loader.exec_module(module)
    worker = module.verified_identity_binary()
    actual = root(worker, 64*1024*1024)
    need(hashlib.sha256(actual).hexdigest() == args.worker_sha256 and root(launcher, 1048576) == original,
         'same independently verified normal worker required')
    need(stat.S_IMODE(worker.stat().st_mode) == 0o755, 'normal worker mode')
    try:
        os.getxattr(worker, 'security.capability', follow_symlinks=False)
    except OSError as error:
        import errno
        need(error.errno == errno.ENODATA, 'worker capability observation required')
    else:
        need(False, 'worker file capabilities forbidden')
    controllers = Path('/sys/fs/cgroup/cgroup.controllers').read_text().split()
    need({'cpu','memory','pids'} <= set(controllers), 'real unified resource controllers required')
    for path in (
        '/opt/consolecrypt-agent-managed-exporter', '/run/consolecrypt-agent/managed-exporter',
        '/etc/consolecrypt-agent/managed-node-exporter.json', '/etc/consolecrypt-agent/managed-exporter-runtime.json',
        '/usr/local/libexec/consolecrypt-agent-managed-exporter', '/run/consolecrypt-agent-managed-control',
        '/etc/tmpfiles.d/consolecrypt-agent-managed-exporter.conf',
        '/etc/systemd/system/consolecrypt-agent-managed-enroll.socket',
        '/etc/systemd/system/consolecrypt-agent-managed-enroll@.service',
    ):
        need(not os.path.lexists(path), 'managed preparation conflict: '+path)
    for unit in ('consolecrypt-agent-managed-enroll.socket', 'consolecrypt-agent-managed-enroll@.service'):
        need(command(['/usr/bin/systemctl','show',unit,'--property=LoadState','--value']).strip() == 'not-found',
             'managed systemd unit conflict')
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
        probe.bind(('127.0.0.1',19100))  # Existing exporter on any port is never adopted.
    # Existing standalone consolecrypt-node-exporter.service and /opt tree are
    # intentionally untouched. Its use of 19100 is a conflict above.
    print(json.dumps(dict(serviceUid=user.pw_uid, serviceGid=group.gr_gid, worker=str(worker)), sort_keys=True))


if __name__ == '__main__':
    main()
