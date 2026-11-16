"""Current build -> exact notices -> existing package -> unsigned typed release.
No keys, signature implementation, downloads, installation or publication here.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import time
import release_platforms
import release_dependency_sources

AGENT = Path(__file__).resolve().parent.parent
TARGETS = {"x86_64-unknown-linux-gnu": ("x86_64", "linux-x86_64"), "aarch64-unknown-linux-gnu": ("aarch64", "linux-aarch64")}


def sha(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def write(path, value):
    with path.open("xb") as stream:
        stream.write((json.dumps(value, sort_keys=True, indent=2) + "\n").encode())


def source_map():
    import release_dependency_sources
    dirty = subprocess.check_output(["git", "-C", str(AGENT.parent), "status", "--porcelain", "--untracked-files=all", "--", "agent", *release_dependency_sources.dirty_paths(AGENT)])
    if dirty.strip():
        raise ValueError("committed exact Agent source required for corresponding-source archive")
    spec = importlib.util.spec_from_file_location("agent_source_check", AGENT / "scripts/check_source.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    values = module.inventory()
    module.check(values)
    return {name: item["sha256"] for name, item in sorted(values.items())}


def dependency_frame(work, label):
    import release_dependency_sources
    import notice_generate
    metadata = None
    if release_dependency_sources.enabled(AGENT):
        # cargo fetch has already completed in the explicit CI public-fetch
        # step. No network or build scripts are allowed during this capture.
        env = {n: os.environ[n] for n in ("PATH", "HOME", "CARGO_HOME", "RUSTUP_HOME",
            "RUSTUP_TOOLCHAIN", "SYSTEMROOT", "WINDIR", "TMP", "TEMP", "TMPDIR", "LANG", "TZ") if n in os.environ}
        env["CARGO_NET_OFFLINE"] = "true"
        observed = subprocess.run(["cargo", "metadata", "--offline", "--locked", "--format-version", "1",
            "--manifest-path", str(AGENT / "identity/Cargo.toml")], env=env, stdin=subprocess.DEVNULL,
            capture_output=True, timeout=60, check=True)
        if len(observed.stdout) > 8 * 1024**2 or len(observed.stderr) > 16384:
            raise ValueError("bounded actual locked dependency metadata required")
        with (work / ("dependency-metadata-" + label + ".json")).open("xb") as f:
            f.write(observed.stdout)
        metadata = json.loads(observed.stdout)
    return release_dependency_sources.capture(AGENT, metadata, notice_generate.read)


def before(work, target):
    release_platforms.require_native(release_platforms.select(target, 'target'))
    if not work.is_absolute() or work.exists() or work.parent.resolve() != work.parent:
        raise ValueError("new absolute owned output required")
    work.mkdir(mode=0o700)
    original = source_map()
    dependency = dependency_frame(work, "before")
    write(work / "source-before.json", dict(target=target, source=original, dependencySourceBefore=dependency))


def artifacts(log, target):
    values = {}
    finished = []
    for line in log.read_text(encoding="utf-8").splitlines():
        try:
            row = json.loads(line)
        except ValueError:
            continue
        if row.get("reason") == "build-finished":
            finished.append(row.get("success"))
        if row.get("reason") == "compiler-artifact" and row.get("executable"):
            name = row["target"]["name"]
            if name in {"cc-agent-identity", "cc-agent-release-sign"}:
                if name in values or row["profile"]["test"] or row["profile"].get("opt_level") != "3":
                    raise ValueError("unique normal release binaries required")
                path = Path(row["executable"])
                if not path.is_absolute() or path.is_symlink() or path.parent.parts[-2:] != (target, "release"):
                    raise ValueError("actual target artifact path mismatch")
                with path.open("rb") as stream:
                    header = stream.read(20)
                if (header[:6] != b"\x7fELF\x02\x01" or int.from_bytes(header[18:20], "little")
                        != {"x86_64-unknown-linux-gnu": 62, "aarch64-unknown-linux-gnu": 183}[target]):
                    raise ValueError("actual ELF architecture mismatch")
                values[name] = dict(path=str(path), sha256=sha(path), bytes=path.stat().st_size)
    if set(values) != {"cc-agent-identity", "cc-agent-release-sign"} or finished != [True]:
        raise ValueError("successful exact two-binary Cargo receipt required")
    return values


def capture_collector(work, target, binary):
    row = release_platforms.select(target, 'target')
    if binary is None:
        raise ValueError("explicit pinned vmagent binary required")
    spec = importlib.util.spec_from_file_location("release_collector_supply", AGENT / "collector/supply.py")
    supply = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(supply)
    pinned = supply.lock()
    release_platforms.supply_entry(pinned, row)
    original = supply.verify_binary(binary, row['collector'])
    import notice_generate
    payload = notice_generate.read(original, release_platforms.MAX_COLLECTOR)
    release_platforms.collector_payload(payload, row)
    destination = work / "vmagent-prod"
    with destination.open("xb") as stream:
        stream.write(payload)
    destination.chmod(0o700)
    return dict(path=str(destination), sha256=row['binarySha256'], bytes=len(payload),
        platform=row['collector'], version=release_platforms.VERSION, packagePath="collector/vmagent-prod",
        supplyLockSha256=sha(AGENT / "collector/supply-lock.json"))


def bind_collector_materials(work, collector, path):
    if path is None:
        raise ValueError("complete actual Go collector materials required before signing")
    import notice_generate
    import collector_materials
    raw = notice_generate.read(path, collector_materials.MAX_JSON)
    material = collector_materials.checked(raw, collector["sha256"])
    release_platforms.material_identity(material, release_platforms.select(collector['platform'], 'collector'))
    destination = work / "collector-materials.json"
    with destination.open("xb") as stream:
        stream.write(raw)
    collector.update(materialsPath=str(destination), materialsSha256=hashlib.sha256(raw).hexdigest(),
        buildInfoSha256=material["buildInfoSha256"])


DISK_INPUT_FIELDS = {"path", "sha256", "bytes", "asBuiltReportPath", "asBuiltReportSha256",
    "sourceMapPath", "sourceMapSha256", "buildInfoPath", "buildInfoSha256",
    "versionOutputPath", "versionOutputSha256", "materialsPath", "materialsSha256",
    "sourceArchivePath", "goArchivePath", "goPath"}


def disk_input(path, target="x86_64-unknown-linux-gnu"):
    # Optional explicit data from the actual normal Prometheus builder. No
    # cache discovery, substitute upstream ELF, placeholder or inferred size.
    if path is None:
        return None
    import notice_generate
    import prototype_package
    def pairs(rows):
        value = {}
        for key, item in rows:
            if key in value:
                raise ValueError("duplicate normal disk input field")
            value[key] = item
        return value
    value = json.loads(notice_generate.read(path, 16384), object_pairs_hook=pairs)
    if (type(value) is not dict or set(value) != DISK_INPUT_FIELDS
            or type(value["bytes"]) is not int or not 64 <= value["bytes"] <= release_platforms.MAX_DISK_BINARY
            or any(type(value[n]) is not str or not Path(value[n]).is_absolute()
                   for n in DISK_INPUT_FIELDS if n.endswith("Path") or n in {"path", "goPath"})
            or any(type(value[n]) is not str or re.fullmatch("[0-9a-f]{64}", value[n]) is None
                   for n in DISK_INPUT_FIELDS if n.endswith("Sha256") or n == "sha256")):
        raise ValueError("closed actual normal disk descriptor within existing package budget required")
    selected=release_platforms.select(target,"target")
    raw = prototype_package.disk_binary_bytes(value["path"], value["asBuiltReportPath"],platform=selected["collector"])
    if len(raw) != value["bytes"] or hashlib.sha256(raw).hexdigest() != value["sha256"]:
        raise ValueError("original normal disk descriptor differs from actual artifact")
    return value


def disk_notices(notice_files, disk, target="x86_64-unknown-linux-gnu"):
    if disk is None:
        return
    from notice_inventory import INVENTORY, validate_notice_inventory
    validate_notice_inventory(notice_files, required=True)
    value = json.loads(notice_files[INVENTORY])
    rows = [r for r in value["components"] if r.get("kind") == "bundled-collector-binary"
            and r.get("name") == "prometheus"]
    if (len(rows) != 1 or rows[0].get("version") != "3.16.0"
            or rows[0].get("platform") != release_platforms.select(target,"target")["collector"]
            or rows[0].get("package_path") != "collector/prometheus-hardmode"
            or rows[0].get("binary_sha256") != disk["sha256"]
            or rows[0].get("binary_bytes") != disk["bytes"] or not rows[0].get("notices")):
        raise ValueError("verified full normal disk notice component required before package")
    # Core's existing conditional prometheus_materials.component performs the
    # actual source/buildinfo/module ZIP/stdlib verification in generate().
    # If that branch is absent or omitted it cannot be silently treated as RAM.


def candidate_budget(path):
    from stage import MAX_PACKAGE
    if path.stat().st_size > MAX_PACKAGE:
        raise ValueError("candidate exceeds unchanged compressed package budget")


def capture_supervisor_runtime(work, reviewed_pin):
    if reviewed_pin is None:return None
    import prototype_package
    raw=prototype_package.public_runtime_pin(reviewed_pin)
    value=json.loads(raw)
    copied=work/"supervisor-runtime.input.json"
    with copied.open("xb")as stream:stream.write(raw)
    if copied.read_bytes()!=raw:raise ValueError("reviewed supervisor runtime copy changed")
    return dict(format=value["format"],path=str(copied),sha256=hashlib.sha256(raw).hexdigest(),bytes=len(raw),
        pythonExecutable=value["pythonExecutable"],pythonSha256=value["pythonSha256"])


def recheck_supervisor_runtime(reviewed_pin, captured):
    if captured is None:
        if reviewed_pin is not None:raise ValueError("reviewed supervisor runtime capture missing")
        return
    import prototype_package
    raw=prototype_package.public_runtime_pin(reviewed_pin)
    if hashlib.sha256(raw).hexdigest()!=captured["sha256"] or len(raw)!=captured["bytes"]:
        raise ValueError("reviewed supervisor runtime input changed")
    copied=prototype_package.public_runtime_pin(captured["path"])
    if copied!=raw:raise ValueError("reviewed supervisor runtime copy changed")


def package(work, version, display, floor, collector_binary, collector_materials_path, disk_input_path=None, supervisor_runtime_pin=None):
    if (not re.fullmatch(r"[1-9][0-9]{0,18}", version) or not re.fullmatch(r"0|[1-9][0-9]{0,18}", floor)
            or int(version) > 2**64 - 1 or int(version) <= int(floor)
            or not re.fullmatch(r"(?:0|[1-9][0-9]{0,5})\.(?:0|[1-9][0-9]{0,5})\.(?:0|[1-9][0-9]{0,5})", display)):
        raise ValueError("explicit reviewed release version/display/floor required")
    first = json.loads((work / "source-before.json").read_bytes())
    after = source_map()
    if after != first["source"]:
        raise ValueError("source changed during actual build")
    dependency = dependency_frame(work, "after")
    if dependency != first["dependencySourceBefore"]:
        raise ValueError("compiled Native/protocol source changed during actual build")
    target = first["target"]
    collector = capture_collector(work, target, collector_binary)
    bind_collector_materials(work, collector, collector_materials_path)
    disk = disk_input(disk_input_path,target)
    supervisor_runtime=capture_supervisor_runtime(work,supervisor_runtime_pin)
    actual = artifacts(work / "cargo-build.jsonl", target)
    # Public copies, not hardlinks into a shared mutable Cargo target.
    for name, row in actual.items():
        path = work / name
        with Path(row["path"]).open("rb") as src, path.open("xb") as dst:
            shutil.copyfileobj(src, dst)
        path.chmod(0o700)
        if sha(path) != row["sha256"] or path.stat().st_nlink != 1:
            raise ValueError("normal binary immutable copy mismatch")
        row["path"] = str(path)
    descriptor = dict(format="cc-agent-release-build/v1", target=target, sourceBefore=first["source"], sourceAfter=after,
        sourceCommit=subprocess.check_output(["git", "-C", str(AGENT.parent), "rev-parse", "HEAD"], text=True).strip(),
        cargoLogSha256=sha(work / "cargo-build.jsonl"), binary=actual["cc-agent-identity"], signer=actual["cc-agent-release-sign"],
        rustc=subprocess.check_output(["rustc", "--version", "--verbose"], text=True).strip(),
        signingToolBundled=False, actualRuntimeVersion=None, collector=collector)
    import prototype_package
    import notice_generate
    _,runtime_stamp=prototype_package.stamp_runtime_version(
        notice_generate.read(AGENT/prototype_package.RUNTIME_VERSION_FILE,65536),display)
    descriptor["runtimeVersionStamp"]=runtime_stamp
    descriptor.update(dependencySourceBefore=first["dependencySourceBefore"], dependencySourceAfter=dependency)
    if supervisor_runtime is not None:descriptor["supervisorRuntime"]=supervisor_runtime
    if disk is not None:
        descriptor["prometheus"] = disk
    write(work / "build-public.json", descriptor)
    descriptor["descriptorSha256"] = sha(work / "build-public.json")
    import notice_generate
    cargo_home = Path(os.environ.get("CARGO_HOME", str(Path.home() / ".cargo")))
    roots = [p for p in (cargo_home / "registry/cache").glob("index.crates.io-*") if p.is_dir()]
    if len(roots) != 1:
        raise ValueError("one actual public Cargo archive cache required")
    metadata = json.loads((work / "dependency-metadata-after.json").read_bytes()) if release_dependency_sources.enabled(AGENT) else None
    notice_files = notice_generate.generate(AGENT, roots[0], descriptor, dependency_metadata=metadata)
    disk_notices(notice_files, disk,target)
    overlay = work / "package-delivery"
    overlay.mkdir(mode=0o700)
    for name, raw in notice_files.items():
        path = overlay / name
        path.parent.mkdir(parents=True, exist_ok=True)
        with path.open("xb") as stream:
            stream.write(raw)
    for name in ("launch.py", "consolecrypt-agent.service"):
        shutil.copyfile(AGENT / "delivery" / name, overlay / name)
    import prototype_package
    from stage import build_package
    from notice_inventory import validate_notice_inventory
    old = prototype_package.DIRECTORY
    try:
        prototype_package.DIRECTORY = overlay
        extra = {} if disk is None else {"disk_binary": Path(disk["path"]), "disk_report": Path(disk["asBuiltReportPath"])}
        inputs = prototype_package.explicit_files(work / "cc-agent-identity", None if supervisor_runtime is None else Path(supervisor_runtime["path"]), collector_binary=Path(collector["path"]), display_version=display, **extra)
        validation = validate_notice_inventory(inputs, required=True)
        if (hashlib.sha256(inputs.get("collector/vmagent-prod", b"")).hexdigest() != collector["sha256"]
                or len(inputs.get("collector/vmagent-prod", b"")) != collector["bytes"]):
            raise ValueError("packager did not retain original pinned collector bytes")
        executable = {"bin/cc-agent-identity", "collector/vmagent-prod"} | ({"collector/prometheus-hardmode"} if disk is not None else set())
        build_package(inputs, work / "candidate.zip", display, executable)
        candidate_budget(work / "candidate.zip")
    finally:
        prototype_package.DIRECTORY = old
    now = int(time.time())
    unsigned = dict(format=1, version=int(version), displayVersion=display, protocol=1,
        capabilities=dict(version=1, scopes=["telemetry.read", "metrics.collect", "exporters.manage", "agent.update"]),
        platform=TARGETS[target][1], artifactSha256=sha(work / "candidate.zip"), artifactBytes=(work / "candidate.zip").stat().st_size,
        issuedAt=now, expiresAt=now + 7 * 86400)
    write(work / "unsigned-release.json", unsigned)
    write(work / "package-public.json", dict(format="cc-agent-release-package/v1", buildDescriptorSha256=descriptor["descriptorSha256"],
        unsignedManifestSha256=sha(work / "unsigned-release.json"),
        inventorySha256=sha(overlay / "notices/inventory.json"), source=after, inputSha256={n:hashlib.sha256(v).hexdigest() for n,v in sorted(inputs.items())},
        artifactSha256=unsigned["artifactSha256"], artifactBytes=unsigned["artifactBytes"], noticeValidation=validation,
        collector=collector,runtimeVersionStamp=runtime_stamp, CI_candidate_only=True, production_distribution_ready=False, approvedFloor=int(floor)))
    recheck_supervisor_runtime(supervisor_runtime_pin,supervisor_runtime)
    if source_map() != after:
        raise ValueError("source changed during package capture")
    if dependency_frame(work, "package") != dependency:
        raise ValueError("compiled dependency source changed during package capture")


def self_verify(work, floor):
    descriptor = json.loads((work / "build-public.json").read_bytes())
    package_receipt = json.loads((work / "package-public.json").read_bytes())
    if sha(work / "unsigned-release.json") != package_receipt["unsignedManifestSha256"]:
        raise ValueError("original unsigned release changed")
    expected = json.loads((work / "unsigned-release.json").read_bytes())
    pin = json.loads((work / "signed/public-key.json").read_bytes())
    if (set(pin) != {"format", "publicKey"} or type(pin["format"]) is not int or pin["format"] != 1
            or pin["publicKey"] != os.environ.get("CC_AGENT_RELEASE_EXPECTED_PUBLIC_KEY", "")):
        raise ValueError("signed output no longer uses independently approved release pin")
    envelope = json.loads((work / "signed/release-manifest.json").read_bytes())
    if set(envelope) != {"body", "signature"} or envelope["body"] != expected:
        raise ValueError("signed typed body differs from original package release")
    for name, key in (("cc-agent-identity", "binary"), ("cc-agent-release-sign", "signer")):
        if sha(work / name) != descriptor[key]["sha256"]:
            raise ValueError("compiled tool changed")
    command = [str(work / "cc-agent-identity"), "verify-release", "--public-key", str(work / "signed/public-key.json"),
        "--manifest", str(work / "signed/release-manifest.json"), "--artifact", str(work / "candidate.zip"),
        "--platform", TARGETS[descriptor["target"]][1], "--protocol", "1", "--current-version", floor]
    result = subprocess.run(command, stdin=subprocess.DEVNULL, capture_output=True, timeout=10, check=True)
    verified = json.loads(result.stdout)
    if verified != {k: expected[k] for k in ("version", "displayVersion", "platform", "protocol", "artifactSha256", "artifactBytes")}:
        raise ValueError("actual Rust verified release does not match original inputs")
    key = work / "signed/public-key.json"
    write(work / "trust.example.json", dict(version=1, verifierSha256=descriptor["binary"]["sha256"], publicKeySha256=sha(key), platform=expected["platform"]))
    write(work / "verified-public.json", dict(format="cc-agent-release-self-verification/v1", verified=verified,
        verifierSha256=descriptor["binary"]["sha256"], publicKeySha256=sha(key), productionTrustEstablished=False,
        bootstrapRequiresIndependentOperatorFingerprintApproval=True))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("operation", choices=["before", "package", "self-verify"])
    parser.add_argument("--work", type=Path, required=True)
    parser.add_argument("--target", choices=list(TARGETS))
    parser.add_argument("--vmagent-binary", type=Path, help="explicit digest-pinned vmagent-prod for the SAME fixed Linux target; package only")
    parser.add_argument("--collector-materials", type=Path, help="actual fixed collector_materials.py output, required by package")
    parser.add_argument("--disk-input", type=Path, help="optional closed actual normal Prometheus descriptor, including full verified materials; no ambient artifact discovery")
    parser.add_argument("--supervisor-runtime-pin", type=Path, help="optional explicit reviewed target-Host system Python3.11/3.12 public pin; absent denies V2, never measured or synthesized here")
    args = parser.parse_args()
    if args.operation == "before":
        before(args.work, args.target)
    elif args.operation == "package":
        package(args.work, os.environ.get("CC_RELEASE_VERSION", ""), os.environ.get("CC_RELEASE_DISPLAY_VERSION", ""), os.environ.get("CC_AGENT_RELEASE_APPROVED_FLOOR", ""), args.vmagent_binary, args.collector_materials, args.disk_input, args.supervisor_runtime_pin)
    else:
        self_verify(args.work, os.environ.get("CC_AGENT_RELEASE_APPROVED_FLOOR", ""))


if __name__ == "__main__":
    main()
