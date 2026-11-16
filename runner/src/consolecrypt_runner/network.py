"""One explicitly requested TCP endpoint through private stdin/stdout, no keys/listeners."""
import os
import select
import socket
import time

from .errors import require


def bridge_stdio(host, port):
    require(os.name == 'posix' and os.getuid() != 0, 'network_user', 'Use a non-root network origin')
    require(isinstance(host, str) and 0 < len(host) <= 253 and not host.startswith('-') and
            not any(c.isspace() or ord(c) < 32 or c in '/@\x00' for c in host) and
            type(port) is int and 1 <= port <= 65535, 'network_endpoint', 'Invalid origin endpoint')
    with socket.create_connection((host, port), timeout=15) as stream:
        stream.setblocking(False)
        os.set_blocking(0, False); os.set_blocking(1, False)
        upstream, downstream = bytearray(), bytearray()
        input_open = remote_open = write_open = True
        closing = None
        while True:
            if not remote_open and not downstream:
                return
            if closing is not None and time.monotonic() >= closing:
                return
            if not input_open and not upstream and write_open:
                try: stream.shutdown(socket.SHUT_WR)
                except OSError: pass
                write_open = False
            readers = ([0] if input_open and len(upstream) < 128 * 1024 else []) + ([stream] if remote_open and len(downstream) < 128 * 1024 else [])
            writers = ([stream] if upstream else []) + ([1] if downstream else [])
            ready_read, ready_write, _ = select.select(readers, writers, [], .1)
            for source in ready_read:
                try:
                    chunk = os.read(0, 65536) if source == 0 else stream.recv(65536)
                except BlockingIOError:
                    continue
                if source == 0:
                    if chunk: upstream.extend(chunk)
                    else: input_open = False; closing = time.monotonic() + 2
                else:
                    if chunk: downstream.extend(chunk)
                    else: remote_open = False
            for destination in ready_write:
                try:
                    if destination == 1:
                        written = os.write(1, downstream)
                        del downstream[:written]
                    else:
                        written = stream.send(upstream)
                        del upstream[:written]
                except BlockingIOError:
                    continue
