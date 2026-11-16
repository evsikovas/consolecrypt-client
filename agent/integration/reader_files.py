"""Bounded regular-file reads for trusted reader config/verifier paths only."""
import hashlib
import os
import stat


def read_reader_file(path, limit, *, digest=False):
    flags = (os.O_RDONLY | getattr(os, "O_NONBLOCK", 0) | getattr(os, "O_NOFOLLOW", 0)
             | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_BINARY", 0))
    fd = os.open(path, flags)
    try:
        before = os.fstat(fd)
        if not stat.S_ISREG(before.st_mode) or not 0 < before.st_size <= limit:
            raise ValueError("invalid_reader_file")
        left, chunks, hasher = before.st_size, [], hashlib.sha256()
        while left:
            chunk = os.read(fd, min(left, 65536))
            if not chunk:
                raise ValueError("reader_file_changed")
            left -= len(chunk)
            if digest:
                hasher.update(chunk)
            else:
                chunks.append(chunk)
        if os.read(fd, 1):
            raise ValueError("reader_file_changed")
        after = os.fstat(fd)
        def identity(value):
            return (value.st_dev, value.st_ino, value.st_mode, value.st_size,
                    value.st_mtime_ns, value.st_ctime_ns)
        if identity(before) != identity(after):
            raise ValueError("reader_file_changed")
        return hasher.hexdigest() if digest else b"".join(chunks)
    finally:
        os.close(fd)
