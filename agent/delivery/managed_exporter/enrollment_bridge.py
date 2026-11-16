"""Fixed bounded byte relay. It neither parses grants nor admits an operation.

The original pinned SSH stream is carried to the same independently provisioned
Node worker in an unprivileged resource-bounded systemd accepted connection.
"""
import os
from pathlib import Path
import selectors
import socket
import stat
import sys
import time

SOCKET = Path('/run/consolecrypt-agent-managed-control/control.sock')


def need(value):
    if not value:
        raise ValueError('managed exporter control unavailable')


def relay_original(uid):
    need(type(uid) is int and uid > 0 and os.getuid() == os.geteuid() == uid)
    for parent in SOCKET.parents:
        m = parent.lstat()
        need(stat.S_ISDIR(m.st_mode) and m.st_uid == 0 and not m.st_mode & 0o022)
    named = SOCKET.lstat()
    need(stat.S_ISSOCK(named.st_mode) and named.st_uid == uid and stat.S_IMODE(named.st_mode) == 0o600)
    start = time.monotonic()
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as channel:
        channel.settimeout(5)
        channel.connect(str(SOCKET))
        after = SOCKET.lstat()
        need((named.st_dev, named.st_ino, named.st_uid, named.st_mode) ==
             (after.st_dev, after.st_ino, after.st_uid, after.st_mode))
        # PID1 creates/accepts the protected listener. Neither peer identity nor
        # a local socket proves permission: all Node signatures stay mandatory.
        import struct
        _, peer_uid, _ = struct.unpack('3i', channel.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))
        need(peer_uid in (0, uid))
        channel.setblocking(False)
        os.set_blocking(0, False)
        os.set_blocking(1, False)
        incoming = bytearray()
        outgoing = bytearray()
        stdin_open = True
        channel_open = True
        shutdown_sent = False
        end = start + 330
        with selectors.DefaultSelector() as ready:
            while True:
                need(time.monotonic() < end)
                for key in list(ready.get_map().values()):
                    ready.unregister(key.fileobj)
                if stdin_open and len(incoming) < 65536:
                    ready.register(0, selectors.EVENT_READ, 'stdin')
                if outgoing:
                    ready.register(1, selectors.EVENT_WRITE, 'stdout')
                mask = (selectors.EVENT_READ if channel_open and len(outgoing) < 65536 else 0)
                if incoming:
                    mask |= selectors.EVENT_WRITE
                if mask:
                    ready.register(channel, mask, 'channel')
                if not stdin_open and not incoming and not shutdown_sent:
                    channel.shutdown(socket.SHUT_WR)
                    shutdown_sent = True
                if not channel_open and not outgoing:
                    return
                for key, mask in ready.select(min(0.1, max(0, end-time.monotonic()))):
                    if key.data == 'stdin':
                        try:
                            raw = os.read(0, min(16384, 65536-len(incoming)))
                        except BlockingIOError:
                            continue
                        if not raw:
                            stdin_open = False
                            end = min(end, time.monotonic()+5)
                        else:
                            incoming.extend(raw)
                    elif key.data == 'stdout':
                        try:
                            count = os.write(1, outgoing)
                        except BlockingIOError:
                            continue
                        need(count > 0)
                        del outgoing[:count]
                    else:
                        if mask & selectors.EVENT_WRITE and incoming:
                            try:
                                count = channel.send(incoming)
                            except BlockingIOError:
                                count = 0
                            if count:
                                del incoming[:count]
                        if mask & selectors.EVENT_READ and channel_open:
                            try:
                                raw = channel.recv(min(16384, 65536-len(outgoing)))
                            except BlockingIOError:
                                continue
                            if raw:
                                outgoing.extend(raw)
                            else:
                                channel_open = False


def _accepted_members(group, uid):
    # Only descendants in the original child's new session/group. The caller
    # cannot supply a group, UID, command or signal through the wire protocol.
    rows = list(Path('/proc').iterdir())
    need(len(rows) <= 32768)
    members = []
    for path in rows:
        if not path.name.isdecimal():
            continue
        try:
            fields = (path/'stat').read_text().rsplit(') ', 1)[1].split()
            if int(fields[2]) == group:
                need(int(fields[3]) == group and path.stat().st_uid == uid)
                members.append(int(path.name))
                need(len(members) <= 16)
        except (FileNotFoundError, ProcessLookupError):
            pass
    return members


def _accepted_pump(child, end):
    incoming, outgoing = bytearray(), bytearray()
    input_open = output_open = True
    child_input_open = True
    for fd in (0, 1, child.stdin.fileno(), child.stdout.fileno()):
        os.set_blocking(fd, False)
    with selectors.DefaultSelector() as ready:
        while True:
            need(time.monotonic() < end)
            for key in list(ready.get_map().values()):
                ready.unregister(key.fileobj)
            if not input_open and not incoming and child_input_open:
                child.stdin.close()
                child_input_open = False
            if not output_open and not outgoing:
                return
            if input_open and len(incoming) < 65536:
                ready.register(0, selectors.EVENT_READ, 'input')
            if outgoing:
                ready.register(1, selectors.EVENT_WRITE, 'output')
            if output_open and len(outgoing) < 65536:
                ready.register(child.stdout, selectors.EVENT_READ, 'child_output')
            if incoming and child_input_open:
                ready.register(child.stdin, selectors.EVENT_WRITE, 'child_input')
            for key, mask in ready.select(min(.1, max(0, end-time.monotonic()))):
                try:
                    if key.data == 'input':
                        raw = os.read(0, min(16384, 65536-len(incoming)))
                        if raw:
                            incoming.extend(raw)
                        else:
                            input_open = False
                            end = min(end, time.monotonic()+5)
                    elif key.data == 'output':
                        count = os.write(1, outgoing)
                        need(count > 0)
                        del outgoing[:count]
                    elif key.data == 'child_input':
                        count = os.write(child.stdin.fileno(), incoming)
                        need(count > 0)
                        del incoming[:count]
                    else:
                        raw = os.read(child.stdout.fileno(), min(16384, 65536-len(outgoing)))
                        if raw:
                            outgoing.extend(raw)
                        else:
                            output_open = False
                            end = min(end, time.monotonic()+5)
                except BlockingIOError:
                    continue


def _accepted_original(worker):
    """Fixed accepted-socket -> actual same-UID pipes, never an authority.

    Called only with the launcher's already verified normal Node path. The
    accepted stream is not trusted to supply a path, command, UID or policy.
    """
    import ctypes
    import signal
    import subprocess
    end = time.monotonic()+325
    uid = os.getuid()
    need(sys.platform == 'linux' and uid > 0 and os.geteuid() == uid)
    worker = Path(worker)
    need(worker.is_absolute() and worker.name == 'cc-agent-identity')
    for ancestor in (worker, *worker.parents):
        value = ancestor.lstat()
        need(value.st_uid == 0 and not value.st_mode & 0o022 and not stat.S_ISLNK(value.st_mode))
        need(stat.S_ISREG(value.st_mode) if ancestor == worker else stat.S_ISDIR(value.st_mode))
    value = worker.lstat()
    need(value.st_nlink == 1 and stat.S_IMODE(value.st_mode) == 0o755)
    before = [os.fstat(fd) for fd in (0, 1)]
    need(all(stat.S_ISSOCK(value.st_mode) for value in before)
         and (before[0].st_dev, before[0].st_ino) == (before[1].st_dev, before[1].st_ino))
    for parent in SOCKET.parents:
        value = parent.lstat()
        need(stat.S_ISDIR(value.st_mode) and value.st_uid == 0 and not value.st_mode & 0o022)
    with socket.fromfd(0, socket.AF_UNIX, socket.SOCK_STREAM) as accepted:
        need(accepted.family == socket.AF_UNIX and accepted.getsockopt(socket.SOL_SOCKET, socket.SO_TYPE) == socket.SOCK_STREAM
             and accepted.getsockname() == str(SOCKET))
    # Listener filesystem policy and Node signatures govern admission. Do not
    # confuse the allowed SSH frontend UID with this separate service UID.
    need(ctypes.CDLL(None, use_errno=True).prctl(36, 1, 0, 0, 0) == 0)
    child = None
    previous = {sig: signal.getsignal(sig) for sig in (signal.SIGTERM, signal.SIGINT)}
    def cancelled(_signum, _frame):
        raise ValueError('managed exporter control cancelled')
    try:
        for sig in previous:
            signal.signal(sig, cancelled)
        child = subprocess.Popen([str(worker), 'worker-node-enroll-sqlite'],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=None,
            close_fds=True, start_new_session=True, env={'LANG':'C.UTF-8'})
        for stream in (child.stdin, child.stdout):
            actual = os.fstat(stream.fileno())
            need(stat.S_ISFIFO(actual.st_mode) and actual.st_uid == uid
                 and stat.S_IMODE(actual.st_mode) == 0o600)
        leader = (Path('/proc')/str(child.pid)/'stat').read_text().rsplit(') ', 1)[1].split()
        need(int(leader[2]) == int(leader[3]) == child.pid)
        leader_start = int(leader[19])
        _accepted_pump(child, end)  # <=330s including cleanup.
        until = min(end+1, time.monotonic()+1)
        while True:
            status = os.waitid(os.P_PID, child.pid, os.WEXITED|os.WNOHANG|os.WNOWAIT)
            if status is not None:
                return status.si_status if status.si_code == os.CLD_EXITED else -status.si_status
            need(time.monotonic() < until)
            time.sleep(.01)
    finally:
        for sig in previous:
            signal.signal(sig, signal.SIG_IGN)
        try:
            if child is not None:
                for stream in (child.stdin, child.stdout):
                    if stream is not None:
                        stream.close()
                # Hold the original leader zombie with WNOWAIT until every
                # original group member is gone; its PGID cannot be recycled.
                status = os.waitid(os.P_PID, child.pid, os.WEXITED|os.WNOHANG|os.WNOWAIT)
                observed = _accepted_members(child.pid, uid)
                late = status is not None and any(pid != child.pid for pid in observed)
                if status is None or any(pid != child.pid for pid in observed):
                    try:
                        os.killpg(child.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                until = time.monotonic()+1.25
                while True:
                    status = os.waitid(os.P_PID, child.pid, os.WEXITED|os.WNOHANG|os.WNOWAIT)
                    members = _accepted_members(child.pid, uid)
                    for pid in members:
                        if pid != child.pid:
                            try:
                                os.waitpid(pid, os.WNOHANG)
                            except ChildProcessError:
                                pass
                    fields = (Path('/proc')/str(child.pid)/'stat').read_text().rsplit(') ', 1)[1].split()
                    need(int(fields[2]) == int(fields[3]) == child.pid
                         and ('leader_start' not in locals() or int(fields[19]) == leader_start))
                    members = _accepted_members(child.pid, uid)
                    if status is not None and members == [child.pid]:
                        break
                    need(time.monotonic() < until)
                    time.sleep(.01)
                child.wait(timeout=.1)  # Reap only after the original group joined.
                need(child.returncode is not None and not late)
        finally:
            for sig, handler in previous.items():
                signal.signal(sig, handler)
