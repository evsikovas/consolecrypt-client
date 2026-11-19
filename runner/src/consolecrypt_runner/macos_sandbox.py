"""Native macOS worker isolation. Only the signed job's broker is reachable."""
from contextlib import ExitStack
import os
from pathlib import Path
import socket
import sys
import tempfile

from .errors import RunnerError, require
from .sandbox import run_bounded


SANDBOX_EXEC = '/usr/bin/sandbox-exec'


def _symlink_paths(path):
    pending = list(Path(path).parts[1:])
    current = Path('/')
    links = set()
    while pending:
        current /= pending.pop(0)
        if not current.is_symlink():
            continue
        require(current not in links and len(links) < 32, 'sandbox_python', 'Unexpected interpreter symlink chain')
        links.add(current)
        target = Path(os.path.abspath(current.parent / current.readlink()))
        pending = list(target.parts[1:]) + pending
        current = Path('/')
    return links


def run_macos_worker(workspace, command, *, tool_directory=None, broker=None, input_bytes=None, **limits):
    require(sys.platform == 'darwin' and os.getuid() != 0 and os.access(SANDBOX_EXEC, os.X_OK),
            'sandbox_unavailable', 'Native macOS sandbox is unavailable')
    with tempfile.TemporaryDirectory(prefix='cc-worker-', dir='/private/tmp') as directory:
        work = Path(directory)
        (work / 'tmp').mkdir(mode=0o700)
        source = Path(workspace).resolve(strict=True)
        tool = Path(tool_directory).resolve(strict=True) if tool_directory else work
        helper = Path(__file__).resolve().parent
        prefix = Path(sys.prefix).resolve(strict=True)
        base = Path(sys.base_prefix).resolve(strict=True)
        interpreter = Path(sys.executable).resolve(strict=True)
        # Limit readable interpreter files to supported Python installations.
        require(prefix != base and (base.is_relative_to('/opt/homebrew/Cellar/python@3.12') or
                base.is_relative_to('/usr/local/Cellar/python@3.12') or
                base == Path('/Library/Frameworks/Python.framework/Versions/3.12')),
                'sandbox_python', 'Use a Python 3.12 virtual environment from Homebrew or Python.org')
        profile = [
            '(version 1)', '(deny default)', '(allow process-fork)', '(allow process-exec*)',
            '(allow signal (target same-sandbox))', '(allow process-info* (target same-sandbox))',
            '(allow sysctl-read (sysctl-name-prefix "hw.") (sysctl-name "kern.osrelease")'
            ' (sysctl-name "kern.ostype") (sysctl-name "kern.osversion")'
            ' (sysctl-name "kern.osproductversion") (sysctl-name "kern.version")'
            ' (sysctl-name "kern.argmax") (sysctl-name "kern.maxfilesperproc")'
            ' (sysctl-name "kern.maxproc") (sysctl-name "kern.sysv.semmns"))',
            '(allow ipc-posix-sem)',
            '(allow mach-lookup (global-name "com.apple.system.opendirectoryd.libinfo"))',
            '(allow file-write-data (literal "/dev/null"))',
            '(deny system-fcntl (fcntl-command 80 110))',
        ]
        env = {
            'PATH': f'{sys.prefix}/bin:/usr/bin:/bin',
            'HOME': str(work / 'home'), 'TMPDIR': str(work / 'tmp'),
            'LANG': 'en_US.UTF-8', 'LC_ALL': 'en_US.UTF-8',
            'CC_WORK': str(work), 'CC_SOURCE': str(source), 'CC_TOOL': str(tool),
            'PYTHONPATH': str(helper.parent), 'PYTHONSAFEPATH': '1',
            'PYTHONNOUSERSITE': '1', 'PYTHONDONTWRITEBYTECODE': '1',
            'OBJC_DISABLE_INITIALIZE_FORK_SAFETY': 'YES',
            'ANSIBLE_HOME': str(work / '.ansible'),
            'ANSIBLE_LOCAL_TEMP': str(work / '.ansible/tmp'),
            'XDG_CACHE_HOME': str(work / '.cache'),
            'ANSIBLE_HOST_KEY_CHECKING': 'True',
            'ANSIBLE_INVENTORY_ENABLED': 'host_list,yaml,ini',
            'ANSIBLE_GALAXY_SERVER_LIST': '', 'ANSIBLE_FORCE_COLOR': 'False',
        }
        parameters = []

        ancestors = set()

        def path_rule(operation, filter_name, path):
            if operation.startswith('file-read*'):
                ancestors.update(Path(path).parents)
            name = f'CC_PATH_{len(parameters) // 2}'
            parameters.extend(['-D', f'{name}={path}'])
            profile.append(f'(allow {operation} ({filter_name} (param "{name}")))')
            return name

        for folder in (source, tool, work, '/usr/share'):
            path_rule('file-read*', 'subpath', folder)
        for folder in (helper, prefix, base, '/System', '/usr/lib', '/usr/bin',
                       '/bin', '/private/var/db/dyld'):
            path_rule('file-read* file-map-executable', 'subpath', folder)
        # dyld reads the root directory itself before it can load the system cache.
        path_rule('file-read*', 'literal', '/')
        # Homebrew's interpreter can live outside sys.base_prefix, and the
        # venv executable is reached through this fixed interpreter symlink.
        path_rule('file-read* file-map-executable', 'literal', interpreter)
        aliases = _symlink_paths(sys.executable)
        aliases.update(_symlink_paths(sys._base_executable))
        for homebrew in ('/opt/homebrew/opt', '/usr/local/opt'):
            for package in ('mpdecimal', 'openssl@3', 'readline', 'sqlite', 'xz', 'zstd', 'gettext', 'libb2'):
                library = Path(homebrew) / package / 'lib'
                if library.is_dir():
                    path_rule('file-read* file-map-executable', 'subpath', library.resolve(strict=True))
                    aliases.update(_symlink_paths(library))
        for file in ('/dev/null', '/dev/random', '/dev/urandom', '/private/etc/group',
                     '/private/etc/passwd', '/private/etc/localtime'):
            path_rule('file-read*', 'literal', file)
        path_rule('file-write*', 'subpath', work)
        if broker is not None:
            broker = Path(broker).resolve(strict=True)
            name = path_rule('file-read*', 'literal', broker)
            profile.extend(['(allow system-socket (socket-domain AF_UNIX))',
                            f'(allow network-outbound (remote unix-socket (literal (param "{name}"))))'])
            env['CC_BROKER_SOCKET'] = str(broker)
        for link in aliases:
            ancestors.update([link, *link.parents])
        for ancestor in sorted(ancestors):
            path_rule('file-read-metadata', 'literal', ancestor)
        argv = [SANDBOX_EXEC, *parameters, '-p', '\n'.join(profile), sys.executable,
                '-m', 'consolecrypt_runner.child', '--', *command]
        return run_bounded(argv, environment=env, cwd=work, input_bytes=input_bytes, **limits)


def probe_macos_sandbox():
    """Require the same profile to work and to isolate a worker from its parent."""
    with tempfile.TemporaryDirectory(prefix='cc-probe-', dir='/private/tmp') as directory, ExitStack() as sockets:
        root = Path(directory)
        project = root / 'project'
        project.mkdir()
        sample = project / 'sample'
        sample.write_text('project', encoding='utf-8')
        private = root / 'private'
        private.write_text('private', encoding='utf-8')
        allowed = root / 'broker.sock'
        blocked = root / 'other.sock'
        for address in (allowed, blocked):
            listener = sockets.enter_context(socket.socket(socket.AF_UNIX, socket.SOCK_STREAM))
            listener.bind(str(address))
            listener.listen()
        probe = '''
import os, pathlib, socket, sys
def denied(action):
    try:
        action()
    except PermissionError:
        return True
    return False
def connect(path):
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
        client.connect(path)
filesystem = (pathlib.Path('sample').read_text() == 'project'
              and pathlib.Path('written').write_text('scratch') == 7
              and denied(lambda: pathlib.Path(sys.argv[1]).read_text())
              and denied(lambda: pathlib.Path(sys.argv[2]).write_text('changed')))
connect(sys.argv[3])
def bind_ip():
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as client:
        client.bind(('127.0.0.1', 0))
network = denied(bind_ip)
network = network and denied(lambda: connect(sys.argv[4]))
process = denied(lambda: os.kill(os.getppid(), 0))
print(f'filesystem={filesystem}, network={network}, process={process}', flush=True)
sys.exit(0 if filesystem and network and process else 1)
'''
        try:
            result = run_macos_worker(project, [sys.executable, '-c', probe, str(private), str(sample),
                                               str(allowed), str(blocked)], broker=allowed,
                                      timeout=8, output_limit=4096, grace=1)
            available = result.exit_code == 0
        except (OSError, RunnerError):
            available = False
    return {'filesystem': available, 'pid': False, 'network': available, 'process_control': available,
            'reason': None if available else 'macos_sandbox_unavailable'}
