"""Current-build notice generator. Local pinned archives only; no signing/network.
Conservative Cargo resolution is not an exact native/stdlib linkage SBOM.
"""
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import stat
import sys
import tarfile
import tomllib
import zipfile
import release_platforms

def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def encoded(value):
    return (json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True) + "\n").encode()


def module_notice_provenance(full):
    """Keep the complete observation on the same-binary vmagent primary."""
    if "buildInfoObservationRef" in full:
        raise ValueError("original complete source provenance required")
    child=dict(full)
    if "buildInfoObservation" in child:
        observation=child.pop("buildInfoObservation")
        if type(observation)is not dict:
            raise ValueError("complete build information observation required")
        child["buildInfoObservationRef"]=dict(component="vmagent",sha256=digest(encoded(observation)))
    return child


def notice_budget_diagnostic(components, required_bytes, has_disk):
    # Counts only: never source names, paths, text, credentials or their hashes.
    kinds={}
    allowed={"rust-local-path-dependency","rust-pinned-git-dependency","rust-lock-dependency",
        "patched-vendored-rust-dependency","bundled-native-library","vendor-source-only-not-bundled-binary",
        "bundled-python-wheel","bundled-agent-binary","bundled-collector-binary","bundled-go-module",
        "bundled-go-stdlib","bundled-agent-source","source-overlay-not-bundled-binary"}
    for component in components:
        kind=component.get("kind")
        if type(kind)is not str or kind not in allowed:kind="other"
        row=kinds.setdefault(kind,dict(components=0,componentJsonBytes=0,noticeReferences=0))
        row["components"]+=1;row["componentJsonBytes"]+=len(encoded(component))
        row["noticeReferences"]+=len(component.get("notices",[]))
    value=dict(version=1,requiredBytes=required_bytes,limitBytes=256*1024,
        components=len(components),diskCollectorPresent=int(has_disk),kinds=kinds)
    raw=encoded(value)
    if len(raw)>16384:return value
    try:print("CC_AGENT_NOTICE_INVENTORY_BUDGET "+raw.decode().strip(),file=sys.stderr,flush=True)
    except(OSError,ValueError):pass
    if os.environ.get("GITHUB_ACTIONS")!="true":return value
    temp=Path(os.environ.get("RUNNER_TEMP",""))
    try:
        if not temp.is_absolute()or temp.resolve(strict=True)!=temp or not temp.is_dir():return value
        path=temp/"cc-agent-notice-inventory-budget.json"
        fd=os.open(path,os.O_WRONLY|os.O_CREAT|os.O_EXCL|getattr(os,"O_NOFOLLOW",0)|getattr(os,"O_CLOEXEC",0),0o600)
        try:
            info=os.fstat(fd)
            if not stat.S_ISREG(info.st_mode)or info.st_nlink!=1:return value
            with os.fdopen(fd,"wb",closefd=False)as stream:stream.write(raw);stream.flush();os.fsync(fd)
        finally:os.close(fd)
    except(OSError,ValueError):pass
    return value


def read(path, limit=32 * 1024 * 1024):
    path = Path(path)
    for parent in (path, *path.parents):
        m = parent.lstat()
        if stat.S_ISLNK(m.st_mode) or getattr(m, "st_file_attributes", 0) & 0x400:
            raise ValueError("input link: " + str(path))
    before = path.stat()
    if not stat.S_ISREG(before.st_mode) or not 0 <= before.st_size <= limit:
        raise ValueError("input type/size: " + str(path))
    with path.open("rb") as stream:
        raw = stream.read(limit + 1)
    after = path.stat()
    fields = lambda s: (s.st_dev, s.st_ino, s.st_size, s.st_mtime_ns, s.st_ctime_ns)
    if len(raw) > limit or fields(before) != fields(after):
        raise ValueError("input changed: " + str(path))
    return raw


def chosen(expression):
    if expression == "(MIT OR Apache-2.0) AND Unicode-3.0":
        return "MIT AND Unicode-3.0"
    if expression in {"MIT", "MIT OR Apache-2.0", "Apache-2.0 OR MIT", "MIT/Apache-2.0",
                      "MIT OR Apache-2.0 OR BSD-1-Clause", "Unlicense OR MIT",
                      "MIT OR Apache-2.0 OR LGPL-2.1-or-later",
                      "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT", "Zlib OR Apache-2.0 OR MIT"}:
        return "MIT"
    if expression in {"BSD-3-Clause", "Apache-2.0", "CC0-1.0", "Unicode-3.0"}:
        return expression
    raise ValueError("unreviewed license expression: " + str(expression))


def shared_cc0_notice(name,version,cache,source,notice):
    # This exact published crate declares CC0 but omits the document. Preserve
    # the complete authentic CC0 text from the pinned, also bundled bip39 crate;
    # do not claim the missing file existed inside bitcoin_hashes' archive.
    if(name,version)!=("bitcoin_hashes","0.14.101"):
        raise ValueError("no reviewed shared CC0 notice for this crate")
    raw=source(cache/"bip39-3.0.0.crate")
    archive_sha="4559bb9225f8401dcea601cf8dc9ed81ea0dadd9cf7755fdefdd93ef7961e19e"
    if digest(raw)!=archive_sha:raise ValueError("shared CC0 original archive changed")
    import io
    with tarfile.open(fileobj=io.BytesIO(raw),mode="r:gz")as archive:
        member=archive.getmember("bip39-3.0.0/LICENSE")
        if not member.isfile()or member.size!=7049:raise ValueError("shared full CC0 document required")
        body=archive.extractfile(member).read(7050)
    text_sha="7179683e8000e6bdc9bbc60d85edf0a4ac8e76f951857f54fcb775d5886f1309"
    if len(body)!=7049 or digest(body)!=text_sha:raise ValueError("shared full CC0 text changed")
    return notice("bip39-3.0.0/LICENSE (shared complete CC0-1.0 text; bitcoin_hashes archive omits document)",body),dict(
        archive="bip39-3.0.0.crate",archive_sha256=archive_sha,member="bip39-3.0.0/LICENSE",text_sha256=text_sha,
        reason="exact bitcoin_hashes-0.14.101 declares CC0-1.0 but ships no license document")


def generate(agent, cache, descriptor, *, dependency_metadata=None):
    agent, cache = Path(agent).resolve(), Path(cache).resolve()
    sys.path.insert(0, str(agent / "delivery"))
    import prototype_package as package
    from package_wheel import wheel_file, PACKAGE, SHA256
    from notice_inventory import INVENTORY, validate_notice_inventory
    observed = {}
    def source(path, limit=32 * 1024 * 1024):
        raw = read(path, limit)
        key, current = str(Path(path).resolve()), digest(raw)
        if key in observed and observed[key][0] != current:
            raise ValueError("notice input changed between reads")
        observed[key] = (current, max(limit, observed.get(key, (None, 0))[1]))
        return raw
    if descriptor["format"] != "cc-agent-release-build/v1" or descriptor["sourceBefore"] != descriptor["sourceAfter"]:
        raise ValueError("actual current build/source descriptor required")
    for name, expected in descriptor["sourceAfter"].items():
        if digest(source(agent / name)) != expected:
            raise ValueError("actual build source changed: " + name)
    body = source(Path(descriptor["binary"]["path"]))
    if (digest(body) != descriptor["binary"]["sha256"] or len(body) != descriptor["binary"]["bytes"]
            or body[:6] != b"\x7fELF\x02\x01"):
        raise ValueError("actual normal ELF descriptor mismatch")
    machine = int.from_bytes(body[18:20], "little")
    if machine != {"x86_64-unknown-linux-gnu":62,"aarch64-unknown-linux-gnu":183}[descriptor["target"]]:
        raise ValueError("actual ELF target mismatch")
    files, components = {}, []
    def notice(name, raw):
        if not raw or len(raw)>128*1024: raise ValueError("full notice budget")
        h=digest(raw); p="notices/text/"+h+".txt"; files[p]=raw
        return dict(original_path=name,package_path=p,sha256=h)
    project_notice=notice("LICENSE",source(agent/"LICENSE",128*1024))
    lock_raw=source(agent/"identity/Cargo.lock",1024*1024)
    locked = tomllib.loads(lock_raw.decode("utf-8"))["package"]
    import release_dependency_sources
    dependency_components = release_dependency_sources.notice_components(
        agent, descriptor, dependency_metadata, locked, source, notice)
    components.extend(dependency_components)
    supported_git = {(row['name'], row['version'], row['source']) for row in dependency_components
                     if row['source'] is not None}
    for item in locked:
        if "source" not in item:
            continue
        name, version = item["name"], item["version"]
        if (name, version, item['source']) in supported_git:
            continue
        if (not re.fullmatch(r"[a-zA-Z0-9_-]+", name)
                or not re.fullmatch(r"[a-zA-Z0-9_.+-]+", version)
                or item["source"] != "registry+https://github.com/rust-lang/crates.io-index"):
            raise ValueError("unreviewed package source")
        archive = cache / (name + "-" + version + ".crate")
        raw = source(archive)
        if digest(raw) != item["checksum"]:
            raise ValueError("crate archive checksum mismatch: " + name)
        # Re-opened input must match the just-observed checksum; no extraction.
        import io
        with tarfile.open(fileobj=io.BytesIO(raw), mode="r:gz") as tar:
            root = name + "-" + version + "/"
            members = tar.getmembers()
            if len(members) > 10000 or len({m.name for m in members}) != len(members):
                raise ValueError("archive member budget/duplicates")
            def member_bytes(member, limit=128 * 1024):
                if not member.isfile() or not 0 < member.size <= limit:
                    raise ValueError("archive member type/size")
                return tar.extractfile(member).read(limit + 1)
            meta = tomllib.loads(member_bytes(tar.getmember(root + "Cargo.toml")).decode("utf-8"))["package"]
            if (meta["name"], meta["version"]) != (name, version):
                raise ValueError("crate metadata identity mismatch")
            selection = chosen(meta.get("license"))
            notices = []
            for member in members:
                if not member.name.startswith(root):
                    raise ValueError("crate member outside root")
                relative = member.name[len(root):]
                path = PurePosixPath(relative)
                if path.is_absolute() or ".." in path.parts:
                    raise ValueError("unsafe crate member")
                leaf = path.name.lower()
                if not member.isfile() or len(path.parts) > 3:
                    continue
                ancillary = leaf in {"copyright", "authors", "copying", "notice", "notice.txt"}
                selected = ("mit" in leaf if selection.startswith("MIT") else "license" in leaf)
                selected |= meta["license"] == "MIT" and "license" in leaf
                selected |= "Unicode-3.0" in selection and "unicode" in leaf
                if not selected and selection=="MIT" and "license"in leaf:
                    combined=member_bytes(member)
                    selected=all(marker in combined for marker in(b"Permission is hereby granted",b"THE SOFTWARE IS PROVIDED"))
                if ancillary or selected:
                    notices.append(notice(root + relative, member_bytes(member)))
            shared=None
            if not notices and selection=="CC0-1.0":
                entry,shared=shared_cc0_notice(name,version,cache,source,notice);notices.append(entry)
            if not notices:
                raise ValueError("selected full notice missing: " + name)
            # r-efi's complete MIT text is in AUTHORS, not a LICENSE filename.
            if name == "r-efi":
                authors = member_bytes(tar.getmember(root + "AUTHORS"))
                if not all(x in authors for x in (b"AUTHORS-MIT:", b"Permission is hereby granted", b"THE SOFTWARE IS PROVIDED")):
                    raise ValueError("r-efi complete MIT notice missing")
        components.append(dict(name=name, version=version, kind="rust-lock-dependency",
            source=item["source"], crate_archive_sha256=item["checksum"], cached_archive_rehashed=True,
            scope="lock-resolution-superset-not-ELF-linkage-proof", license_declared=meta["license"],
            selected_license=selection, notices=notices,**({"shared_license_text":shared}if shared is not None else{})))

    provenance = json.loads(source(agent / "third_party/sqlite/SOURCE.json", 128 * 1024))
    for name, expected in provenance["files_sha256"].items():
        if digest(source(agent / name)) != expected:
            raise ValueError("patched SQLite source inventory mismatch: " + name)
    vendor = agent / "identity/vendor/libsqlite3-sys"
    components.append(dict(name="libsqlite3-sys", version="0.38.2", kind="patched-vendored-rust-dependency",
        license_declared="MIT", selected_license="MIT", native_source_digest=provenance["native_source_digest"],
        provenance_sha256=digest(source(agent / "third_party/sqlite/SOURCE.json")),
        notices=[notice("identity/vendor/libsqlite3-sys/LICENSE", source(vendor / "LICENSE"))]))
    sqlite = source(vendor / "sqlite3/sqlite3.c")
    position = sqlite.index(b"** The author disclaims copyright to this source code.")
    blessing = sqlite[sqlite.rfind(b"/*", 0, position):sqlite.index(b"*/", position) + 2] + b"\n"
    if b"May you share freely" not in blessing or b"** version 3.53.2." not in sqlite[:2048]:
        raise ValueError("SQLite version/full blessing missing")
    components.append(dict(name="SQLite", version="3.53.2", kind="bundled-native-library",
        source_sha256=digest(sqlite), license_declared="upstream public-domain dedication and blessing",
        build_feature_basis="identity/Cargo.toml bundled plain SQLite; no SQLCipher binary claim",
        notices=[notice("identity/vendor/libsqlite3-sys/sqlite3/sqlite3.c:complete blessing comment", blessing)]))
    components.append(dict(name="SQLCipher", kind="vendor-source-only-not-bundled-binary",
        notices=[notice("identity/vendor/libsqlite3-sys/sqlcipher/LICENSE", source(vendor / "sqlcipher/LICENSE"))]))
    wheels = wheel_file(agent)
    if set(wheels) != {PACKAGE} or digest(wheels[PACKAGE]) != SHA256:
        raise ValueError("maintained wheel binding mismatch")
    # Observe the original wheel and provenance in the final source map too.
    wheel_provenance = json.loads(source(agent / "third_party/websockets/SOURCE.json"))
    if digest(source(agent / "third_party/websockets/websockets-17.4-py3-none-any.whl")) != SHA256:
        raise ValueError("original wheel mismatch")
    wheel_license = source(agent / "third_party/websockets/LICENSE")
    import io
    with zipfile.ZipFile(io.BytesIO(wheels[PACKAGE])) as wheel:
        if wheel.read("websockets-17.4.dist-info/licenses/LICENSE") != wheel_license:
            raise ValueError("wheel/full license mismatch")
    components.append(dict(name="websockets", version="17.4", kind="bundled-python-wheel",
        wheel_sha256=SHA256, package_path=PACKAGE, license_declared="BSD-3-Clause",
        selected_license="BSD-3-Clause", source_provenance=wheel_provenance,
        notices=[notice("third_party/websockets/LICENSE", wheel_license)]))
    components.append(dict(name="cc-agent-identity-policy",version="0.1.0",kind="bundled-agent-binary",
        binary_sha256=digest(body),binary_bytes=len(body),target=descriptor["target"],
        build_descriptor_sha256=descriptor["descriptorSha256"],license_declared="AGPL-3.0-only",notices=[project_notice]))
    selected = release_platforms.select(descriptor["target"], "target")
    collector = descriptor["collector"]
    collector_raw = source(Path(collector["path"]), 80 * 1024 * 1024)
    if (collector["platform"] != selected["collector"] or collector["version"] != "v1.156.0"
            or collector["packagePath"] != "collector/vmagent-prod"
            or digest(collector_raw) != collector["sha256"] or len(collector_raw) != collector["bytes"]
            or collector["supplyLockSha256"] != digest(source(agent / "collector/supply-lock.json"))):
        raise ValueError("original collector descriptor mismatch")
    release_platforms.collector_payload(collector_raw, selected)
    collector_license = source(agent / "delivery/licenses/VMAGENT-LICENSE.txt", 32 * 1024)
    if digest(collector_license) != "778f2412aa6e874bf0e2482e17a119ea53ba508ae6ddd924859a1f32f6e2740f":
        raise ValueError("full pinned VictoriaMetrics license changed")
    components.append(dict(name="vmagent", version="v1.156.0", kind="bundled-collector-binary",
        platform=selected["collector"], package_path="collector/vmagent-prod", binary_sha256=digest(collector_raw),
        binary_bytes=len(collector_raw), license_declared="Apache-2.0", selected_license="Apache-2.0",
        source="https://github.com/VictoriaMetrics/VictoriaMetrics/tree/v1.156.0",
        notices=[notice("VictoriaMetrics/v1.156.0/LICENSE", collector_license)]))
    import collector_materials
    material_raw = source(Path(collector["materialsPath"]), collector_materials.MAX_JSON)
    if digest(material_raw) != collector["materialsSha256"]:
        raise ValueError("original full Go materials changed")
    material_value = collector_materials.checked(material_raw, collector["sha256"])
    release_platforms.material_identity(material_value, selected)
    for row in components:
        if row.get("kind") == "bundled-collector-binary":
            row["source_notice_provenance"] = material_value["sourceNoticeProvenance"]
    if material_value["buildInfoSha256"] != collector["buildInfoSha256"]:
        raise ValueError("actual collector buildinfo changed")
    # Each child already binds this same binary and source record. Its compact
    # reference resolves to the full observation on the existing vmagent row;
    # the independently verified signed materials record stays untouched.
    child_provenance=module_notice_provenance(material_value["sourceNoticeProvenance"])
    # Combine complete texts into bounded notice files; never trim a license to
    # fit the package count. Each original material retains its own SHA below.
    groups = [(row["path"] + "@" + row["version"], row, "bundled-go-module")
              for row in material_value["modules"]]
    groups.append((material_value["stdlib"]["version"], material_value["stdlib"], "bundled-go-stdlib"))
    combined = []
    chunk = b""
    for name, row, kind in groups:
        for item in row["materials"]:
            section = ("=== " + name + "/" + item["path"] + " ===\n").encode() + item["text"].encode() + b"\n"
            if len(section) > 128 * 1024:
                raise ValueError("full material plus attribution exceeds existing notice budget")
            if len(chunk) + len(section) > 128 * 1024:
                combined.append(notice("actual vmagent Go materials: full text bundle", chunk))
                chunk = b""
            chunk += section
    if chunk:
        combined.append(notice("actual vmagent Go materials: full text bundle", chunk))
    for name, row, kind in groups:
        components.append(dict(name=name, kind=kind, archive_sha256=row["archiveSha256"],
            actual_binary_sha256=collector["sha256"], build_info_sha256=collector["buildInfoSha256"],
            material_source_sha256={item["path"]: item["sha256"] for item in row["materials"]},
            notices=combined, source_notice_provenance=child_provenance, license_compatibility_certified=False))
    if "prometheus" in descriptor:
        import prometheus_materials
        components.append(prometheus_materials.component(agent, descriptor["prometheus"], source, notice))
    explicit={name:source(agent/name) for name in package.SOURCES}
    explicit["launch.py"]=source(agent/"delivery/launch.py")
    explicit["systemd/consolecrypt-agent.service"]=source(agent/"delivery/consolecrypt-agent.service")
    runtime=descriptor.get("supervisorRuntime")
    if runtime is not None:
        if type(runtime)is not dict or set(runtime)!={"format","path","sha256","bytes","pythonExecutable","pythonSha256"}:
            raise ValueError("closed reviewed supervisor runtime descriptor required")
        raw=package.public_runtime_pin(runtime["path"])
        value=json.loads(raw)
        if (runtime["format"]!=value["format"] or runtime["pythonExecutable"]!=value["pythonExecutable"]
                or runtime["pythonSha256"]!=value["pythonSha256"] or runtime["sha256"]!=digest(raw)
                or type(runtime["bytes"])is not int or runtime["bytes"]!=len(raw)):
            raise ValueError("reviewed supervisor runtime descriptor mismatch")
        explicit["python/supervisor-runtime.json"]=raw
    stamp=descriptor.get("runtimeVersionStamp")
    if stamp is not None:
        from prototype_package import stamp_runtime_version,RUNTIME_VERSION_FILE
        if type(stamp)is not dict:raise ValueError("closed runtime version stamp required")
        packaged,expected_stamp=stamp_runtime_version(explicit[RUNTIME_VERSION_FILE],stamp.get("runtimeVersion"))
        if stamp!=expected_stamp:raise ValueError("runtime version original/generation mismatch")
        explicit[RUNTIME_VERSION_FILE]=packaged
    components.append(dict(name="ConsoleCrypt Agent source",kind="bundled-agent-source",license_declared="AGPL-3.0-only",
        notices=[project_notice],source_file_sha256={n:digest(b) for n,b in sorted(explicit.items())},
        **({"runtime_version_stamp":stamp}if stamp is not None else{})))
    components.append(dict(name="Prometheus agent overlay",version="3.16.0",kind="source-overlay-not-bundled-binary",
        provenance_sha256=digest(source(agent/"third_party/prometheus-agent/SOURCE.json")),license_declared="Apache-2.0",
        notices=[notice("third_party/prometheus-agent/upstream/"+n,source(agent/"third_party/prometheus-agent/upstream"/n)) for n in ("LICENSE","NOTICE")]))
    inventory=dict(format="cc-agent-notice-inventory/v1",cargo_lock_sha256=digest(lock_raw),binary_sha256=digest(body),
        build_descriptor_sha256=descriptor["descriptorSha256"],components=components,legal_certification=False,
        production_distribution_ready=False,scope="exact current build/package inputs; Cargo.lock resolution superset, not exact ELF linkage/SBOM certification",
        issues=["Final Rust standard-library/native linkage notice review and corresponding-source distribution review required",
                "Go module/standard-library full materials are collected from actual buildinfo and verified sources; final legal compatibility/corresponding-source review remains required"],
        external_not_bundled=["system CPython","optional external exporter executables and dependencies"])
    files[INVENTORY]=encoded(inventory)
    if len(files[INVENTORY])>256*1024:
        try:notice_budget_diagnostic(components,len(files[INVENTORY]),"prometheus"in descriptor)
        except Exception:pass # Optional diagnostic cannot replace the original denial.
    validate_notice_inventory(files,required=True)
    if len(explicit)+len(files)+2>512:raise ValueError("existing package count exceeded")
    for p,(h,limit) in observed.items():
        if digest(read(p,limit))!=h:raise ValueError("notice input changed")
    return files
