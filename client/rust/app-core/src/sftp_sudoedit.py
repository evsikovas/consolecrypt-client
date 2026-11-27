"""Save an SSH editor buffer as the connected user, with explicit sudo authorization."""
from contextlib import contextmanager
import hashlib
import os
from pathlib import Path
import secrets
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


# These are protocol outcomes, never raw sudo output or remote file contents.
CHANGED, BAD_PASSWORD, DENIED, WRITABLE_DIR = 3, 4, 5, 6


def run_sudo(sudo, args, env, password, deadline, cwd=None):
    # With NOPASSWD, sudo never reads stdin. Only send the password in
    # response to sudo's prompt, so it cannot become input to an editor.
    prompt = b'[consolecrypt-sudo]'
    args = [sudo, '-S', '-k', '-p', prompt.decode('ascii'), *args]
    with subprocess.Popen(args, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, cwd=cwd,
                          stderr=subprocess.PIPE, env=env) as proc:
        tail = b''
        failure = DENIED
        try:
            with selectors.DefaultSelector() as poll:
                poll.register(proc.stderr, selectors.EVENT_READ)
                while time.monotonic() < deadline:
                    if not poll.select(max(0, deadline - time.monotonic())):
                        break
                    chunk = os.read(proc.stderr.fileno(), 4096)
                    if not chunk:
                        status = proc.wait(timeout=max(0, deadline - time.monotonic()))
                        return failure if status else 0
                    tail += chunk
                    if (b'editing files in a writable directory is not permitted' in tail
                            or b'cannot open a file in a path writable by the user' in tail):
                        failure = WRITABLE_DIR
                    elif any(msg in tail.lower() for msg in (
                            b'sorry, try again', b'incorrect password', b'incorrect authentication',
                            b'authentication failure', b'authentication failed')):
                        failure = BAD_PASSWORD
                    if prompt in tail and not proc.stdin.closed:
                        proc.stdin.write(password)
                        proc.stdin.close()
                    tail = tail[-96:]
            return 1
        finally:
            if proc.poll() is None:
                proc.kill()


@contextmanager
def open_directory(path):
    fd = os.open(path, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        yield fd
    finally:
        os.close(fd)


def file_attrs(fd):
    return {key: os.getxattr(fd, key) for key in os.listxattr(fd)}


def save_in_writable_directory(sudo, path, digest, data, password, deadline):
    # The SSH user can replace an entry in this directory. Only assigning the
    # original owner to our staging file needs separate sudo authorization.
    chown = shutil.which('chown', path=os.defpath)
    if not chown or not sys.platform.startswith('linux') or not os.path.isdir('/proc/self/fd'):
        return WRITABLE_DIR
    if not os.access(path.parent, os.W_OK | os.X_OK):
        return WRITABLE_DIR
    with open_directory(path.parent) as parent:
        fd = os.open(path.name, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW, dir_fd=parent)
        with os.fdopen(fd, 'rb') as original:
            info = os.fstat(fd)
            if (not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or info.st_nlink != 1
                    or info.st_mode & 0o7000):
                return WRITABLE_DIR
            if info.st_size > LIMIT or hashlib.sha256(original.read(LIMIT + 1)).hexdigest() != digest:
                return CHANGED
            attrs = file_attrs(fd)
            with tempfile.TemporaryDirectory(prefix='.consolecrypt-', dir=f'/proc/self/fd/{parent}') as tmp:
                with open_directory(tmp) as private:
                    return replace_from_stage(sudo, chown, parent, private, path, info, attrs, data, password, deadline)


def replace_from_stage(sudo, chown, parent, private, path, original, attrs, data, password, deadline):
    name = secrets.token_hex(16)
    fd = os.open(name, os.O_CREAT | os.O_EXCL | os.O_RDWR, 0o600, dir_fd=private)
    with os.fdopen(fd, 'wb') as out:
        out.write(data)
        out.flush()
        os.fchmod(fd, stat.S_IMODE(original.st_mode))
        # Do not silently strip an ACL, capability, or security label.
        if file_attrs(fd) != attrs:
            return WRITABLE_DIR
        # The privileged utility works in the pinned private directory and
        # never receives the user's destination path or follows a symlink.
        args = ['--', chown, '-h', f'--from={os.getuid()}', f'{original.st_uid}:{original.st_gid}', name]
        result = run_sudo(sudo, args, dict(os.environ, LC_ALL='C'), password, deadline, cwd=f'/proc/self/fd/{private}')
        if result:
            return BAD_PASSWORD if result == BAD_PASSWORD else WRITABLE_DIR
        staged = os.fstat(fd)
        if ((staged.st_uid, staged.st_gid, staged.st_mode) !=
                (original.st_uid, original.st_gid, original.st_mode) or file_attrs(fd) != attrs):
            return WRITABLE_DIR
        os.fsync(fd)
        current = os.stat(path.name, dir_fd=parent, follow_symlinks=False)
        if (not os.path.samestat(current, original) or current.st_ctime_ns != original.st_ctime_ns
                or not os.path.samestat(path.parent.stat(), os.fstat(parent))
                or not os.path.samestat(os.stat(name, dir_fd=private, follow_symlinks=False), staged)):
            return CHANGED
        os.replace(name, path.name, src_dir_fd=private, dst_dir_fd=parent)
        return 0


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
        env = dict(os.environ, LC_ALL='C', SUDO_EDITOR=f'{sys.executable} -I {editor}',
                   CC_SUDOEDIT_SOURCE=str(path), CC_SUDOEDIT_DIGEST=digest)
        deadline = time.monotonic() + 45
        status = run_sudo(sudo, ['-e', '--', str(path)], env, password, deadline)
        if (work / 'changed').exists():
            return 3
        if status == WRITABLE_DIR:
            return save_in_writable_directory(sudo, path, digest, data, password, deadline)
        if status or not (work / 'edited').exists():
            return status or DENIED
        with path.open('rb') as stream:
            return 0 if stream.read(LIMIT + 1) == data else 3


if __name__ == '__main__':
    try:
        sys.exit(save(Path(sys.argv[1]), sys.argv[2]))
    except (OSError, subprocess.SubprocessError):
        sys.exit(1)
