"""Trusted single-artifact reader. stdin is /dev/null; stdout is archive data.

Never imports, extracts or invokes downloaded project code. The trusted parent
mounts this bounded input outside /project and independently checks all bytes.
"""
from pathlib import Path
import ssl
import sys

from .dependency_fetch import fetch_pinned_archive
from .protocol import decode


def main():
    try:
        spec = decode(Path('/tool/dependency-input.json').read_bytes(), max_bytes=8192)
        context = ssl.create_default_context(cafile='/tool/dependency-ca.pem') if spec['custom_ca'] else None
        raw = fetch_pinned_archive(spec['record'], _context=context, timeout=45)
        sys.stdout.buffer.write(raw)
        sys.stdout.buffer.flush()
    except Exception:
        # Neither remote bodies nor request-derived exception text enters logs.
        raise SystemExit(65)


if __name__ == '__main__':
    main()
