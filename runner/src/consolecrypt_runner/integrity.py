"""Content fingerprint, not just the reported versions, of the execution tools."""
from __future__ import annotations

import hashlib
from pathlib import Path
import re

from .errors import require
from .protocol import encode


def dependency_fingerprint(distributions, prefix: Path):
    prefix = prefix.resolve(strict=True)
    entries, total = [], 0
    for distribution in distributions:
        name = re.sub(r'[-_.]+', '-', distribution.metadata['Name']).lower()
        if name == 'consolecrypt-runner':
            continue  # The helper's own sources have a separate fingerprint.
        require(distribution.files is not None, 'environment_integrity', 'Dependency has no file manifest')
        for relative in sorted(distribution.files, key=str):
            if str(relative).endswith(('.pyc', '.pyo')):
                continue
            path = Path(distribution.locate_file(relative))
            resolved = path.resolve(strict=True)
            require(resolved.is_relative_to(prefix) and not path.is_symlink() and resolved.is_file(),
                    'environment_integrity', 'Dependency file escapes the private environment')
            before = resolved.stat()
            total += before.st_size
            require(total <= 512 * 1024 * 1024 and len(entries) < 100000,
                    'environment_integrity', 'Dependency file budget exceeded')
            digest = hashlib.sha256()
            with resolved.open('rb') as stream:
                while chunk := stream.read(1024 * 1024):
                    digest.update(chunk)
            after = resolved.stat()
            require((before.st_size, before.st_mtime_ns, before.st_ctime_ns) ==
                    (after.st_size, after.st_mtime_ns, after.st_ctime_ns),
                    'environment_integrity', 'Dependency changed during verification')
            entries.append({'distribution': name, 'path': resolved.relative_to(prefix).as_posix(),
                            'sha256': digest.hexdigest(), 'mode': after.st_mode & 0o777})
    raw = encode(sorted(entries, key=lambda item: (item['distribution'], item['path'])))
    require(len(raw) <= 32 * 1024 * 1024, 'environment_integrity', 'Dependency manifest budget exceeded')
    return hashlib.sha256(raw).hexdigest()
