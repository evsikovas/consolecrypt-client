"""Unprivileged package staging only. Never calls systemctl, Docker, apt or a shell.

Signature verification is injected by the identity bridge; omission always denies.
This is not a host installer and accepts only an explicitly initialized staging root.
"""

import argparse
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import secrets
import stat
import sys
import tempfile
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parent))
from notice_inventory import validate_notice_inventory
import release_platforms

MAX_PACKAGE = 96 * 1024 * 1024
MAX_EXPANDED = 256 * 1024 * 1024
MAX_FILES = 512
MAX_METADATA = 128 * 1024
VERSION = re.compile(r"(?:0|[1-9][0-9]{0,5})\.(?:0|[1-9][0-9]{0,5})\.(?:0|[1-9][0-9]{0,5})\Z")
MARKER = ".cc-agent-staging.json"


class StageError(ValueError):
    pass


def digest(content):
    return hashlib.sha256(content).hexdigest()


def _json(content):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise StageError("duplicate JSON field")
            result[key] = value
        return result
    try:
        return json.loads(content, object_pairs_hook=unique)
    except (ValueError, UnicodeError):
        raise StageError("invalid package JSON") from None


def _safe_path(path):
    path = Path(os.path.abspath(path))
    for component in (path, *path.parents):
        if component.exists() or component.is_symlink():
            info = component.lstat()
            if stat.S_ISLNK(info.st_mode) or getattr(info, "st_file_attributes", 0) & 0x400:
                raise StageError("symlink/reparse path refused")
    return path


def _root(path):
    path = _safe_path(path)
    if path == Path(path.anchor) or len(path.parts) < 3:
        raise StageError("staging root too broad")
    marker = _safe_path(path / MARKER)
    if not marker.is_file() or marker.stat().st_size > 1024:
        raise StageError("not an initialized staging root")
    data = _json(marker.read_bytes())
    if data.get("format") != "cc-agent-staging-v1" or data.get("path") != str(path.resolve()):
        raise StageError("staging root binding mismatch")
    return path


def initialize_root(path):
    path = _safe_path(path)
    if path == Path(path.anchor) or len(path.parts) < 3:
        raise StageError("staging root too broad")
    path.mkdir(mode=0o700, parents=False, exist_ok=True)
    if any(path.iterdir()):
        raise StageError("initialize only an empty directory")
    (path / MARKER).write_text(json.dumps({"format": "cc-agent-staging-v1", "path": str(path.resolve()),
                                          "instance": secrets.token_hex(16)}), encoding="utf-8")
    return path


def _relative(name):
    if not isinstance(name, str) or len(name) > 160 or not re.fullmatch(r"[A-Za-z0-9_./-]+", name):
        raise StageError("invalid package path")
    path = PurePosixPath(name)
    if path.is_absolute() or not path.parts or any(p in (".", "..") for p in path.parts) or str(path) != name:
        raise StageError("package path traversal")
    if any(p.startswith(".") for p in path.parts):
        raise StageError("hidden package path refused")
    reserved = {"con", "prn", "aux", "nul", *(f"com{i}" for i in range(1, 10)), *(f"lpt{i}" for i in range(1, 10))}
    if any(p.endswith((".", " ")) or p.split(".")[0].lower() in reserved for p in path.parts):
        raise StageError("nonportable package path")
    return path


def _public_parents(parent, root):
    # Only public software beneath the SAME initialized prefix. Never repair
    # existing dirs, or touch private identity/journal/credential directories.
    root=_root(root);parent=_safe_path(parent)
    try:relative=parent.relative_to(root)
    except ValueError:raise StageError('public software path escaped owned prefix') from None
    def checked(path, fd=None):
        named=path.lstat();held=os.fstat(fd)if fd is not None else named
        identity=lambda s:(s.st_dev,s.st_ino,s.st_uid,s.st_mode)
        if (not stat.S_ISDIR(held.st_mode) or held.st_uid!=0 or stat.S_IMODE(held.st_mode)!=0o755
                or identity(named)!=identity(held)):
            raise StageError('existing public software directory requires exact root0755')
    checked(root)
    current=root
    for part in relative.parts:
        current=current/part
        try:current.mkdir(mode=0o700) # Exclusive; root umask077 cannot remove owner access.
        except FileExistsError:
            _safe_path(current);checked(current) # No chmod/adoption of an existing leaf.
            continue
        flags=os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_NONBLOCK|os.O_CLOEXEC
        fd=os.open(current,flags)
        try:
            st=os.fstat(fd);named=current.lstat()
            if (not stat.S_ISDIR(st.st_mode) or st.st_uid!=0 or st.st_mode&0o077
                    or (st.st_dev,st.st_ino)!=(named.st_dev,named.st_ino)):
                raise StageError('new public software directory custody changed')
            os.fchmod(fd,0o755);os.fsync(fd);checked(current,fd)
        finally:os.close(fd)
    _safe_path(parent);checked(parent)


def _write(path, content, mode=0o644, *, public_root=None):
    path = _safe_path(path)
    if sys.platform=='linux' and os.geteuid()==0 and public_root is not None:
        _public_parents(path.parent,public_root)
    else:
        path.parent.mkdir(mode=0o755, parents=True, exist_ok=True)
    # An initialized root is owned by the staging caller; no component may be
    # a symlink/junction. Recheck after directory creation too.
    _safe_path(path)
    fd, temporary = tempfile.mkstemp(prefix=".stage-", dir=path.parent)
    try:
        with os.fdopen(fd, "wb") as stream:
            stream.write(content)
            stream.flush()
            os.fsync(stream.fileno())
        os.chmod(temporary, mode)
        os.replace(temporary, path)
        if sys.platform == 'linux':
            # Linux directory entry durability for each existing atomic write;
            # this is not a multi-file/power-failure transaction guarantee.
            parent = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
            try:
                os.fsync(parent)
            finally:
                os.close(parent)
    finally:
        Path(temporary).unlink(missing_ok=True)


def _check_mode(path, expected):
    if os.name != "nt" and stat.S_IMODE(path.stat().st_mode) != int(expected, 8):
        raise StageError("staged release mode changed")


def _anchor(root):
    marker = _json(_safe_path(root / MARKER).read_bytes())
    path = _safe_path(root / "var/lib/consolecrypt-agent/release-anchor.json")
    if marker.get("release_anchor_required") and not path.is_file():
        raise StageError("release high-water anchor missing")
    if not path.exists():
        return path, None
    if path.stat().st_size > 4096:
        raise StageError("release high-water anchor oversized")
    value = _json(path.read_bytes())
    if (not isinstance(value, dict) or type(value.get("version")) is not int or value["version"] < 1
            or type(value.get("protocol")) is not int or value["protocol"] != 1
            or not isinstance(value.get("displayVersion"), str) or not VERSION.fullmatch(value["displayVersion"])
            or not isinstance(value.get("artifactSha256"), str) or not re.fullmatch(r"[0-9a-f]{64}", value["artifactSha256"])):
        raise StageError("release high-water anchor invalid")
    return path, value


COLLECTOR_FILES = ("collector/unit_spec.py", "collector/volatile_unit_adapter.py", "collector/volatile_unit_entry.py", "collector/vmagent-prod")
COLLECTOR_VM_SHA256 = "2039bc05b6ad5c96b34b72801908896bf8a6a7bcab8ae32120cbb8e6217ef6f6"
DISK_FILES = ('collector/disk_binding.py', 'collector/disk_unit_spec.py',
              'collector/disk_unit_adapter.py', 'collector/disk_unit_entry.py', 'collector/prometheus-hardmode')


def _disk_files(files, modes=None):
    present = set(DISK_FILES) & set(files)
    # New sources may accompany a RAM-only package; without a binary there is
    # no installed/advertised disk mode. Any actual disk binary needs closure.
    if 'collector/prometheus-hardmode' not in present:
        return False
    if present != set(DISK_FILES): raise StageError('incomplete signed disk collector runtime')
    binary = files[DISK_FILES[-1]]
    # Only the two fixed ELF identities. stage_release additionally requires
    # EVERY component to match the SAME original verified release platform;
    # legacy installed/runtime paths remain AMD64 and never auto-convert.
    if (not 64 <= len(binary) <= release_platforms.MAX_DISK_BINARY or binary[:6] != b'\x7fELF\x02\x01'
            or int.from_bytes(binary[18:20], 'little') not in {62,183}):
        raise StageError('unsupported signed disk collector binary')
    if any(not 0 < len(files[n]) <= 65536 for n in DISK_FILES[:-1]):
        raise StageError('disk collector runtime source budget')
    if modes is not None and any(modes[n] != ('0755' if n == DISK_FILES[-1] else '0644') for n in DISK_FILES):
        raise StageError('signed disk collector runtime modes')
    return True


SERVICE_FILES = ('collector/service_binding.py', 'collector/service_unit_spec.py',
                 'collector/service_unit_adapter.py', 'collector/service_unit_entry.py', 'collector/service_daemon.py')

def _service_files(files, modes=None):
    present = set(SERVICE_FILES) & set(files)
    if not present:
        return False
    if present != set(SERVICE_FILES):
        raise StageError('incomplete signed collector service source closure')
    if any(not 0 < len(files[name]) <= 65536 for name in SERVICE_FILES):
        raise StageError('collector service source budget')
    if modes is not None and any(modes[name] != '0644' for name in SERVICE_FILES):
        raise StageError('collector service signed source modes')
    return True

def _collector_files(files, modes=None):
    _disk_files(files, modes)
    _service_files(files, modes)
    # Old packages with NONE remain old packages. A partial new closure denies.
    present = set(COLLECTOR_FILES) & set(files)
    if not present: return False
    if present != set(COLLECTOR_FILES): raise StageError("incomplete signed collector runtime")
    binary = files["collector/vmagent-prod"]
    try:
        if len(binary)>23209584: raise ValueError('existing collector per-leaf cap')
        release_platforms.collector_payload(binary,release_platforms.elf_platform(binary))
    except ValueError: raise StageError("unsupported signed collector binary") from None
    if any(not 0 < len(files[n]) <= 65536 for n in COLLECTOR_FILES[:-1]):
        raise StageError("collector runtime source budget")
    if modes is not None and any(modes[n] != ("0755" if n.endswith("vmagent-prod") else "0644") for n in COLLECTOR_FILES):
        raise StageError("signed collector runtime modes")
    return True


def _managed_files(root, code, files, *, public_install=False):
    sources = {"launch.py": code / "launch.py",
               "systemd/consolecrypt-agent.service": root / "etc/systemd/system/consolecrypt-agent.service"}
    if _collector_files(files):
        sources.update({name: code / name for name in COLLECTOR_FILES})
    if _disk_files(files):
        sources.update({name: code / name for name in DISK_FILES})
    if _service_files(files):
        sources.update({name: code / name for name in SERVICE_FILES})
    managed_path = _safe_path(code / "managed.json")
    previous = _json(managed_path.read_bytes()) if managed_path.exists() else {}
    new = {str(destination.relative_to(root)).replace("\\", "/"): files[source]
           for source, destination in sources.items() if source in files}
    for name, expected in previous.items():
        _relative(name)
        path = _safe_path(root / name)
        if path.exists() and digest(path.read_bytes()) != expected:
            raise StageError("managed launcher or service was modified")
    for name, value in new.items():
        path = _safe_path(root / name)
        if path.exists() and name not in previous:
            raise StageError("unowned launcher or service exists")
        _write(path, value, 0o755 if name.endswith(("/collector/vmagent-prod", '/collector/prometheus-hardmode')) else 0o644, public_root=root if public_install else None)
    for name in previous.keys() - new.keys():
        _safe_path(root / name).unlink(missing_ok=True)
    _write(managed_path, json.dumps({name: digest(value) for name, value in new.items()}, sort_keys=True).encode())


def build_package(files, output, version="0.1.0", executable_paths=frozenset()):
    """Build a deterministic package from explicit file bytes, never directory copies."""
    if not VERSION.fullmatch(version) or not files or len(files) > MAX_FILES:
        raise StageError("package version/file count")
    files = {str(_relative(name)): content for name, content in files.items()}
    if ({"package.json", "installed.json"} & {name.lower() for name in files}
            or len({name.lower() for name in files}) != len(files)
            or sum(len(content) for content in files.values()) > MAX_EXPANDED
            or not set(executable_paths) <= set(files)
            or not set(executable_paths) <= {"bin/cc-agent-identity", "identity/bin/cc-agent-identity", "collector/vmagent-prod", "collector/prometheus-hardmode"}):
        raise StageError("package budget")
    if _collector_files(files):
        _collector_files(files, {n: "0755" if n in executable_paths else "0644" for n in files})
    manifest = {"format": "cc-agent-package-v1", "version": version, "protocol": 1,
                "files": {name: digest(content) for name, content in sorted(files.items())},
                "file_modes": {name: "0755" if name in executable_paths else "0644" for name in sorted(files)}}
    try:
        validate_notice_inventory(files)
    except ValueError as error:
        raise StageError(str(error)) from None
    content = json.dumps(manifest, sort_keys=True, separators=(",", ":")).encode()
    if len(content) > MAX_METADATA:
        raise StageError("package manifest budget")
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED) as package:
        for name, value in [("package.json", content), *sorted(files.items())]:
            info = zipfile.ZipInfo(name, (2026, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = (stat.S_IFREG | 0o644) << 16
            package.writestr(info, value)
    return manifest


def stage_release(root, artifact, verify=None, *, public_install=False):
    root = _root(root)
    marker_path = root / MARKER
    marker = _json(marker_path.read_bytes())
    anchor_path, anchor = _anchor(root)
    artifact = _safe_path(artifact)
    if not artifact.is_file() or artifact.stat().st_size > MAX_PACKAGE:
        raise StageError("package size")
    before = artifact.read_bytes()
    artifact_sha = digest(before)
    if anchor is not None and hasattr(verify, "current_version"):
        # Derive the CLI replay floor from persisted state, never reset it from
        # a repeated --current-version 0. An exact known artifact may be
        # cryptographically rechecked for idempotent installation.
        same = anchor.get("artifactSha256") == artifact_sha
        verify.current_version = max(verify.current_version, anchor["version"] - (1 if same else 0))
    if verify is None or verify(artifact) is not True:
        raise StageError("signature verification denied")
    # Verification and extraction must observe the identical artifact.
    if digest(artifact.read_bytes()) != artifact_sha:
        raise StageError("package changed after verification")
    try:
        with zipfile.ZipFile(io.BytesIO(before)) as archive:
            infos = archive.infolist()
            if len(infos) > MAX_FILES + 1 or len({item.filename.lower() for item in infos}) != len(infos):
                raise StageError("package file count/duplicate")
            if sum(item.file_size for item in infos) > MAX_EXPANDED + MAX_METADATA:
                raise StageError("expanded package budget")
            for item in infos:
                _relative(item.filename)
                if not stat.S_ISREG(item.external_attr >> 16) or item.is_dir():
                    raise StageError("only regular package files allowed")
            if archive.getinfo("package.json").file_size > MAX_METADATA:
                raise StageError("package manifest budget")
            manifest_bytes = archive.read("package.json")
            if len(manifest_bytes) > MAX_METADATA:
                raise StageError("package manifest budget")
            manifest = _json(manifest_bytes)
            if (not isinstance(manifest, dict) or set(manifest) != {"format", "version", "protocol", "files", "file_modes"}
                    or manifest["format"] != "cc-agent-package-v1" or type(manifest["protocol"]) is not int
                    or manifest["protocol"] != 1 or not VERSION.fullmatch(manifest["version"])
                    or not isinstance(manifest["files"], dict) or not manifest["files"]
                    or not isinstance(manifest["file_modes"], dict)
                    or set(manifest["file_modes"]) != set(manifest["files"])
                    or {"package.json", "installed.json"} & manifest["files"].keys()):
                raise StageError("unsupported package manifest")
            if set(manifest["files"]) | {"package.json"} != {item.filename for item in infos}:
                raise StageError("unlisted package files")
            files = {}
            for name, expected in manifest["files"].items():
                mode = manifest["file_modes"][name]
                if (mode not in ("0644", "0755") or mode == "0755" and name not in
                        {"bin/cc-agent-identity", "identity/bin/cc-agent-identity", "collector/vmagent-prod", "collector/prometheus-hardmode"}):
                    raise StageError("executable mode not allowed")
                value = archive.read(name)
                if digest(value) != expected:
                    raise StageError("package file digest mismatch")
                files[name] = value
            _collector_files(files, manifest["file_modes"])
            try:
                validate_notice_inventory(files)
            except ValueError as error:
                raise StageError(str(error)) from None
    except (KeyError, zipfile.BadZipFile, RuntimeError, TypeError):
        raise StageError("invalid package archive") from None
    code = _safe_path(root / "opt/consolecrypt-agent")
    active = _safe_path(code / "current.json")
    previous = _json(active.read_bytes()) if active.exists() else None
    version = manifest["version"]
    release_attestation = getattr(verify, "verified_release", None)
    if release_attestation is not None:
        try:
            release_attestation=release_platforms.verified_release(release_attestation)
            selected=release_platforms.select(release_attestation['platform'],'release')
            for name in ('bin/cc-agent-identity','identity/bin/cc-agent-identity','collector/vmagent-prod','collector/prometheus-hardmode'):
                if name in files: release_platforms.require_elf(files[name],selected)
        except ValueError: raise StageError('signed release component architecture mismatch') from None
    if release_attestation is not None and (release_attestation["displayVersion"] != version
            or release_attestation["artifactSha256"] != artifact_sha
            or release_attestation["artifactBytes"] != len(before)):
        raise StageError("release signature and package metadata mismatch")
    if release_attestation is not None:
        preview={"version":version,"artifact_sha256":artifact_sha,
            "files":{**manifest['files'],'package.json':digest(manifest_bytes)},
            "file_modes":{**manifest['file_modes'],'package.json':'0644'},
            "format":'cc-agent-installed-release/v2','verified_release':release_attestation}
        if len(json.dumps(preview,sort_keys=True).encode())>MAX_METADATA:
            raise StageError('installed release metadata budget')
        if anchor is not None and (release_attestation["version"] < anchor["version"]
                or release_attestation["version"] == anchor["version"] and
                (release_attestation["artifactSha256"] != anchor["artifactSha256"]
                 or release_attestation["displayVersion"] != anchor["displayVersion"])):
            raise StageError("release sequence rollback/conflict")
        # Public release checkpoint: root-owned read-only to the service user.
        _write(anchor_path, json.dumps(release_attestation, sort_keys=True).encode(), 0o644, public_root=root if public_install else None)
        marker["release_anchor_required"] = True
        _write(marker_path, json.dumps(marker, sort_keys=True).encode(), 0o600)
    if previous and tuple(map(int, version.split("."))) < tuple(map(int, previous["version"].split("."))):
        raise StageError("downgrade requires explicit rollback")
    release_dir = _safe_path(code / "releases" / version)
    metadata = {"version": version, "artifact_sha256": artifact_sha,
                "files": {**manifest["files"], "package.json": digest(manifest_bytes)},
                "file_modes": {**manifest["file_modes"], "package.json": "0644"}}
    if release_attestation is not None:
        metadata.update(format="cc-agent-installed-release/v2",verified_release=release_attestation)
    record = _safe_path(release_dir / "installed.json")
    if record.exists():
        if _json(record.read_bytes()) != metadata:
            raise StageError("version already staged with different content")
    else:
        # Preflight all destinations before writing, including Windows junctions.
        for name in [*files, "package.json", "installed.json"]:
            destination = _safe_path(release_dir / name)
            if destination.exists():
                raise StageError("existing unowned release file")
        for name, value in files.items():
            _write(release_dir / name, value, int(manifest["file_modes"][name], 8), public_root=root if public_install else None)
        _write(release_dir / "package.json", manifest_bytes, public_root=root if public_install else None)
        _write(record, json.dumps(metadata, sort_keys=True).encode(), public_root=root if public_install else None)
    # Verify persisted bytes even on idempotent reinstallation.
    for name, expected in metadata["files"].items():
        path = _safe_path(release_dir / name)
        if digest(path.read_bytes()) != expected:
            raise StageError("staged release changed")
        _check_mode(path, metadata["file_modes"][name])
    # Stable entrypoint and unit are copied only from the verified package.
    # They remain inert files beneath the explicit staging root.
    _managed_files(root, code, files, public_install=public_install)
    current = {"version": version, "artifact_sha256": artifact_sha,
               "previous_version": previous["version"] if previous and previous["version"] != version else (previous or {}).get("previous_version")}
    _write(active, json.dumps(current, sort_keys=True).encode(), public_root=root if public_install else None)
    return current


def rollback(root, *, public_install=False):
    root = _root(root)
    _anchor(root)  # An explicitly requested rollback cannot recreate a lost floor.
    code = root / "opt/consolecrypt-agent"
    current = _json(_safe_path(code / "current.json").read_bytes())
    version = current.get("previous_version")
    if not isinstance(version, str) or not VERSION.fullmatch(version):
        raise StageError("no known rollback release")
    directory = _safe_path(code / "releases" / version)
    record = _json(_safe_path(directory / "installed.json").read_bytes())
    for name, expected in record["files"].items():
        _relative(name)
        path = _safe_path(directory / name)
        if digest(path.read_bytes()) != expected:
            raise StageError("rollback release changed")
        _check_mode(path, record["file_modes"][name])
    managed = {name: _safe_path(directory / name).read_bytes()
               for name in ("launch.py", "systemd/consolecrypt-agent.service", *COLLECTOR_FILES, *DISK_FILES, *SERVICE_FILES)
               if name in record["files"]}
    _collector_files(managed, record["file_modes"])
    _managed_files(root, code, managed, public_install=public_install)
    value = {"version": version, "artifact_sha256": record["artifact_sha256"], "previous_version": current["version"]}
    _write(code / "current.json", json.dumps(value, sort_keys=True).encode(), public_root=root if public_install else None)
    return value


def uninstall(root):
    """Remove only unchanged, recorded staged code. Preserve all config and metrics."""
    root = _root(root)
    code = _safe_path(root / "opt/consolecrypt-agent")
    result = {"removed": [], "preserved_modified": [], "history_preserved": True, "config_preserved": True}
    releases = _safe_path(code / "releases")
    managed_path = _safe_path(code / "managed.json")
    if managed_path.exists():
        managed = _json(managed_path.read_bytes())
        for name, expected in managed.items():
            _relative(name)
            path = _safe_path(root / name)
            if path.exists() and digest(path.read_bytes()) == expected:
                path.unlink()
                result["removed"].append(name)
            elif path.exists():
                result["preserved_modified"].append(name)
        managed_path.unlink()
    if releases.exists():
        for directory in releases.iterdir():
            _safe_path(directory)
            if not directory.is_dir() or not VERSION.fullmatch(directory.name):
                continue
            record_path = _safe_path(directory / "installed.json")
            if not record_path.is_file():
                continue
            record = _json(record_path.read_bytes())
            for name, expected in record["files"].items():
                _relative(name)
                path = _safe_path(directory / name)
                if path.exists() and digest(path.read_bytes()) != expected:
                    result["preserved_modified"].append(str(path.relative_to(root)))
                elif path.exists():
                    path.unlink()
                    result["removed"].append(str(path.relative_to(root)))
                    parent = path.parent
                    while parent != directory:
                        try:
                            parent.rmdir()
                        except OSError:
                            break
                        parent = parent.parent
            record_path.unlink()
            try:
                directory.rmdir()
            except OSError:
                pass
    pointer = _safe_path(code / "current.json")
    pointer.unlink(missing_ok=True)
    return result


def main():
    parser = argparse.ArgumentParser(description="Staging-only Agent package management; no host install")
    parser.add_argument("action", choices=("init", "install", "uninstall", "rollback"))
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--artifact", type=Path)
    parser.add_argument("--verifier", type=Path)
    parser.add_argument("--verifier-sha256")
    parser.add_argument("--public-key", type=Path)
    parser.add_argument("--release-manifest", type=Path)
    parser.add_argument("--platform", choices=("linux-x86_64", "linux-aarch64"))
    parser.add_argument("--current-version", type=int)
    args = parser.parse_args()
    if args.action == "install":
        required = (args.artifact, args.verifier, args.verifier_sha256, args.public_key,
                    args.release_manifest, args.platform, args.current_version)
        if any(value is None for value in required):
            parser.error("install requires artifact, pinned verifier, public-key, signed release-manifest, platform and current-version")
        from release_verifier import ReleaseVerifier
        verifier = ReleaseVerifier(args.verifier, args.verifier_sha256, args.public_key,
                                   args.release_manifest, platform=args.platform, current_version=args.current_version)
        print(json.dumps(stage_release(args.root, args.artifact, verifier)))
        return
    functions = {"init": initialize_root, "uninstall": uninstall, "rollback": rollback}
    result = functions[args.action](args.root)
    print(json.dumps(str(result) if isinstance(result, Path) else result))


if __name__ == "__main__":
    main()
