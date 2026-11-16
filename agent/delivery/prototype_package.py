"""Explicit reproducible Agent ZIP inputs. No examples, node state, caches or credentials."""

import argparse
import ast
import hashlib
import json
import os
import stat
import re
from pathlib import Path
import sys
import tempfile
import zlib
import release_platforms

from notice_inventory import INVENTORY, validate_notice_inventory
from stage import build_package
from package_wheel import wheel_file, PACKAGE as WSS_PACKAGE, SHA256 as WSS_SHA256

DIRECTORY = Path(__file__).resolve().parent
AGENT = DIRECTORY.parent
SOURCES = (
    "integration/__init__.py", "integration/__main__.py", "integration/identity_gate.py",
    "integration/profile_manifest.py", "integration/profile_runtime.py",
    "integration/collector_status.py", "integration/device_reader_config.py",
    "integration/device_reader_gate.py", "integration/reader_channel.py", "integration/reader_files.py",
    "integration/sqlite_worker_gate.py", "integration/sqlite_custody_gate.py",
    "integration/tunnel_config.py", "integration/tunnel_supervised_child.py", "integration/outbound_control.py",
    "runtime/cc_agent/__init__.py", "runtime/cc_agent/__main__.py", "runtime/cc_agent/manifest.py",
    "runtime/cc_agent/service.py", "runtime/cc_agent/history.py", "runtime/cc_agent/audit.py",
    "runtime/cc_agent/history_reader.py", "runtime/cc_agent/http_ingress.py",
    "runtime/cc_agent/read_expiry.py", "runtime/cc_agent/read_access.py", "runtime/cc_agent/device_reader.py",
    "runtime/cc_agent/tunnel.py", "runtime/cc_agent/tunnel_audit.py", "runtime/cc_agent/wss_carrier.py",
    "runtime/cc_agent/tunnel_policy_v2.py", "runtime/cc_agent/tunnel_supervised.py",
    "runtime/cc_agent/outbound_control.py", "runtime/cc_agent/outbound_control_daemon.py",
    "collector/__init__.py", "collector/manager.py", "collector/supply.py", "collector/supply-lock.json",
    "collector/native_status.py", "collector/unit_spec.py",
    "collector/volatile_unit_adapter.py", "collector/volatile_unit_entry.py",
    "collector/service_binding.py", "collector/service_unit_spec.py",
    "collector/service_unit_adapter.py", "collector/service_unit_entry.py", "collector/service_daemon.py",
    "delivery/consolecrypt-agent-collector.service.in", "delivery/collector_service_provision.py",
    "delivery/roles/consolecrypt_agent_collector_service/defaults/main.yml",
    "delivery/roles/consolecrypt_agent_collector_service/tasks/main.yml",
    "delivery/roles/consolecrypt_agent_collector_service/meta/main.yml",
    "delivery/roles/consolecrypt_agent_collector_service/README.md",
    "collector/disk_binding.py", "collector/disk_unit_spec.py",
    "collector/disk_unit_adapter.py", "collector/disk_unit_entry.py",
    "delivery/release_platforms.py", "delivery/render_profile.py", "delivery/update_status.py", "delivery/container_preflight.py",
    "delivery/stage.py", "delivery/notice_inventory.py", "delivery/release_verifier.py",
    "delivery/monitoring_plan.py", "delivery/monitoring_prefix.py", "delivery/monitoring_transaction.py",
    "delivery/monitoring_snapshot.py", "delivery/MONITORING_CONFIG_BACKUP.md", "delivery/rules.yml",
    "delivery/roles/consolecrypt_monitoring_prefix/defaults/main.yml",
    "delivery/roles/consolecrypt_monitoring_prefix/tasks/main.yml", "delivery/roles/consolecrypt_monitoring_prefix/meta/main.yml",
    "delivery/roles/consolecrypt_monitoring_prefix/README.md",
    "delivery/roles/consolecrypt_agent_profile/defaults/main.yml",
    "delivery/roles/consolecrypt_agent_profile/tasks/main.yml", "delivery/roles/consolecrypt_agent_profile/meta/main.yml",
    "delivery/roles/consolecrypt_agent_profile/README.md",
    "delivery/roles/consolecrypt_agent_identity/defaults/main.yml",
    "delivery/roles/consolecrypt_agent_identity/tasks/main.yml", "delivery/roles/consolecrypt_agent_identity/meta/main.yml",
    "delivery/roles/consolecrypt_agent_identity/README.md")


RUNTIME_VERSION_FILE="runtime/cc_agent/__init__.py"
RUNTIME_VERSION_PATTERN=re.compile(r"(?:0|[1-9][0-9]{0,5})\.(?:0|[1-9][0-9]{0,5})\.(?:0|[1-9][0-9]{0,5})\Z")


def runtime_declared_version(raw):
    """Read a literal from captured public code without importing/executing it."""
    if type(raw)is not bytes or len(raw)>65536:raise ValueError("runtime version module budget")
    tree=ast.parse(raw.decode("utf-8"))
    assignments=[node for node in tree.body if isinstance(node,ast.Assign)
                 and len(node.targets)==1 and isinstance(node.targets[0],ast.Name)
                 and node.targets[0].id=="__version__"]
    writes=[node for node in ast.walk(tree)if isinstance(node,ast.Name)
            and node.id=="__version__"and isinstance(node.ctx,(ast.Store,ast.Del))]
    imported=[alias for node in ast.walk(tree)if isinstance(node,(ast.Import,ast.ImportFrom))
              for alias in node.names if(alias.asname or alias.name)=="__version__"or alias.name=="*"]
    if len(assignments)!=1 or len(writes)!=1 or imported:
        raise ValueError("one literal runtime version assignment required")
    node=assignments[0]
    if (not isinstance(node.value,ast.Constant)or type(node.value.value)is not str
            or not RUNTIME_VERSION_PATTERN.fullmatch(node.value.value)
            or node.lineno!=node.end_lineno or node.col_offset!=0):
        raise ValueError("literal runtime version module required")
    return assignments[0].value.value


def stamp_runtime_version(raw,display_version):
    if type(display_version)is not str or not RUNTIME_VERSION_PATTERN.fullmatch(display_version):
        raise ValueError("verified package display version required")
    original=runtime_declared_version(raw)
    tree=ast.parse(raw.decode("utf-8"));node=next(n for n in tree.body if isinstance(n,ast.Assign)and len(n.targets)==1
        and isinstance(n.targets[0],ast.Name)and n.targets[0].id=="__version__")
    lines=raw.splitlines(keepends=True);line=lines[node.lineno-1]
    # AST columns are UTF8 byte offsets: replace ONLY this literal. Preserve
    # imports, comments, whitespace/newlines and every other original byte.
    # Source/dev and old imported processes never read desired state/env.
    lines[node.lineno-1]=line[:node.value.col_offset]+json.dumps(display_version).encode()+line[node.value.end_col_offset:]
    packaged=b"".join(lines)
    if runtime_declared_version(packaged)!=display_version:raise ValueError("packaged runtime version mismatch")
    return packaged,dict(format="cc-agent-packaged-runtime-version/v1",path=RUNTIME_VERSION_FILE,
        sourceVersion=original,runtimeVersion=display_version,sourceSha256=hashlib.sha256(raw).hexdigest(),
        packagedSha256=hashlib.sha256(packaged).hexdigest(),generation="exact-version-assignment/v1")


def public_runtime_pin(path):
    path = Path(path).absolute()
    for parent in (path,*path.parents):
        metadata=parent.lstat()
        if stat.S_ISLNK(metadata.st_mode) or getattr(metadata,'st_file_attributes',0)&0x400:
            raise ValueError("supervisor runtime pin linked input")
    before=path.lstat()
    if not stat.S_ISREG(before.st_mode) or before.st_nlink!=1 or not 0<before.st_size<=4096:
        raise ValueError("supervisor runtime pin unavailable")
    identity=lambda value:(value.st_dev,value.st_ino,value.st_size,value.st_mtime_ns,value.st_ctime_ns,value.st_nlink)
    fd=os.open(path,os.O_RDONLY|getattr(os,'O_NOFOLLOW',0)|getattr(os,'O_NONBLOCK',0)|getattr(os,'O_BINARY',0))
    with os.fdopen(fd,'rb')as source:
        if identity(os.fstat(source.fileno()))!=identity(before):raise ValueError("supervisor runtime pin changed")
        raw=source.read(4097)
        if len(raw)!=before.st_size or identity(os.fstat(source.fileno()))!=identity(before) or identity(path.lstat())!=identity(before):
            raise ValueError("supervisor runtime pin changed")
    def pairs(items):
        value = {}
        for key, item in items:
            if key in value:
                raise ValueError("duplicate supervisor runtime pin field")
            value[key] = item
        return value
    value = json.loads(raw, object_pairs_hook=pairs)
    if (type(value) is not dict or set(value) != {"format", "pythonExecutable", "pythonSha256"}
            or value["format"] != "cc-tunnel-supervisor-runtime/v1"
            or type(value["pythonExecutable"]) is not str
            or value["pythonExecutable"] not in {"/usr/bin/python3.11", "/usr/bin/python3.12"}
            or type(value["pythonSha256"]) is not str
            or re.fullmatch("[0-9a-f]{64}", value["pythonSha256"]) is None
            or value["pythonSha256"] == "0" * 64):
        raise ValueError("unsupported supervisor runtime pin")
    return raw  # Never mint a fingerprint or read a host runtime/private key.


VMAGENT_SHA256 = "2039bc05b6ad5c96b34b72801908896bf8a6a7bcab8ae32120cbb8e6217ef6f6"
VMAGENT_BYTES = 23209584


def collector_binary_bytes(path, *, platform="linux-amd64"):
    selected = release_platforms.select(platform, "collector")
    # Explicit build input only; never look up/download/copy an ambient cache.
    path = Path(path)
    if any(p.is_symlink() for p in (path, *path.parents)):
        raise ValueError("collector binary input linked")
    fd = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_NONBLOCK", 0) | getattr(os, "O_CLOEXEC", 0) | getattr(os, "O_BINARY", 0))
    try:
        before = os.fstat(fd)
        if (not stat.S_ISREG(before.st_mode) or not 64 <= before.st_size <= release_platforms.MAX_COLLECTOR
                or (selected["observedBytes"] is not None and before.st_size != selected["observedBytes"])):
            raise ValueError("collector binary input unavailable")
        chunks = []; left = before.st_size + 1
        while left:
            block = os.read(fd, min(left, 1048576))
            if not block: break
            chunks.append(block); left -= len(block)
        raw = b"".join(chunks); after = os.fstat(fd); named = path.stat(follow_symlinks=False)
        ident = lambda m: (m.st_dev, m.st_ino, m.st_size, m.st_mtime_ns, m.st_ctime_ns)
        if (ident(before) != ident(after) or ident(before) != ident(named)
                or len(raw) != before.st_size):
            raise ValueError("collector binary input changed or unsupported")
        return release_platforms.collector_payload(raw, selected)
    finally:
        os.close(fd)


def disk_binary_bytes(binary, report, *, platform="linux-amd64"):
    selected = release_platforms.select(platform, "collector")
    # Explicit normal-build artifact plus its public observed provenance only.
    # This is not signature authority: the existing signed release verifier is.
    path = Path(binary); proof = Path(report)
    if path.is_symlink() or proof.is_symlink() or not 0 < proof.stat().st_size <= 16384:
        raise ValueError('normal disk collector build provenance unavailable')
    record = json.loads(proof.read_bytes())
    if (record.get('passed') is not True or record.get('normalProductBuild') is not True
            or record.get('sourceUnchanged') is not True or record.get('noDaemonStarted') is not True
            or not re.fullmatch('[0-9a-f]{64}', record.get('sourceMapSha256', ''))):
        raise ValueError('normal disk collector build provenance incomplete')
    fd = os.open(path, os.O_RDONLY | getattr(os, 'O_NOFOLLOW', 0) | getattr(os, 'O_NONBLOCK', 0))
    try:
        before = os.fstat(fd)
        if not stat.S_ISREG(before.st_mode) or before.st_nlink != 1 or not 64 <= before.st_size <= release_platforms.MAX_DISK_BINARY:
            raise ValueError('normal disk collector exceeds existing release budget')
        with os.fdopen(os.dup(fd), 'rb') as source: raw = source.read(release_platforms.MAX_DISK_BINARY + 1)
        after = os.fstat(fd); named = path.stat(follow_symlinks=False)
        identity = lambda st: (st.st_dev, st.st_ino, st.st_size, st.st_mtime_ns, st.st_ctime_ns)
        if (identity(before) != identity(after) or identity(before) != identity(named)
                or len(raw) != before.st_size or len(raw) != record.get('binaryBytes')
                or hashlib.sha256(raw).hexdigest() != record.get('binarySha256')
                or raw[:6] != b'\x7fELF\x02\x01' or int.from_bytes(raw[18:20], 'little') != selected["elfMachine"]):
            raise ValueError('normal disk collector input changed')
        return raw
    finally: os.close(fd)


def explicit_files(binary, runtime_pin=None, collector_binary=None, disk_binary=None, disk_report=None, display_version=None):
    files = {name: (AGENT / name).read_bytes() for name in SOURCES}
    runtime_stamp=None
    if display_version is not None:
        files[RUNTIME_VERSION_FILE],runtime_stamp=stamp_runtime_version(files[RUNTIME_VERSION_FILE],display_version)
    files["launch.py"] = (DIRECTORY / "launch.py").read_bytes()
    files["systemd/consolecrypt-agent.service"] = (DIRECTORY / "consolecrypt-agent.service").read_bytes()
    files["bin/cc-agent-identity"] = Path(binary).read_bytes()
    selected = release_platforms.elf_platform(files["bin/cc-agent-identity"])
    if collector_binary is None:
        raise ValueError("explicit reviewed collector binary input required")
    files["collector/vmagent-prod"] = collector_binary_bytes(collector_binary, platform=selected["collector"])
    if (disk_binary is None) != (disk_report is None):
        raise ValueError('normal disk collector requires both artifact and observed build report')
    if disk_binary is not None:
        files['collector/prometheus-hardmode'] = disk_binary_bytes(disk_binary, disk_report, platform=selected["collector"])
    files.update(wheel_file(AGENT))
    if runtime_pin is not None:
        files["python/supervisor-runtime.json"] = public_runtime_pin(runtime_pin)
    files[INVENTORY] = (DIRECTORY / INVENTORY).read_bytes()
    inventory = json.loads(files[INVENTORY])
    wheels = [item for item in inventory["components"] if item.get("kind") == "bundled-python-wheel"
              and item.get("name") == "websockets" and item.get("version") == "17.4"]
    if (len(wheels) != 1 or wheels[0].get("wheel_sha256") != WSS_SHA256
            or wheels[0].get("package_path") != WSS_PACKAGE
            or not wheels[0].get("notices")):
        raise ValueError("current WSS wheel and full notice inventory binding required")
    source_components = [item for item in inventory["components"] if item["kind"] == "bundled-agent-source"]
    if len(source_components) != 1:
        raise ValueError("Agent source component inventory required")
    if source_components[0].get("runtime_version_stamp")!=runtime_stamp:
        raise ValueError("original/generated runtime version source custody mismatch")
    declared = source_components[0].get("source_file_sha256")
    source_names = set(SOURCES) | {"launch.py", "systemd/consolecrypt-agent.service"}
    if runtime_pin is not None:
        source_names.add("python/supervisor-runtime.json")
    if (not isinstance(declared, dict) or set(declared) != source_names
            or any(hashlib.sha256(files[name]).hexdigest() != expected for name, expected in declared.items())):
        raise ValueError("inventory/Agent source input mismatch")
    binaries = [item for item in inventory["components"] if item["kind"] == "bundled-agent-binary"]
    if len(binaries) != 1 or binaries[0]["binary_sha256"] != hashlib.sha256(files["bin/cc-agent-identity"]).hexdigest():
        raise ValueError("inventory/bundled binary mismatch")
    collectors = [item for item in inventory["components"] if item.get("kind") == "bundled-collector-binary"]
    ram_collectors = [item for item in collectors if item.get('name') == 'vmagent']
    if (len(ram_collectors) != 1 or ram_collectors[0].get("name") != "vmagent"
            or ram_collectors[0].get("version") != "v1.156.0" or ram_collectors[0].get("platform") != selected["collector"]
            or ram_collectors[0].get("package_path") != "collector/vmagent-prod"
            or ram_collectors[0].get("binary_sha256") != selected["binarySha256"]
            or ram_collectors[0].get("binary_bytes") != len(files["collector/vmagent-prod"]) or not ram_collectors[0].get("notices")):
        raise ValueError("pinned collector binary and full notice inventory binding required")
    disk_collectors = [item for item in collectors if item.get('name') == 'prometheus']
    if len(collectors) != 1 + (disk_binary is not None) or len(disk_collectors) != (disk_binary is not None):
        raise ValueError('unexpected collector notice components')
    if disk_binary is not None:
        c = disk_collectors[0]; raw = files['collector/prometheus-hardmode']
        if (c.get('version') != '3.16.0' or c.get('platform') != selected['collector']
                or c.get('package_path') != 'collector/prometheus-hardmode' or not c.get('notices')
                or c.get('binary_sha256') != hashlib.sha256(raw).hexdigest() or c.get('binary_bytes') != len(raw)):
            raise ValueError('normal disk collector and full notices binding required')
    for component in inventory["components"]:
        for notice in component.get("notices", []):
            path = notice["package_path"]
            files[path] = (DIRECTORY / path).read_bytes()
    validate_notice_inventory(files, required=True)
    if any(part in {"examples", "tests", ".cache", "target", "__pycache__"} for name in files for part in Path(name).parts):
        raise ValueError("unexpected prototype package input")
    return files


def reproducibility(binary, runtime_pin=None, collector_binary=None, disk_binary=None, disk_report=None, display_version=None):
    with tempfile.TemporaryDirectory(prefix="cc-agent-repro-package-") as temporary:
        directory = Path(temporary)
        first, second = directory / "first.zip", directory / "second.zip"
        before = explicit_files(binary, runtime_pin, collector_binary, disk_binary, disk_report,display_version)
        executable = {"bin/cc-agent-identity", "collector/vmagent-prod"} | ({'collector/prometheus-hardmode'} if disk_binary is not None else set())
        build_package(before, first, display_version or "0.1.0", executable)
        after = explicit_files(binary, runtime_pin, collector_binary, disk_binary, disk_report,display_version)
        if before != after:
            raise ValueError("package inputs changed")
        build_package(after, second, display_version or "0.1.0", executable)
        if first.read_bytes() != second.read_bytes():
            raise ValueError("ZIP output not reproducible from unchanged input bytes")
        digest = hashlib.sha256(first.read_bytes()).hexdigest()
        report = {"format": "cc-agent-repro-package-proof/v1", "passed": True,
                  "artifact_sha256": digest, "artifact_bytes": first.stat().st_size, "file_count": len(before),
                  "input_file_sha256": {name: hashlib.sha256(value).hexdigest() for name, value in sorted(before.items())},
                  "notice_validation": validate_notice_inventory(before, required=True),
                  "python": sys.version.split()[0], "zlib": zlib.ZLIB_VERSION,
                  "claim": "same ZIP bytes from the same explicit inputs and packager; not a reproducible Rust build",
                  "examples_or_private_state_included": False, "production_distribution_ready": False}
    report["temporary_objects_cleaned"] = not directory.exists()
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--identity-binary", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--supervisor-runtime-pin", type=Path,
                        help="explicit reviewed target-Host system Python3.11/3.12 path+SHA public pin; no discovery, fallback or generation")
    parser.add_argument("--collector-binary", type=Path, required=True, help="explicit reviewed fixed vmagent v1.156.0 linux-amd64 binary; no cache discovery")
    parser.add_argument('--disk-binary', type=Path, help='explicit normal cmd/prometheus ELF; no test ELF or cache lookup')
    parser.add_argument('--disk-build-report', type=Path, help='matching completed normal build report; no authority claim')
    parser.add_argument('--display-version',help='explicit package display/runtime stamp version; omit only for legacy/dev inventory')
    args = parser.parse_args()
    report = reproducibility(args.identity_binary, args.supervisor_runtime_pin, args.collector_binary, args.disk_binary, args.disk_build_report,args.display_version)
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, sort_keys=True, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({key: value for key, value in report.items() if key != "input_file_sha256"}, indent=2))


if __name__ == "__main__":
    main()
