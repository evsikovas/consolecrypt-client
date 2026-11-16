"""One job-only Unix socket. It forwards data to the native scope gate, never SSH keys."""
from pathlib import Path
import os
import socket
import socketserver
import struct
import threading
import sys
import time

from .errors import RunnerError, require
from .protocol import MAX_FRAME_BYTES, decode, write_frame


class _PrivateSocketBroker:
    def __init__(self, directory: Path, handle_request, close_scope, cancel, *, slots_limit=5, error_message="Scoped job operation failed", frame_limit=MAX_FRAME_BYTES, handoff_seconds=0, keep_owned_directory=False):
        require(not directory.exists(), 'broker_directory', 'Broker directory must be new')
        directory.mkdir(mode=0o700)
        self.directory = directory
        self._keep_owned_directory=keep_owned_directory
        self.cancel, self._close_scope = cancel, close_scope
        owner_uid = os.getuid()
        slots = threading.BoundedSemaphore(slots_limit)
        broker = self

        class Handler(socketserver.StreamRequestHandler):
            def handle(self):
                try:
                    if hasattr(socket, 'SO_PEERCRED'):
                        _, uid, _ = struct.unpack('3i', self.request.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))
                        require(uid == owner_uid, 'broker_peer', 'Invalid job socket peer')
                    self.request.settimeout(5)
                    raw = self.rfile.readline(frame_limit + 1)
                    require(len(raw) <= frame_limit and raw.endswith(b'\n'), 'broker_frame', 'Invalid broker frame')
                    request = decode(raw)
                    result = handle_request(request)
                    write_frame(self.wfile, {'version': 1, 'result': result, 'error': None})
                except RunnerError as error:
                    write_frame(self.wfile, {'version': 1, 'result': None, 'error': {'code': error.code, 'message': error_message}})
                except (OSError, ValueError):
                    pass

        class Server(socketserver.ThreadingMixIn, socketserver.UnixStreamServer):
            daemon_threads = False
            block_on_close = True
            request_queue_size = slots_limit

            def process_request(self, request, address):
                acquired=slots.acquire(blocking=False)
                if not acquired and handoff_seconds:
                    # At most ONE accept-loop socket waits; no extra handler,
                    # unbounded queue or Native lookup while prior response is
                    # flushed but its thread is still releasing the slot.
                    deadline=time.monotonic()+min(handoff_seconds,5)
                    while not broker.cancel.is_set() and time.monotonic()<deadline:
                        if slots.acquire(timeout=min(.05,max(0,deadline-time.monotonic()))):
                            acquired=True;break
                if acquired:
                    try:
                        super().process_request(request, address)
                    except BaseException:
                        slots.release()
                        raise
                else:
                    self.shutdown_request(request)

            def process_request_thread(self, request, address):
                try:
                    super().process_request_thread(request, address)
                finally:
                    slots.release()

            def handle_error(self, request, address):
                # Never dump guest payloads or unfiltered exception text.
                pass

        if sys.platform == 'linux':
            # AF_UNIX addresses have a ~108-byte limit even on filesystems
            # supporting much longer private job paths. Bind through our own
            # directory FD; the inode is still in the job directory and the
            # guest reaches it through its short read-only /run bind mount.
            fd = os.open(directory, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
            try:
                self.server = Server('/proc/self/fd/' + str(fd) + '/broker.sock', Handler)
            finally:
                os.close(fd)
        else:
            self.server = Server(str(directory / 'broker.sock'), Handler)
        os.chmod(directory / 'broker.sock', 0o600)
        self.thread = threading.Thread(target=self.server.serve_forever, kwargs={'poll_interval': .1}, daemon=False)
        self.thread.start()

    def close(self):
        self.cancel.set()
        self._close_scope()
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=5)
        if not self._keep_owned_directory:
            (self.directory / 'broker.sock').unlink(missing_ok=True)
            self.directory.rmdir()

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()


class JobBroker(_PrivateSocketBroker):
    """Existing signed-job API and exact target-channel semantics unchanged."""
    def __init__(self, directory: Path, verified, channel, cancel):
        self.verified, self.channel = verified, channel
        def handle(request):
            require(isinstance(request, dict) and set(request) == {'version', 'host_id', 'operation', 'body'} and
                    type(request['version']) is int and request['version'] == 1,
                    'broker_request', 'Invalid scoped broker request')
            del request['version']
            return channel.call(verified, **request, cancel=cancel,
                                timeout=verified.value['policy']['timeout_seconds'])
        super().__init__(directory, handle, lambda: channel.revoke(verified), cancel)
