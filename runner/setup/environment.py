"""Built-in local/WSL setup. Never reads a project or installs system packages."""
from __future__ import annotations

import hashlib
import importlib.util
import json
import os
from pathlib import Path, PurePosixPath
import platform
import pwd
import re
import shutil
import signal
import subprocess
import sys
import threading
import uuid


def private_directory(path, *, create=True):
    """Reject symlink/foreign/writable parents rather than follow them."""
    path = Path(path)
    home = Path(pwd.getpwuid(os.getuid()).pw_dir)
    if not home.is_absolute() or home.is_symlink() or home.stat().st_uid != os.getuid():
        raise ValueError('unsafe_home')
    if not path.is_relative_to(home) or os.getuid() == 0:
        raise ValueError('non_root_required')
    current = home
    for part in path.relative_to(home).parts:
        current /= part
        if create:
            try:
                current.mkdir(mode=0o700)
            except FileExistsError:
                pass
        info = current.lstat()
        if current.is_symlink() or not current.is_dir() or info.st_uid != os.getuid() or info.st_mode & 0o022:
            raise ValueError('unsafe_directory')
    return path


def managed_root():
    return Path(pwd.getpwuid(os.getuid()).pw_dir) / '.local/share/consolecrypt/runner-environments'


# A deliberately small data parser: never source/eval os-release or expand it.
# Missing/ambiguous/unsupported fields remain unknown; no Linux defaults inferred.
_OS_RELEASE_BYTES = 8192
_OS_RELEASE_FIELDS = {'ID': 'id', 'VERSION_ID': 'version_id', 'PRETTY_NAME': 'pretty_name'}


def _release_value(raw):
    if not raw:
        return None
    if raw[0] in ('"', "'"):
        quote = raw[0]
        if len(raw) < 2 or raw[-1] != quote:
            return None
        value = []
        index = 1
        while index < len(raw) - 1:
            char = raw[index]
            if char == quote:
                return None  # No concatenated quoted strings or trailing programs.
            if quote == '"' and char == "\\":
                index += 1
                if index >= len(raw) - 1 or raw[index] not in '\\"$`':
                    return None
                char = raw[index]
            elif quote == '"' and char in '$`':
                return None  # Unsupported unescaped shell syntax; never expand.
            value.append(char)
            index += 1
        value = ''.join(value)
    elif re.fullmatch(r'[A-Za-z0-9._-]+', raw):
        value = raw
    else:
        return None
    if not value or len(value) > 256 or any(not c.isprintable() for c in value):
        return None
    return value


def _read_os_release(path='/etc/os-release'):
    try:
        with Path(path).open('rb') as stream:
            raw = stream.read(_OS_RELEASE_BYTES + 1)
        if len(raw) > _OS_RELEASE_BYTES:
            return {}
        text = raw.decode('utf-8', errors='strict')
    except (OSError, UnicodeError):
        return {}
    fields = {}
    seen = set()
    for line in text.split('\n'):
        line = line.removesuffix('\r')
        if not line or line.startswith('#') or '=' not in line:
            continue
        key, raw_value = line.split('=', 1)
        field = _OS_RELEASE_FIELDS.get(key)
        if field is None:
            continue
        if field in seen:
            fields.pop(field, None)  # Ambiguous duplicates are unknown, even if equal.
            continue
        seen.add(field)
        value = _release_value(raw_value)
        if value is None or key == 'ID' and re.fullmatch(r'[a-z0-9._-]+', value) is None:
            continue
        fields[field] = value
    return fields


def _environment_key():
    version = f'{sys.version_info.major}.{sys.version_info.minor}'
    machine = platform.machine()
    if sys.platform == 'linux' and (machine == 'x86_64' and version in ('3.12', '3.14')
                                    or machine == 'aarch64' and version in ('3.12', '3.13')):
        return f'linux-{machine}-py{sys.version_info.major}{sys.version_info.minor}'
    return None


def scan(expected_package_fingerprint=None):
    uid = os.getuid()
    user = pwd.getpwuid(uid)
    users = [user.pw_name] if uid else [
        row.pw_name for row in pwd.getpwall()
        if 1000 <= row.pw_uid < 65534 and row.pw_dir.startswith('/home/')
        and re.fullmatch(r'[a-zA-Z0-9_.-]{1,64}', row.pw_name) and not row.pw_name.startswith('-')
        and row.pw_shell not in ('/bin/false', '/usr/sbin/nologin', '/sbin/nologin')
    ][:32]
    version = f'{sys.version_info.major}.{sys.version_info.minor}'
    machine = platform.machine()
    supported = _environment_key() is not None
    tools = {name: shutil.which(name, path='/usr/bin:/bin') is not None for name in ('bwrap', 'git', 'ssh')}
    missing = [name for name, available in tools.items() if not available]
    if not supported:
        missing.append('supported_python')
    if importlib.util.find_spec('venv') is None or importlib.util.find_spec('ensurepip') is None:
        missing.append('python_venv')
    if uid == 0:
        missing.append('non_root_user')
    if str(Path(user.pw_dir)).startswith('/mnt/'):
        missing.append('linux_home')
    try:
        if os.statvfs(user.pw_dir).f_flag & os.ST_NOEXEC:
            missing.append('executable_home')
    except OSError:
        missing.append('executable_home')
    if supported and uid and tools['bwrap']:
        try:
            tested = subprocess.run(['/usr/bin/bwrap', '--unshare-all', '--die-with-parent',
                                     '--ro-bind', '/', '/', '--', '/usr/bin/true'],
                                    stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                    stderr=subprocess.DEVNULL, timeout=8)
            if tested.returncode:
                missing.append('namespaces')
        except (OSError, subprocess.TimeoutExpired):
            missing.append('namespaces')
    candidates = []
    outdated = []
    root = managed_root()
    # Scan only ConsoleCrypt's bounded, private installation directory.
    if uid and root.is_dir() and not root.is_symlink():
        private_directory(root, create=False)
        for directory in sorted(root.iterdir())[:32]:
            if directory.is_symlink() or not directory.is_dir():
                continue
            marker = directory / 'installation.json'
            try:
                if marker.is_symlink() or marker.stat().st_size > 8192:
                    continue
                receipt = json.loads(marker.read_text(encoding='utf-8'))
                expected = {'python': str(directory / 'venv/bin/python'),
                            'module_dir': str(directory / 'src'), 'root': str(directory / 'state')}
                if isinstance(receipt, dict) and receipt.get('version') == 1 and receipt.get('paths') == expected and all(Path(p).exists() for p in expected.values()):
                    candidates.append(expected)
                    if expected_package_fingerprint is not None and receipt.get('package_fingerprint') != expected_package_fingerprint:
                        outdated.append(expected['python'])
            except (OSError, ValueError, TypeError):
                continue
    release = platform.release()
    os_release = _read_os_release()
    os_id = os_release.get('id', '')
    packages = [package for key, package in [('python_venv', 'python3-venv'), ('bwrap', 'bubblewrap'),
                ('git', 'git'), ('ssh', 'openssh-client')] if key in missing]
    command = ('sudo apt-get update && sudo apt-get install --no-install-recommends ' + ' '.join(packages)) \
        if os_id in ('ubuntu', 'debian') and packages else None
    return {'version': 1, 'user': user.pw_name, 'users': users, 'non_root': uid != 0,
            'python': platform.python_version(), 'executable': sys.executable,
            'system': platform.system(), 'machine': machine, 'release': release,
            'os_release': os_release,
            'wsl2': 'microsoft-standard' in release.lower() and 'wsl2' in release.lower(),
            'missing': missing, 'install_supported': not missing,
            'install_parent': str(root), 'candidates': candidates, 'outdated_candidates': outdated,
            'prerequisite_command': command}


def install(payload):
    report = scan()
    if not report['install_supported'] or payload.get('require_wsl2') and not report['wsl2']:
        raise ValueError('prerequisites_missing')
    key = _environment_key()
    if key is None:
        raise ValueError('prerequisites_missing')
    files = payload['files']
    if not isinstance(files, dict) or not 10 <= len(files) <= 128:
        raise ValueError('invalid_package')
    lock_name = f'requirements-{key}.lock'
    if lock_name not in files or 'src/consolecrypt_runner/__main__.py' not in files:
        raise ValueError('invalid_package')
    # Validate the entire built-in package BEFORE creating anything.
    total = 0
    for name, content in files.items():
        parts = PurePosixPath(name)
        if (not isinstance(content, str) or str(parts) != name or parts.is_absolute()
                or '..' in parts.parts or '\\' in name
                or not (name.startswith('src/consolecrypt_runner/') and name.endswith(('.py', '.json'))
                        or re.fullmatch(r'requirements-linux-(?:x86_64-py(?:312|314)|aarch64-py(?:312|313))\.lock', name))):
            raise ValueError('invalid_package')
        total += len(content.encode())
    if total > 4 * 1024 * 1024:
        raise ValueError('package_too_large')
    fingerprint = payload.get('package_fingerprint')
    if fingerprint is not None and (not isinstance(fingerprint, str) or re.fullmatch(r'[0-9a-f]{64}', fingerprint) is None):
        raise ValueError('invalid_package')
    parent = private_directory(managed_root())
    directory = parent / str(uuid.uuid4())
    directory.mkdir(mode=0o700)  # Exclusive; no existing environment is replaced.
    os.umask(0o077)
    for name, content in files.items():
        target = directory / name
        target.parent.mkdir(parents=True, mode=0o700, exist_ok=True)
        with target.open('x', encoding='utf-8', newline='\n') as stream:
            stream.write(content)
    clean_env = {'HOME': str(directory), 'PATH': '/usr/bin:/bin', 'LANG': 'C.UTF-8',
                 'PYTHONDONTWRITEBYTECODE': '1', 'PYTHONNOUSERSITE': '1',
                 'PIP_CONFIG_FILE': os.devnull}
    try:
        subprocess.run([sys.executable, '-I', '-m', 'venv', str(directory / 'venv')],
                       env=clean_env, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                       stderr=subprocess.DEVNULL, check=True, timeout=90)
        python = str(directory / 'venv/bin/python')
        subprocess.run([python, '-I', '-m', 'pip', '--isolated', 'install', '--no-input',
                        '--disable-pip-version-check', '--no-cache-dir', '--only-binary=:all:',
                        '--index-url', 'https://pypi.org/simple', '--require-hashes',
                        '-r', str(directory / lock_name)], env=clean_env, stdin=subprocess.DEVNULL,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True, timeout=600)
        expected = json.loads(files[f'src/consolecrypt_runner/environments/{key}.json'])
        if 'pip' not in expected['packages']:
            # pip bootstraps the venv but is not an execution dependency in
            # this lock. Remove it before comparing the full installed set.
            subprocess.run([python, '-I', '-m', 'pip', '--isolated', 'uninstall', '-y', 'pip'],
                           env=clean_env, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                           stderr=subprocess.DEVNULL, check=True, timeout=30)
        # Re-use the actual helper diagnostics (versions, files, lock and sandbox).
        # Metadata/hash checks alone cannot detect a noexec mount, an ABI or
        # shared-library load failure. Import the actual helper bootstrap too,
        # without calling main(), opening a listener or executing any project.
        check = "import sys,json;sys.path.insert(0,sys.argv[1]);from consolecrypt_runner import __main__ as bootstrap;from consolecrypt_runner.probe import probe;print(json.dumps(probe()))"
        process = subprocess.run([python, '-I', '-c', check, str(directory / 'src')],
                                 env=clean_env, stdin=subprocess.DEVNULL, capture_output=True,
                                 check=True, timeout=90)
        if len(process.stdout) > 65536:
            raise ValueError('invalid_probe')
        probe = json.loads(process.stdout)
        if (not probe['dependencies_ready'] or not probe['non_root']
                or not all(probe['sandbox'][key] for key in ('filesystem', 'network', 'pid'))):
            raise ValueError('environment_check_failed')
        (directory / 'state').mkdir(mode=0o700)
        paths = {'python': python, 'module_dir': str(directory / 'src'), 'root': str(directory / 'state')}
        receipt = {'version': 1, 'paths': paths,
                   'package_fingerprint': fingerprint,
                   'package_sha256': hashlib.sha256(json.dumps(files, sort_keys=True).encode()).hexdigest()}
        with (directory / 'installation.json').open('x', encoding='utf-8') as stream:
            json.dump(receipt, stream)
        return {'version': 1, 'paths': paths, 'probe': probe}
    except Exception:
        # Preserve a failed installation for diagnosis. Never publish it as a
        # ready candidate, delete arbitrary paths, or overwrite another Runner.
        raise ValueError('installation_failed') from None


def main():
    try:
        request = json.loads(sys.stdin.buffer.readline(4 * 1024 * 1024 + 1))
        if request == {'action': 'scan'}:
            result = scan()
        elif isinstance(request, dict) and set(request) == {'action', 'expected_package_fingerprint'} and request['action'] == 'scan':
            expected = request['expected_package_fingerprint']
            if not isinstance(expected, str) or re.fullmatch(r'[0-9a-f]{64}', expected) is None:
                raise ValueError('invalid_request')
            result = scan(expected)
        elif isinstance(request, dict) and request.get('action') == 'install':
            # The control pipe stays open for the operation. Parent exit/lock/
            # timeout closes it and kills only this private process group,
            # including pip. Never shut down a distro or another terminal.
            if os.getpgrp() != os.getpid():
                os.setsid()
            def watch_parent():
                os.read(sys.stdin.fileno(), 1)
                os.killpg(os.getpgrp(), signal.SIGTERM)
            threading.Thread(target=watch_parent, daemon=True).start()
            result = install(request)
        elif isinstance(request, dict) and request.get('action') in ('cleanup_scan', 'cleanup_delete'):
            # Native prepends the built-in cleanup source to -c; direct source
            # fixtures load only this fixed sibling, never a caller path.
            cleanup = globals()
            if 'cleanup_scan' not in cleanup:
                import runpy
                cleanup = runpy.run_path(str(Path(__file__).with_name('environment_cleanup.py')))
            if request['action'] == 'cleanup_scan':
                if set(request) != {'action', 'protected_paths', 'selected_python'}:
                    raise ValueError('cleanup_invalid_request')
                result = cleanup['cleanup_scan'](request['protected_paths'], request['selected_python'])
            else:
                result = cleanup['cleanup_delete'](request, cleanup['_CleanupControl'](sys.stdin.fileno()))
        else:
            raise ValueError('invalid_request')
        print(json.dumps({'ok': True, 'result': result}))
    except Exception as error:
        code = str(error) if isinstance(error, ValueError) and re.fullmatch(r'[a-z_]+', str(error)) else 'setup_failed'
        print(json.dumps({'ok': False, 'error': code}))
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
