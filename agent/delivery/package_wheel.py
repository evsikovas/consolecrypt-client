"""Select one pinned pure-Python wheel for the verified release's import path.

No pip, installation into ambient Python, imports or network calls. The launcher
verifies the packaged file before enabling zipimport from this release only.
"""
import hashlib
import io
from pathlib import PurePosixPath
import stat
import zipfile

WHEEL_NAME = 'websockets-17.4-py3-none-any.whl'
SOURCE = 'third_party/websockets/' + WHEEL_NAME
PACKAGE = 'python/site-packages/' + WHEEL_NAME
SHA256 = 'c6bd2c9781915592b209160d0f017d4c7c82ad53075e6b0c24d2a89299e7f9e9'


def wheel_file(agent_root):
    path = agent_root / SOURCE
    info = path.lstat()
    if (not stat.S_ISREG(info.st_mode) or getattr(info, 'st_file_attributes', 0) & 0x400
            or info.st_size != 212900):
        raise ValueError('pinned regular WSS wheel required')
    content = path.read_bytes()
    if hashlib.sha256(content).hexdigest() != SHA256:
        raise ValueError('WSS wheel changed')
    with zipfile.ZipFile(io.BytesIO(content)) as archive:
        entries = archive.infolist()
        if (len(entries) > 128 or len({i.filename for i in entries}) != len(entries)
                or sum(i.file_size for i in entries) > 2 * 1024**2):
            raise ValueError('WSS wheel bounds')
        for item in entries:
            name = PurePosixPath(item.filename)
            if (name.is_absolute() or '..' in name.parts or str(name) != item.filename
                    or '\\' in item.filename or ':' in item.filename
                    or name.suffix.lower() in {'.so', '.pyd', '.dll', '.exe'}
                    or stat.S_ISLNK(item.external_attr >> 16)):
                raise ValueError('pure Python WSS wheel required')
    return {PACKAGE: content}
