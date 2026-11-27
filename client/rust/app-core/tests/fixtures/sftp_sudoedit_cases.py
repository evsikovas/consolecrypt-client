"""Run inside a fresh Ubuntu container with sudo and Python 3 installed."""
import hashlib
import os
from pathlib import Path
import secrets
import stat
import subprocess
import sys
import tempfile

if __name__ == '__main__':
    if os.getuid() != 0 or not Path('/.dockerenv').exists():
        sys.exit('This fixture requires a disposable Docker container.')
    helper = sys.argv[1]
    name = 'ccedit' + secrets.token_hex(4)
    password = secrets.token_urlsafe(24).encode()
    subprocess.run(['useradd', '-m', name], check=True)
    subprocess.run(['chpasswd'], input=name.encode() + b':' + password + b'\n', check=True)
    home = Path('/home') / name
    rule = Path('/etc/sudoers.d') / name

    def permit(commands):
        rule.write_text(f'{name} ALL=(root) {commands}\n')
        rule.chmod(0o440)

    def save(path, password_input, original, replacement, expected):
        result = subprocess.run(
            ['runuser', '-u', name, '--', 'python3', '-I', helper, str(path),
             hashlib.sha256(original).hexdigest()],
            input=password_input + b'\n' + replacement, timeout=55,
        )
        assert result.returncode == expected, (path, result.returncode, expected)

    with tempfile.TemporaryDirectory(prefix='cc-edit-', dir='/etc') as config:
        os.chmod(config, 0o755)
        system_file = Path(config) / 'fixture.conf'
        user_file = home / '123.py'
        for target in (system_file, user_file):
            target.write_bytes(b'original')
            target.chmod(0o640)
            os.chown(target, 0, home.stat().st_gid)
        permit('PASSWD: ALL')
        for target in (system_file, user_file):
            save(target, b'incorrect', b'original', b'wrong', 4)
            assert target.read_bytes() == b'original'
            save(target, password, b'original', b'new value', 0)
            assert target.read_bytes() == b'new value'
            info = target.stat()
            assert (info.st_uid, info.st_gid, stat.S_IMODE(info.st_mode)) == (0, home.stat().st_gid, 0o640)
            save(target, password, b'original', b'stale', 3)
            assert target.read_bytes() == b'new value'
        permit(f'NOPASSWD: sudoedit {user_file}')
        save(user_file, password, b'new value', b'not authorized', 6)
        assert user_file.read_bytes() == b'new value'
        permit('NOPASSWD: ALL')
        save(user_file, b'', b'new value', b'', 0)
        assert user_file.read_bytes() == b''
        assert user_file.stat().st_uid == 0
        assert not list(home.glob('.consolecrypt-*'))
        alias = home / 'alias'
        alias.symlink_to(user_file)
        save(alias, b'', b'', b'link', 3)
        os.link(user_file, home / 'hardlink')
        save(user_file, b'', b'', b'hardlink', 6)
        assert user_file.read_bytes() == b''
    print('PASS: writable home, system directory, owner/group/mode, password, policy, conflict, NOPASSWD, links')
