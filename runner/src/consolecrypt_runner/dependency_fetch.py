"""Single pinned HTTPS artifact, for a future explicitly approved worker.

No environment proxy, redirects, cookies, auth or repository/project code. This
internal module grants no network authority: a caller must first bind the exact
source to a user-reviewed plan and run it under an outer process deadline.
"""
import hashlib
import http.client
import socket
import ssl
import time
from urllib.parse import urlsplit

from .dependencies import dependency_target
from .dependency_archive import MAX_ARCHIVE_BYTES
from .errors import RunnerError, require


def fetch_pinned_archive(record, *, _context=None, timeout=45):
    """Return verified compressed bytes, not an installed dependency.

    _context is only for trusted caller configuration/isolated TLS tests, never
    from a project or RPC payload. Worker default uses the system CA roots.
    The parent must enforce its hard process timeout too: socket timeouts alone
    do not bound a server that slowly drips HTTP headers.
    """
    dependency_target(record)
    source = dict(record['source'])
    require(type(timeout) is int and 1 <= timeout <= 60, 'dependency_fetch_timeout',
            'Invalid dependency fetch timeout')
    url = urlsplit(source['url'])
    require(source['url'].isascii(), 'dependency_source',
            'Dependency source must be an ASCII HTTPS URI')
    context = _context if _context is not None else ssl.create_default_context()
    require(context.verify_mode == ssl.CERT_REQUIRED and context.check_hostname,
            'dependency_fetch_tls', 'Dependency fetch requires verified TLS and hostname')
    deadline = time.monotonic() + timeout
    connection = http.client.HTTPSConnection(url.hostname, url.port or 443,
        timeout=min(5, timeout), context=context)
    try:
        connection.request('GET', url.path or '/', headers={
            'Accept': 'application/octet-stream', 'Accept-Encoding': 'identity',
            'User-Agent': 'ConsoleCrypt-Runner-Dependency/1', 'Connection': 'close'})
        response = connection.getresponse()
        require(response.status == 200, 'dependency_fetch_status',
                'Dependency source must return HTTP 200 without a redirect')
        require(response.getheader('Content-Encoding', 'identity').lower() == 'identity',
                'dependency_fetch_encoding', 'HTTP content encoding is unsupported for pinned archives')
        lengths = response.headers.get_all('Content-Length', [])
        transfers = response.headers.get_all('Transfer-Encoding', [])
        require(len(lengths) <= 1 and len(transfers) <= 1 and not (lengths and transfers),
                'dependency_fetch_framing', 'Ambiguous dependency response framing')
        if transfers:
            require(transfers[0].lower() == 'chunked', 'dependency_fetch_framing',
                    'Unsupported dependency transfer encoding')
        length = None
        if lengths:
            require(lengths[0].isascii() and lengths[0].isdigit() and len(lengths[0]) <= 12,
                    'dependency_fetch_size', 'Invalid dependency response size')
            length = int(lengths[0])
            require(0 < length <= MAX_ARCHIVE_BYTES, 'dependency_fetch_size',
                    'Dependency response exceeds its compressed byte budget')
        body = bytearray()
        while True:
            remaining = deadline - time.monotonic()
            require(remaining > 0, 'dependency_fetch_timeout', 'Dependency download exceeded its time budget')
            if connection.sock is not None:
                connection.sock.settimeout(min(5, remaining))
            chunk = response.read(min(128 * 1024, MAX_ARCHIVE_BYTES - len(body) + 1))
            if not chunk:
                break
            body.extend(chunk)
            require(len(body) <= MAX_ARCHIVE_BYTES, 'dependency_fetch_size',
                    'Dependency response exceeds its compressed byte budget')
        require(length is None or len(body) == length, 'dependency_fetch_size',
                'Dependency response ended before its declared size')
        require(body and hashlib.sha256(body).hexdigest() == source['sha256'],
                'dependency_archive_digest', 'Downloaded dependency differs from its pinned SHA-256')
        return bytes(body)
    except (OSError, socket.timeout, http.client.HTTPException, ValueError) as error:
        raise RunnerError('dependency_fetch_failed', 'Cannot download the pinned dependency over verified HTTPS') from error
    finally:
        connection.close()
