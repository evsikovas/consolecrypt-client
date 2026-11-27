"""Run as the connected SSH user. sudoedit owns the privileged copy-back."""
import os
from pathlib import Path
import selectors
import shutil
import stat
import subprocess
import sys
import tempfile
import time

LIMIT = 4 * 1024 * 1024

# sudoedit runs this editor as the invoking user, on its private working copy.
EDITOR = """
import hashlib
import os
from pathlib import Path
import sys

work = Path(__file__).parent
source = Path(os.environ['CC_SUDOEDIT_SOURCE'])
revision = os.environ['CC_SUDOEDIT_DIGEST']
copy = Path(sys.argv[1])
def matches(path):
    with path.open('rb') as stream:
        return hashlib.sha256(stream.read(4 * 1024 * 1024 + 1)).hexdigest() == revision

if len(sys.argv) != 2 or not matches(copy) or source.resolve() != source or not matches(source):
    (work / 'changed').touch()
    sys.exit(1)
copy.write_bytes((work / 'document').read_bytes())
(work / 'edited').touch()
"""


def run_sudoedit(sudo, path, env, password):
    # With NOPASSWD, sudo never reads stdin. Only send the password in
    # response to sudo's prompt, so it cannot become input to an editor.
    prompt = b'[consolecrypt-sudo]'
    args = [sudo, '-S', '-k', '-p', prompt.decode('ascii'), '-e', '--', str(path)]
    with subprocess.Popen(args, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL,
                          stderr=subprocess.PIPE, env=env) as proc:
        deadline = time.monotonic() + 45
        tail = b''
        try:
            with selectors.DefaultSelector() as poll:
                poll.register(proc.stderr, selectors.EVENT_READ)
                while time.monotonic() < deadline:
                    if not poll.select(max(0, deadline - time.monotonic())):
                        break
                    chunk = os.read(proc.stderr.fileno(), 4096)
                    if not chunk:
                        return proc.wait(timeout=max(0, deadline - time.monotonic()))
                    tail += chunk
                    if prompt in tail and not proc.stdin.closed:
                        proc.stdin.write(password)
                        proc.stdin.close()
                    tail = tail[-(len(prompt) - 1):]
            return 1
        finally:
            if proc.poll() is None:
                proc.kill()


def save(path, digest):
    sudo = shutil.which('sudo', path=os.defpath)
    if not sudo:
        return 127
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or info.st_mode & 0o7000 or path.resolve() != path:
        return 3
    password = sys.stdin.buffer.readline(4098)
    data = sys.stdin.buffer.read(LIMIT + 1)
    if not password.endswith(b'\n') or len(password) > 4097 or len(data) > LIMIT:
        return 1
    # Use a predictable character set for SUDO_EDITOR's command string. Keep
    # the directory itself unique and private even on shared SSH hosts.
    with tempfile.TemporaryDirectory(prefix='cc-edit-', dir='/tmp') as directory:
        work = Path(directory)
        (work / 'document').write_bytes(data)
        editor = work / 'edit.py'
        editor.write_text(EDITOR, encoding='utf-8')
        env = dict(os.environ, SUDO_EDITOR=f'{sys.executable} -I {editor}',
                   CC_SUDOEDIT_SOURCE=str(path), CC_SUDOEDIT_DIGEST=digest)
        status = run_sudoedit(sudo, path, env, password)
        if (work / 'changed').exists():
            return 3
        if status or not (work / 'edited').exists():
            return 4
        with path.open('rb') as stream:
            return 0 if stream.read(LIMIT + 1) == data else 3


if __name__ == '__main__':
    try:
        sys.exit(save(Path(sys.argv[1]), sys.argv[2]))
    except (OSError, subprocess.SubprocessError):
        sys.exit(1)
