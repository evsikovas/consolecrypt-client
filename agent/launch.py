"""Stable read-only launcher for verified staged packages. No shell or symlinks."""

from contextlib import contextmanager
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import runpy
import stat
import sys


def reject_links(path):
    for parent in (path, *path.parents):
        info = parent.lstat()
        if stat.S_ISLNK(info.st_mode) or getattr(info, "st_file_attributes", 0) & 0x400:
            raise ValueError("linked release path refused")


COLLECTOR_FILES = ("collector/unit_spec.py", "collector/volatile_unit_adapter.py", "collector/volatile_unit_entry.py", "collector/vmagent-prod")
VMAGENT_SHA256 = "2039bc05b6ad5c96b34b72801908896bf8a6a7bcab8ae32120cbb8e6217ef6f6"


@contextmanager
def _collector_file(path, mode, cap):
    # Keep the SAME nofollow FD and original custody/identity checks through
    # every read, including streaming digest/comparison of a large ELF.
    reject_links(path)
    for parent in path.parents:
        info = parent.lstat()
        if not stat.S_ISDIR(info.st_mode) or info.st_uid != 0 or info.st_mode & 0o022:
            raise ValueError("collector parent custody changed")
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC)
    try:
        before = os.fstat(fd)
        if (not stat.S_ISREG(before.st_mode) or before.st_uid != 0 or before.st_nlink != 1
                or stat.S_IMODE(before.st_mode) != mode or not 0 < before.st_size <= cap):
            raise ValueError("collector file custody changed")
        yield fd, before
        after = os.fstat(fd); named = path.lstat()
        identity = lambda m: (m.st_dev, m.st_ino, m.st_mode, m.st_uid, m.st_nlink, m.st_size, m.st_mtime_ns, m.st_ctime_ns)
        if identity(before) != identity(after) or identity(before) != identity(named):
            raise ValueError("collector file changed during read")
    finally:
        os.close(fd)


def _collector_bytes(path, mode, cap):
    with _collector_file(path, mode, cap) as (fd, before):
        blocks = []; left = cap + 1
        while left:
            block = os.read(fd, min(left, 1048576))
            if not block: break
            blocks.append(block); left -= len(block)
        raw = b"".join(blocks)
        if len(raw) != before.st_size:
            raise ValueError("collector file changed during read")
        return raw


def _collector_fingerprint(path, mode, cap):
    with _collector_file(path, mode, cap) as (fd, before):
        digest = hashlib.sha256(); total = 0; header = b""
        while total <= cap:
            block = os.read(fd, min(1048576, cap + 1 - total))
            if not block: break
            if len(header) < 64: header += block[:64-len(header)]
            digest.update(block); total += len(block)
        if total != before.st_size:
            raise ValueError("collector file changed during read")
        return total, digest.hexdigest(), header


def _collector_equal(packaged, stable, mode, cap):
    def block(fd, limit):
        chunks = []; total = 0
        while total < limit:
            part = os.read(fd, limit - total)
            if not part: break
            chunks.append(part); total += len(part)
        return b"".join(chunks)
    with _collector_file(packaged, mode, cap) as (first, a):
        with _collector_file(stable, mode, cap) as (second, b):
            if a.st_size != b.st_size: raise ValueError("stable collector bytes changed")
            digest = hashlib.sha256(); total = 0; header = b""
            while total <= cap:
                left = block(first, min(1048576, cap + 1 - total))
                right = block(second, min(1048576, cap + 1 - total))
                if left != right: raise ValueError("stable collector bytes changed")
                if not left: break
                if len(header) < 64: header += left[:64-len(header)]
                digest.update(left); total += len(left)
            if total != a.st_size: raise ValueError("collector file changed during read")
            return total, digest.hexdigest(), header


def _stable_collector(code, release, record):
    if os.name != "posix": raise ValueError("installed collector requires Linux")
    if not set(COLLECTOR_FILES) <= record["files"].keys():
        raise ValueError("complete signed collector runtime required")
    platforms,row=_selected_platform(code,release,record) if record.get("format")=="cc-agent-installed-release/v2" else (None,None)
    for name in COLLECTOR_FILES:
        executable = name == "collector/vmagent-prod"
        mode = 0o755 if executable else 0o644
        cap = 23209584 if executable else 65536
        if record["file_modes"].get(name) != ("0755" if executable else "0644"):
            raise ValueError("collector signed mode changed")
        size, digest, header = _collector_equal(release / name, code / name, mode, cap)
        if digest != record["files"][name]:
            raise ValueError("stable collector differs from selected signed release")
        if executable:
            if platforms is not None:
                platforms.require_elf(header,row)
                if (not 64 <= size <= platforms.MAX_COLLECTOR
                        or row['observedBytes'] is not None and size != row['observedBytes']
                        or digest != row['binarySha256']):
                    raise ValueError("original pinned collector bytes unavailable")
            elif (size!=23209584 or digest!=VMAGENT_SHA256
                    or header[:6]!=b"\x7fELF\x02\x01" or int.from_bytes(header[18:20],"little")!=62):
                raise ValueError("unsupported installed collector ELF")



def verified_identity_binary():
    """Fixed root-custody selected package verifier; no execution or caller path."""
    code = Path(__file__).absolute().parent
    if os.name != "posix" or code != Path("/opt/consolecrypt-agent-install/opt/consolecrypt-agent"):
        raise ValueError("fixed installed enrollment prefix required")
    current = json.loads(_collector_bytes(code / "current.json", 0o644, 4096))
    version = current.get("version")
    if type(version) is not str or not re.fullmatch(r"[0-9]{1,6}\.[0-9]{1,6}\.[0-9]{1,6}", version):
        raise ValueError("fixed installed version required")
    release = code / "releases" / version
    record = json.loads(_collector_bytes(release / "installed.json", 0o644, 128*1024))
    if (record.get("version") != version or record.get("artifact_sha256") != current.get("artifact_sha256")
            or type(record.get("files")) is not dict or not 1 <= len(record["files"]) <= 513
            or type(record.get("file_modes")) is not dict or set(record["file_modes"]) != set(record["files"])):
        raise ValueError("installed enrollment record mismatch")
    anchor = code.parents[1] / "var/lib/consolecrypt-agent/release-anchor.json"
    floor = json.loads(_collector_bytes(anchor, 0o644, 4096))
    if type(floor.get("version")) is not int or floor["version"] < 1 or floor.get("protocol") != 1:
        raise ValueError("installed enrollment anchor invalid")
    total = 0
    for name, expected in record["files"].items():
        relative = PurePosixPath(name)
        if relative.is_absolute() or ".." in relative.parts or "\\" in name or ":" in name:
            raise ValueError("installed enrollment traversal")
        mode = record["file_modes"][name]
        if mode not in ("0644", "0755"):
            raise ValueError("installed enrollment mode")
        size, digest, _ = _collector_fingerprint(release.joinpath(*relative.parts), int(mode, 8), 192 * 1024 * 1024 if name=="collector/prometheus-hardmode" else 64 * 1024 * 1024)
        total += size
        if total > 256 * 1024 * 1024 + 128*1024 or digest != expected:
            raise ValueError("installed enrollment content mismatch")
    if (record["files"].get("launch.py") != hashlib.sha256(_collector_bytes(code / "launch.py", 0o644, 131072)).hexdigest()
            or record["file_modes"].get("bin/cc-agent-identity") != "0755"):
        raise ValueError("installed enrollment dispatch binding")
    binary = release / "bin/cc-agent-identity"
    _, _, raw = _collector_fingerprint(binary, 0o755, 32 * 1024 * 1024)
    if record.get('format')=='cc-agent-installed-release/v2':
        platforms,row=_selected_platform(code,release,record)
        platforms.require_elf(raw,row)
    elif raw[:6]!=b"\x7fELF\x02\x01" or int.from_bytes(raw[18:20],"little")!=62:
        raise ValueError("normal Linux enrollment ELF required")
    return binary


def _selected_platform(code, release, record):
    if record.get('format')!='cc-agent-installed-release/v2':
        raise ValueError('architecture-bound installed release required')
    current=json.loads(_collector_bytes(code/'current.json',0o644,4096))
    if release.parent!=code/'releases' or release.name!=record.get('version'):
        raise ValueError('selected release namespace changed')
    name='delivery/release_platforms.py'
    raw=_collector_bytes(release/name,0o644,65536)
    if record['file_modes'].get(name)!='0644' or hashlib.sha256(raw).hexdigest()!=record['files'].get(name):
        raise ValueError('selected signed platform helper changed')
    # Only exact root-owned bytes from this selected signed release, never an
    # ambient module, caller path or caller architecture selection.
    import types
    module=types.ModuleType('_cc_selected_release_platforms')
    exec(compile(raw,str(release/name),'exec'),module.__dict__)
    return module,module.installed_platform(current,record)


DISK_FILES = ("collector/disk_binding.py", "collector/disk_unit_spec.py",
              "collector/disk_unit_adapter.py", "collector/disk_unit_entry.py", "collector/prometheus-hardmode")


def _stable_disk(code, release, record):
    # Sources may accompany RAM-only releases; only the exact signed disk ELF
    # selects the additional fixed closure. Catalog hashes cannot substitute
    # for correspondence to the currently selected signed package.
    if DISK_FILES[-1] not in record["files"]:
        return
    if not set(DISK_FILES) <= record["files"].keys():
        raise ValueError("complete signed disk collector runtime required")
    platforms,row=_selected_platform(code,release,record) if record.get('format')=='cc-agent-installed-release/v2' else (None,None)
    for name in DISK_FILES:
        executable = name == DISK_FILES[-1]
        mode = 0o755 if executable else 0o644
        cap = 192 * 1024 * 1024 if executable else 65536
        if record["file_modes"].get(name) != ("0755" if executable else "0644"):
            raise ValueError("disk collector signed mode changed")
        _, digest, header = _collector_equal(release / name, code / name, mode, cap)
        if digest != record["files"][name]:
            raise ValueError("stable disk collector differs from selected signed release")
        if executable:
            if platforms is not None:
                platforms.require_elf(header,row)
            elif (len(header) < 64 or header[:6] != b"\x7fELF\x02\x01"
                    or int.from_bytes(header[18:20], "little") != 62):
                raise ValueError("unsupported installed disk collector ELF")


SERVICE_FILES = ("collector/service_binding.py", "collector/service_unit_spec.py",
                 "collector/service_unit_adapter.py", "collector/service_unit_entry.py", "collector/service_daemon.py")


def run_collector_service():
    worker = verified_identity_binary()
    code = Path(__file__).absolute().parent
    release = worker.parent.parent
    record = json.loads(_collector_bytes(release / "installed.json", 0o644, 128*1024))
    # Stable Python imports must correspond to the SAME selected signed release.
    # RAM/disk verification retains exact custody under the measured package caps.
    _stable_disk(code, release, record)
    if DISK_FILES[-1] not in record["files"] or not set(SERVICE_FILES) <= record["files"].keys():
        raise ValueError("complete signed finite-service disk runtime required")
    namespace = None
    for name in SERVICE_FILES:
        if record["file_modes"].get(name) != "0644":
            raise ValueError("service signed mode changed")
        packaged = _collector_bytes(release / name, 0o644, 65536)
        stable = _collector_bytes(code / name, 0o644, 65536)
        if packaged != stable or hashlib.sha256(packaged).hexdigest() != record["files"][name]:
            raise ValueError("stable service differs from selected signed release")
        if name == "collector/service_daemon.py":
            namespace = {"__name__": "cc_verified_service_daemon", "__file__": str(code / name)}
            exec(compile(stable, str(code / name), "exec"), namespace)
    return namespace["run_verified"](worker)


def _managed_enrollment_worker(worker):
    # SAME administrator-installed public fingerprint as enrollment-worker's
    # frontend. It grants no permission; all Node signatures stay mandatory.
    path = Path('/etc/consolecrypt-agent/managed-exporter-runtime.json')
    def unique(rows):
        value = {}
        for key, item in rows:
            if key in value:
                raise ValueError('duplicate managed runtime field')
            value[key] = item
        return value
    config = json.loads(_collector_bytes(path, 0o644, 4096), object_pairs_hook=unique)
    if (type(config) is not dict or set(config) != {'format','serviceUid','serviceGid','bridgeSha256'}
            or config['format'] != 'cc-agent-managed-exporter-control/v1'
            or type(config['serviceUid']) is not int or config['serviceUid'] <= 0
            or config['serviceUid'] != os.getuid() or os.geteuid() != os.getuid()
            or type(config['serviceGid']) is not int or config['serviceGid'] != os.getgid()
            or os.getegid() != os.getgid()
            or type(config['bridgeSha256']) is not str
            or re.fullmatch('[0-9a-f]{64}', config['bridgeSha256']) is None):
        raise ValueError('managed runtime identity changed')
    bridge = Path('/usr/local/libexec/consolecrypt-agent-managed-exporter/enrollment_bridge.py')
    raw = _collector_bytes(bridge, 0o644, 65536)
    if hashlib.sha256(raw).hexdigest() != config['bridgeSha256']:
        raise ValueError('managed runtime bridge changed')
    namespace = {'__name__':'verified_managed_enrollment_bridge','__file__':str(bridge)}
    exec(compile(raw, str(bridge), 'exec'), namespace)
    if _collector_bytes(bridge, 0o644, 65536) != raw:
        raise ValueError('managed runtime bridge changed')
    return namespace['_accepted_original'](worker)


def main():
    if "--collector-service" in sys.argv[1:]:
        if sys.argv[1:] != ["--collector-service"]:
            raise ValueError("fixed service accepts no arguments")
        raise SystemExit(run_collector_service())
    if "--enrollment-worker" in sys.argv[1:]:
        if sys.argv[1:] != ["--enrollment-worker"]:
            raise ValueError("fixed enrollment worker accepts no arguments")
        worker = verified_identity_binary()
        if any(stat.S_ISSOCK(os.fstat(fd).st_mode) for fd in (0, 1)):
            raise SystemExit(_managed_enrollment_worker(worker))
        os.execve(str(worker), [str(worker), "worker-node-enroll-sqlite"], {"LANG": "C.UTF-8"})
        raise RuntimeError("fixed enrollment exec unexpectedly returned")
    code = Path(__file__).absolute().parent
    reject_links(code)
    pointer = code / "current.json"
    reject_links(pointer)
    if pointer.stat().st_size > 4096:
        raise ValueError("oversize current pointer")
    current = json.loads(pointer.read_bytes())
    version = current["version"]
    if not isinstance(version, str) or not re.fullmatch(r"[0-9]{1,6}\.[0-9]{1,6}\.[0-9]{1,6}", version):
        raise ValueError("invalid current version")
    release = code / "releases" / version
    record_path = release / "installed.json"
    reject_links(record_path)
    if record_path.stat().st_size > 128*1024:
        raise ValueError("oversize release record")
    record = json.loads(record_path.read_bytes())
    if record["artifact_sha256"] != current["artifact_sha256"] or record["version"] != version:
        raise ValueError("release pointer mismatch")
    if len(record["files"]) > 513:
        raise ValueError("file count budget")
    required = {"integration/__init__.py", "integration/__main__.py", "integration/identity_gate.py",
                "integration/profile_manifest.py", "integration/profile_runtime.py", "integration/collector_status.py",
                "integration/device_reader_config.py", "integration/device_reader_gate.py",
                "integration/reader_channel.py", "integration/reader_files.py",
                "integration/sqlite_worker_gate.py", "integration/sqlite_custody_gate.py",
                "integration/tunnel_config.py", "integration/tunnel_supervised_child.py",
                "runtime/cc_agent/__init__.py", "runtime/cc_agent/__main__.py", "runtime/cc_agent/manifest.py",
                "runtime/cc_agent/history.py", "runtime/cc_agent/service.py", "runtime/cc_agent/audit.py",
                "runtime/cc_agent/history_reader.py", "runtime/cc_agent/http_ingress.py",
                "runtime/cc_agent/read_expiry.py", "runtime/cc_agent/read_access.py", "runtime/cc_agent/device_reader.py",
                "runtime/cc_agent/tunnel.py", "runtime/cc_agent/tunnel_audit.py", "runtime/cc_agent/wss_carrier.py",
                "runtime/cc_agent/tunnel_policy_v2.py", "runtime/cc_agent/tunnel_supervised.py",
                "collector/__init__.py", "collector/manager.py", "collector/supply.py", "collector/native_status.py"}
    if not required <= record["files"].keys():
        raise ValueError("complete signed Agent entrypoint required")
    anchor = code.parents[1] / "var/lib/consolecrypt-agent/release-anchor.json"
    reject_links(anchor)
    if anchor.stat().st_size > 4096:
        raise ValueError("release anchor oversized")
    floor = json.loads(anchor.read_bytes())
    if type(floor.get("version")) is not int or floor["version"] < 1 or floor.get("protocol") != 1:
        raise ValueError("release anchor invalid")
    total = 0
    for name, expected in record["files"].items():
        relative = PurePosixPath(name)
        if relative.is_absolute() or ".." in relative.parts or "\\" in name or ":" in name:
            raise ValueError("release traversal")
        path = release.joinpath(*relative.parts)
        reject_links(path)
        size = path.stat().st_size
        total += size
        if size > (192 * 1024 * 1024 if name=="collector/prometheus-hardmode" else 64 * 1024 * 1024) or total > 256 * 1024 * 1024 + 128*1024:
            raise ValueError("release byte budget")
        if hashlib.sha256(path.read_bytes()).hexdigest() != expected:
            raise ValueError("release file changed")
        mode = record["file_modes"][name]
        if mode not in ("0644", "0755") or os.name != "nt" and stat.S_IMODE(path.stat().st_mode) != int(mode, 8):
            raise ValueError("release file mode changed")
    _stable_collector(code, release, record)
    _stable_disk(code, release, record)
    # Paths are root-owned in real delivery; the service account cannot rewrite
    # package code, current.json or installed.json. Local staging has no such claim.
    wheel = "python/site-packages/websockets-17.4-py3-none-any.whl"
    if record["files"].get(wheel) != "c6bd2c9781915592b209160d0f017d4c7c82ad53075e6b0c24d2a89299e7f9e9":
        raise ValueError("pinned package-owned WSS wheel required")
    sys.path.insert(0, str(release / wheel))
    sys.path.insert(0, str(release))
    if sys.argv[1:] == ["outbound-control"]:
        outbound = {"integration/outbound_control.py", "runtime/cc_agent/outbound_control.py", "runtime/cc_agent/outbound_control_daemon.py"}
        if not outbound <= record["files"].keys():
            raise ValueError("complete signed outbound-control runtime required")
        runpy.run_module("integration.outbound_control", run_name="__main__")
        return
    runpy.run_module("integration", run_name="__main__")


if __name__ == "__main__":
    main()
