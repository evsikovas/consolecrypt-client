"""One bounded socket check for the selected SSH host."""
import errno
import signal
import socket
import sys
import time


def check(host, port, udp):
    stage = "dns_error"

    def expired(*_):
        raise TimeoutError()

    signal.signal(signal.SIGALRM, expired)
    signal.alarm(5)
    deadline = time.monotonic() + 5
    try:
        kind = socket.SOCK_DGRAM if udp else socket.SOCK_STREAM
        addresses = socket.getaddrinfo(host, port, type=kind)
        if not addresses:
            return stage
        stage = "timeout"
        for i, (family, socktype, proto, _, address) in enumerate(addresses):
            try:
                with socket.socket(family, socktype, proto) as sock:
                    remaining = max(0.001, deadline - time.monotonic())
                    sock.settimeout(remaining if udp else remaining / (len(addresses) - i))
                    sock.connect(address)
                    if not udp:
                        return "connected"
                    sock.send(b"\0")
                    stage = "no_reply"
                    sock.recv(1)
                    return "reply"
            except OSError as error:
                if isinstance(error, (socket.timeout, TimeoutError)):
                    result = stage
                elif error.errno in (errno.ECONNREFUSED, errno.ECONNRESET):
                    result = "refused"
                elif error.errno in (errno.ENETUNREACH, errno.EHOSTUNREACH):
                    result = "unreachable"
                else:
                    result = "socket_error"
                if udp or i + 1 == len(addresses) or time.monotonic() >= deadline:
                    return result
    except (socket.gaierror, TimeoutError):
        return stage
    finally:
        signal.alarm(0)


if __name__ == "__main__":
    print(check(sys.argv[1], int(sys.argv[2]), sys.argv[3] == "udp"))
