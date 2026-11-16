"""Fixed no-payload namespace prerequisite, never a MetricsCollect authority."""
import json
import os
from pathlib import Path
import stat
import sys


def require(ok):
    if not ok:
        raise ValueError("collector_namespace_unsupported")


def main():
    require(len(sys.argv) == 3 and all(x.isdecimal() for x in sys.argv[1:]))
    uid, gid = map(int, sys.argv[1:])
    require(uid > 0 and gid > 0 and os.getuid() == uid and os.getgid() == gid)
    for name, expected in (("uid_map", uid), ("gid_map", gid)):
        rows = [line.split() for line in Path("/proc/self", name).read_text().splitlines()]
        require(rows == [[str(expected), str(expected), "1"]])
    root = Path("/run/consolecrypt-agent/volatile")
    st = root.lstat()
    require(stat.S_ISDIR(st.st_mode) and st.st_uid == uid and stat.S_IMODE(st.st_mode) == 0o700)
    rows, roots = [], []
    raw = Path("/proc/self/mountinfo").read_bytes()
    require(len(raw) <= 262144)
    for line in raw.decode("ascii").splitlines():
        first, last = line.split(" - ", 1)
        a, b = first.split(), last.split()
        if a[4] == str(root):
            rows.append((a, b))
        if a[4] == "/":
            roots.append(a)
    require(len(rows) == 1 and len(roots) == 1 and "ro" in roots[0][5].split(","))
    a, b = rows[0]
    require(b[0] == "tmpfs" and {"rw", "nosuid", "nodev", "noexec", "noswap"}
            <= set(a[5].split(",")) | set(b[2].split(",")))
    fs = os.statvfs(root)
    require(fs.f_blocks * fs.f_frsize == 33554432 and 0 < fs.f_files <= 4096)
    print("CC_COLLECTOR_NAMESPACE_OK", flush=True)


if __name__ == "__main__":
    try:
        main()
    except BaseException:
        # No path, environment or arbitrary exception text is exposed.
        print("collector_namespace_unsupported", file=sys.stderr)
        raise SystemExit(1)
