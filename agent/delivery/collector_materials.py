"""Explicit public vmagent Go-notice collection, never execute the input ELF.

Uses the official local Go reader/downloader with a fresh private module cache,
public proxy+sumdb, and no VCS/hooks/toolchain auto-download. Missing exact build
versions/sums, local replacements or missing license material deny packaging.
This is material collection/integrity, not a legal compatibility certification.
"""
import argparse
import base64
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import stat
import subprocess
import tarfile
import time
import urllib.parse
import urllib.request
import zipfile
import release_platforms

from notice_generate import read

PIN = "2039bc05b6ad5c96b34b72801908896bf8a6a7bcab8ae32120cbb8e6217ef6f6"
MAX_JSON = 16 * 1024 * 1024
# Actual official listing/archive capture for the pinned2039 binary's GoVersion.
# This is reviewed public source provenance, never a caller-asserted checksum.
STDLIB_SOURCE_PIN = ("go1.27.3", "6e7749035cf360f06b2575d162ea79df9fda8ef671889e5c375f8e50777f26bb", 35134035)
MATERIAL = re.compile(r"(?:licen[cs]e|notice|copying|authors|contributors|copyright|patents)(?:[._-].*)?", re.I)


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def unique(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate public material field")
        result[key] = value
    return result


def decode(raw):
    if len(raw) > MAX_JSON:
        raise ValueError("collector material JSON budget")
    return json.loads(raw, object_pairs_hook=unique)


def encoded(value):
    return (json.dumps(value, sort_keys=True, ensure_ascii=True, indent=2) + "\n").encode()


def write(path, raw):
    with path.open("xb") as stream:
        stream.write(raw)


def module_identity(value):
    if type(value) is not dict or value.get("Replace") is not None:
        raise ValueError("local/replaced/unresolved Go module denied")
    path, version, checksum = (value.get(name) for name in ("Path", "Version", "Sum"))
    if (type(path) is not str or len(path) > 240
            or re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._~/-]*", path) is None
            or any(piece in {"", ".", ".."} for piece in path.split("/"))
            or "." not in path.split("/")[0]
            or type(version) is not str or re.fullmatch(r"v[0-9]+\.[0-9]+\.[0-9]+(?:-[A-Za-z0-9.-]+)?(?:\+incompatible)?", version) is None
            or type(checksum) is not str or not checksum.startswith("h1:")):
        raise ValueError("exact Go module path/version/sum unavailable")
    try:
        value = base64.b64decode(checksum[3:], validate=True)
    except ValueError:
        raise ValueError("invalid Go module sum") from None
    if len(value) != 32 or base64.b64encode(value).decode() != checksum[3:]:
        raise ValueError("invalid Go module sum")
    return dict(path=path, version=version, sum=checksum)


def official_source(platform="linux-amd64"):
    selected = release_platforms.select(platform, "collector")
    root = Path(__file__).resolve().parents[1] / "collector/notices/v1.156.0"
    leaf = 'ARM64_SOURCE.json' if selected['collector']=='linux-arm64' else 'SOURCE.json'
    raw = read(root / leaf, 128 * 1024)
    if selected['collector']=='linux-arm64' and digest(raw)!='58761ad78c69f9eb2ada9ce63cefaacc50fc34f62f081668e87a639fe48bf100':
        raise ValueError('reviewed ARM parser/source notice record changed')
    value = decode(raw)
    mod = read(root / "go.mod", 1024 * 1024)
    sums = read(root / "go.sum", 1024 * 1024)
    if (value.get("format") != "cc-vmagent-official-source-notices/v1"
            or value.get("binarySha256") != selected['binarySha256'] or value.get("upstreamTag") != "v1.156.0"
            or not 64 <= value.get("binaryBytes") <= release_platforms.MAX_COLLECTOR or value.get("observedCommit") != "0c1bf87a0c9843c30566df0c0aa6593466425ac5"
            or value.get("binaryVcsCommit") is not None or value.get("sourceBuildCorrespondence") is not False
            or value.get("authenticatedSignerClaimed") is not False
            or digest(mod) != value.get("goModSha256") or digest(sums) != value.get("goSumSha256")
            or digest(mod) != "73dcd4cb47c36008138cbf4f7bd5e7aaec00ceeac55c2f80f359c76a8d330b81"
            or digest(sums) != "0dc0008e40f5823daa757d2a9dc85c8ae1edf233ad80de6ed56522c564d84135"
            or not mod.startswith(b"module github.com/VictoriaMetrics/VictoriaMetrics\n")):
        raise ValueError("reviewed official-source notice record changed")
    value = dict(value, _recordSha256=digest(raw), _mod=mod.decode(), _sums=sums.decode())
    return value


def source_provenance(source):
    value = dict(sourceRecordSha256=source["_recordSha256"], declaredUpstreamTag="v1.156.0",
        officialSourceCommit=source["observedCommit"], binaryVcsCommit=None,
        actualMainVersion="(devel)", moduleSumBasis="exact official-tag go.sum when absent from binary",
        sourceBuildCorrespondence=False, authenticatedSignerClaimed=False)
    if 'buildInfoObservation' in source:
        value['buildInfoObservation']=source['buildInfoObservation']
    return value


def build_modules(raw, source=None):
    value = decode(raw)
    if (type(value) is not dict or type(value.get("GoVersion")) is not str
            or re.fullmatch(r"go1\.[0-9]+\.[0-9]+", value["GoVersion"]) is None
            or type(value.get("Deps")) is not list or not 1 <= len(value["Deps"]) <= 128):
        raise ValueError("complete release Go build information unavailable")
    if source is None:
        main = module_identity(value.get("Main"))
        if main["path"] != "github.com/VictoriaMetrics/VictoriaMetrics" or main["version"] != "v1.156.0":
            raise ValueError("actual main-module release is unresolved or foreign")
        modules = [main] + [module_identity(row) for row in value["Deps"]]
    else:
        # This exact observed official binary omits VCS data and every h1 sum.
        # Never manufacture embedded facts: this separate pinned source record
        # supplies notice-only provenance. Its actual commit is NOT a binary VCS
        # commit, and sourceBuildCorrespondence remains false throughout.
        if value != source["observedBuildInfo"]:
            raise ValueError("actual pinned binary buildinfo differs from reviewed facts")
        declared = {}
        for line in source["_sums"].splitlines():
            parts = line.split()
            if len(parts) != 3:
                raise ValueError("invalid pinned Go checksum row")
            key = (parts[0], parts[1])
            if key in declared:
                raise ValueError("duplicate pinned Go checksum row")
            declared[key] = parts[2]
        modules = []
        for row in value["Deps"]:
            if type(row) is not dict or row.get("Replace") is not None:
                raise ValueError("local/replaced Go dependency denied")
            path, version = row.get("Path"), row.get("Version")
            checksum = declared.get((path, version))
            if (checksum is None or row.get("Sum") not in {None, "", checksum}
                    or re.search(r"(?m)^\s*" + re.escape(path) + r"\s+" + re.escape(version) + r"(?:\s|$)", source["_mod"]) is None):
                raise ValueError("exact actual dependency missing from pinned source mod/sum")
            modules.append(module_identity(dict(Path=path, Version=version, Sum=checksum)))
    if len({row["path"] for row in modules}) != len(modules):
        raise ValueError("duplicate Go module identity")
    return value["GoVersion"], modules


def safe_relative(name):
    if type(name) is not str:
        return False
    p = PurePosixPath(name)
    return (isinstance(name, str) and 0 < len(name) <= 512 and not p.is_absolute()
            and str(p) == name and ".." not in p.parts and "\\" not in name and ":" not in name)


def material(name, raw):
    if not safe_relative(name) or not 0 <= len(raw) <= 128 * 1024:
        raise ValueError("full notice material path/size budget")
    # Empty upstream files, including nested testdata/LICENSE, are source facts.
    # Component coverage is checked separately; never drop or trim these bytes.
    return dict(path=name, sha256=digest(raw), text=raw.decode("utf-8"))


def zip_materials(raw, path, version):
    prefix = path + "@" + version + "/"
    result = []
    with zipfile.ZipFile(io.BytesIO(raw)) as archive:
        members = archive.infolist()
        if len(members) > 100000 or len({row.filename for row in members}) != len(members):
            raise ValueError("module ZIP member budget/duplicates")
        for row in members:
            if not row.filename.startswith(prefix):
                raise ValueError("module ZIP outside exact module root")
            relative = row.filename[len(prefix):]
            if row.is_dir():
                continue
            mode = row.external_attr >> 16
            if not safe_relative(relative) or stat.S_IFMT(mode) not in {0, stat.S_IFREG}:
                raise ValueError("unsafe module ZIP member")
            if MATERIAL.fullmatch(PurePosixPath(relative).name):
                if row.file_size > 128 * 1024:
                    raise ValueError("full module notice exceeds existing notice budget")
                result.append(material(relative, archive.read(row)))
    if not result or not any(x["text"].strip() and re.match(r"(?:licen[cs]e|copying)(?:[._-]|$)", PurePosixPath(x["path"]).name, re.I) for x in result):
        # Public archive identity only, never material content. Keep diagnostics
        # bounded even for an unhelpful upstream archive with many notices.
        details = dict(module=path, version=version, materialCount=len(result), materials=[
            dict(path=row["path"], bytes=len(row["text"].encode("utf-8")), sha256=row["sha256"])
            for row in result[:32]])
        raise ValueError("full nonempty module license material missing: " + json.dumps(details, sort_keys=True))
    return sorted(result, key=lambda row: row["path"])


def stdlib_materials(raw):
    result = []
    with tarfile.open(fileobj=io.BytesIO(raw), mode="r:gz") as archive:
        members = archive.getmembers()
        if len(members) > 100000 or len({row.name for row in members}) != len(members):
            raise ValueError("stdlib archive member budget/duplicates")
        for row in members:
            # tarfile normalizes the official top-level directory go/ to go.
            # Accept ONLY that directory entry, never a file/link at the root.
            if row.name == "go" and row.isdir():
                continue
            if not row.name.startswith("go/"):
                kind = ("directory" if row.isdir() else "regular" if row.isfile()
                        else "symlink" if row.issym() else "hardlink" if row.islnk() else "other")
                raise ValueError("stdlib source root mismatch: " + json.dumps(
                    dict(path=row.name[:512], type=kind, bytes=row.size), sort_keys=True))
            relative = row.name[3:]
            if row.isdir():
                continue
            if not safe_relative(relative) or not row.isfile():
                raise ValueError("unsafe stdlib source member")
            if MATERIAL.fullmatch(PurePosixPath(relative).name):
                if row.size > 128 * 1024:
                    raise ValueError("full stdlib notice exceeds existing notice budget")
                result.append(material(relative, archive.extractfile(row).read(128 * 1024 + 1)))
    check_stdlib_coverage(result)
    return sorted(result, key=lambda row: row["path"])


def check_stdlib_coverage(rows):
    # Actual official go1.27.3 source has root LICENSE/PATENTS. Legacy author
    # lists are absent upstream, not fabricated requirements. All found files
    # (including any AUTHORS/CONTRIBUTORS in other source layouts) are retained.
    required = {"LICENSE", "PATENTS"}
    roots = {row["path"]: row for row in rows if "/" not in row["path"]}
    missing = sorted(required - roots.keys())
    empty = sorted(name for name in required & roots.keys() if not roots[name]["text"].strip())
    if missing or empty:
        raise ValueError("matching full Go source license material missing: " + json.dumps(
            dict(found=sorted(roots), missing=missing, empty=empty), sort_keys=True))


def explicit_stdlib_source(go_version, source_input):
    path = Path(source_input)
    if not path.is_absolute():
        raise ValueError("explicit absolute official stdlib source record required")
    value = decode(read(path, 1024 * 1024))
    filename = go_version + ".src.tar.gz"
    if (go_version, value.get("archiveSha256"), value.get("archiveBytes")) != STDLIB_SOURCE_PIN:
        raise ValueError("reviewed official stdlib source pin required")
    if (value.get("format") != "cc-official-go-source-input/v1" or value.get("version") != go_version
            or value.get("archiveFile") != filename or value.get("archiveUrl") != "https://go.dev/dl/" + filename
            or value.get("archiveFinalUrl") not in {"https://go.dev/dl/" + filename, "https://dl.google.com/go/" + filename}
            or value.get("listingUrl") != "https://go.dev/dl/?mode=json&include=all"
            or type(value.get("listingSha256")) is not str or re.fullmatch(r"[0-9a-f]{64}", value["listingSha256"]) is None
            or type(value.get("archiveSha256")) is not str or re.fullmatch(r"[0-9a-f]{64}", value["archiveSha256"]) is None
            or type(value.get("archiveBytes")) is not int or not 0 < value["archiveBytes"] <= 64 * 1024 * 1024
            or value.get("binaryExecuted") is not False or value.get("legalCertification") is not False):
        raise ValueError("official stdlib record/version/hash binding mismatch")
    # Adjacent exact version-derived filename; never resolve an input-selected
    # arbitrary archive path or discover an ambient source cache.
    raw = read(path.parent / filename, 64 * 1024 * 1024)
    if len(raw) != value["archiveBytes"] or digest(raw) != value["archiveSha256"]:
        raise ValueError("explicit official stdlib archive bytes changed")
    return raw


def public_get(url, limit):
    if urllib.parse.urlsplit(url).hostname != "go.dev" or not url.startswith("https://go.dev/"):
        raise ValueError("fixed official Go source URL required")
    with urllib.request.urlopen(url, timeout=30) as stream:
        final = urllib.parse.urlsplit(stream.geturl())
        if final.scheme != "https" or final.hostname not in {"go.dev", "dl.google.com"}:
            raise ValueError("unreviewed Go source redirect")
        raw = stream.read(limit + 1)
    if len(raw) > limit:
        raise ValueError("official Go source response budget")
    return raw


def environment(work, go):
    go = Path(go)
    if not go.is_absolute() or go.parent.name != "bin" or go.name not in {"go", "go.exe"}:
        raise ValueError("explicit absolute official Go root/bin/go required")
    result = {name: os.environ[name] for name in ("SYSTEMROOT", "WINDIR") if name in os.environ}
    for name in ("home", "gopath", "modcache", "buildcache", "tmp"):
        (work / name).mkdir(mode=0o700)
    result.update(HOME=str(work / "home"), USERPROFILE=str(work / "home"),
        TMPDIR=str(work / "tmp"), TMP=str(work / "tmp"), TEMP=str(work / "tmp"),
        PATH=str(go.parent) + os.pathsep + os.defpath,
        GOROOT=str(go.parent.parent), GOENV="off", GOTOOLCHAIN="local", GOWORK="off", GOFLAGS="", GO111MODULE="on",
        GOPATH=str(work / "gopath"), GOMODCACHE=str(work / "modcache"), GOCACHE=str(work / "buildcache"),
        GOPROXY="https://proxy.golang.org", GOSUMDB="sum.golang.org", GONOSUMDB="", GOPRIVATE="",
        GONOPROXY="", GOVCS="*:off", GOAUTH="off", GOTELEMETRY="off")
    return result


def command(argv, env, cwd, until):
    remaining = until - time.monotonic()
    if remaining <= 0:
        raise ValueError("collector material whole budget expired")
    # Public Go commands only; no shell, module code/test/build/install, or ELF.
    result = subprocess.run(argv, cwd=cwd, env=env, stdin=subprocess.DEVNULL,
        capture_output=True, timeout=min(60, remaining), check=True)
    if len(result.stdout) > 256 * 1024 or len(result.stderr) > 256 * 1024:
        raise ValueError("Go metadata output budget")
    return result.stdout


def collect(binary, output, go, stdlib_source_input=None, platform="linux-amd64"):
    selected = release_platforms.select(platform, "collector")
    original = read(binary, 80 * 1024 * 1024)
    release_platforms.collector_payload(original, selected)
    if not output.is_absolute() or output.exists() or output.parent.resolve() != output.parent:
        raise ValueError("new private absolute materials output required")
    output.mkdir(mode=0o700)
    go = Path(go)
    if not go.is_absolute():
        raise ValueError("explicit absolute official Go tool required")
    # Preserve the selected path for the existing strict no-link reader; resolving
    # first would erase a symlink from the original trusted-tool selection.
    go_before = digest(read(go, 128 * 1024 * 1024))
    env = environment(output, go)
    until = time.monotonic() + 600
    info = command([str(go), "version", "-m", "-json", str(binary)], env, output, until)
    write(output / "buildinfo.json", info)  # Preserve actual unresolved evidence too.
    source = official_source(selected["collector"])
    go_version, modules = build_modules(info, source)
    # Refuse a bad explicit input BEFORE downloading any module ZIP.
    stdlib_raw = explicit_stdlib_source(go_version, stdlib_source_input) if stdlib_source_input is not None else None
    rows = []
    for identity in modules:
        result = decode(command([str(go), "mod", "download", "-json", identity["path"] + "@" + identity["version"]], env, output, until))
        if result.get("Error") or any(result.get(key) != identity[field] for key, field in (("Path", "path"), ("Version", "version"), ("Sum", "sum"))):
            raise ValueError("actual Go downloader checksum/identity mismatch")
        archive = Path(result["Zip"])
        if not archive.is_absolute() or not archive.is_relative_to(output / "modcache"):
            raise ValueError("module archive outside fresh owned cache")
        raw = read(archive, 128 * 1024 * 1024)
        rows.append(dict(**identity, archiveSha256=digest(raw), materials=zip_materials(raw, identity["path"], identity["version"])))
    if stdlib_raw is not None:
        raw = stdlib_raw
    else:
        listing = decode(public_get("https://go.dev/dl/?mode=json&include=all", 8 * 1024 * 1024))
        entries = [row for version in listing if version.get("version") == go_version for row in version.get("files", [])
                   if row.get("kind") == "source" and row.get("filename") == go_version + ".src.tar.gz"]
        if len(entries) != 1:
            raise ValueError("exact official Go source archive unavailable")
        entry = entries[0]
        raw = public_get("https://go.dev/dl/" + entry["filename"], 64 * 1024 * 1024)
        if digest(raw) != entry["sha256"] or len(raw) != entry["size"]:
            raise ValueError("official matching Go source archive mismatch")
    if (go_version, digest(raw), len(raw)) != STDLIB_SOURCE_PIN:
        raise ValueError("reviewed official stdlib source pin required")
    value = dict(format="cc-vmagent-go-materials/v1", binarySha256=selected["binarySha256"], binaryBytes=len(original),
        buildInfo=decode(info), buildInfoJson=info.decode("utf-8"), buildInfoSha256=digest(info), modules=rows,
        stdlib=dict(version=go_version, archiveSha256=digest(raw), materials=stdlib_materials(raw)),
        goToolSha256=go_before, sourceNoticeProvenance=source_provenance(source), legalCertification=False, binaryExecuted=False)
    raw = encoded(value)
    checked(raw, selected["binarySha256"])
    if digest(read(binary, 80 * 1024 * 1024)) != selected["binarySha256"] or digest(read(go, 128 * 1024 * 1024)) != go_before:
        raise ValueError("original public inputs changed")
    write(output / "materials.json", raw)
    write(output / "receipt.json", encoded(dict(format="cc-vmagent-material-capture/v1", binarySha256=selected["binarySha256"],
        buildInfoSha256=digest(info), materialsSha256=digest(raw), modules=len(rows), goVersion=go_version,
        binaryExecuted=False, legalCertification=False)))


def checked(raw, binary_sha):
    rows = [release_platforms.select(name,"collector") for name in ("linux-amd64","linux-arm64")]
    matches = [row for row in rows if row["binarySha256"] == binary_sha]
    if len(matches) != 1: raise ValueError("original fixed collector platform hash required")
    selected = matches[0]
    value = decode(raw)
    fields={"format", "binarySha256", "binaryBytes", "buildInfo", "buildInfoJson",
            "buildInfoSha256", "modules", "stdlib", "sourceNoticeProvenance", "legalCertification", "binaryExecuted"}
    parsed=type(value)is dict and value.get('format')=='cc-vmagent-go-materials-parser/v1'
    # Native official-Go captures and qualified parser records stay distinct.
    # Both still pass the exact fixed binary/source/buildinfo/module checks.
    fields |= {'parserObservation','materialReuse','projectLicense'} if parsed else {'goToolSha256'}
    if (type(value) is not dict or set(value) != fields
            or value.get("format") not in {"cc-vmagent-go-materials/v1","cc-vmagent-go-materials-parser/v1"} or value.get("binarySha256") != binary_sha
            or type(value.get("binaryBytes")) is not int or value.get("binaryBytes") != selected['observedBytes'] or value.get("legalCertification") is not False
            or value.get("binaryExecuted") is not False):
        raise ValueError("actual original collector materials required")
    if not parsed and (type(value.get("goToolSha256")) is not str
            or re.fullmatch(r"[0-9a-f]{64}", value["goToolSha256"]) is None
            or value["goToolSha256"] == "0" * 64):
        raise ValueError("original Go tool capture hash required")
    # Preserve the actual raw buildinfo hash as producer evidence; object JSON
    # formatting is not reinterpreted as the original go stdout bytes.
    actual_info = value.get("buildInfoJson")
    if (type(actual_info) is not str or digest(actual_info.encode()) != value.get("buildInfoSha256")
            or decode(actual_info.encode()) != value.get("buildInfo")):
        raise ValueError("original Go buildinfo bytes changed")
    release_platforms.material_identity(value, selected)
    source = official_source(selected["collector"])
    if parsed:
        if (selected['collector']!='linux-arm64'
                or value['parserObservation']!=source.get('buildInfoObservation')
                or value['materialReuse']!=source.get('materialReuse')
                or digest(encoded(dict(modules=value['modules'],stdlib=value['stdlib'])))!=source['materialReuse']['modulesAndStdlibSha256']):
            raise ValueError('original ARM parser/material reuse evidence changed')
        check_materials([value['projectLicense']])
        if value['projectLicense']['sha256']!=source['projectLicenseSha256']:
            raise ValueError('original ARM project license changed')
    if value.get("sourceNoticeProvenance") != source_provenance(source):
        raise ValueError("qualified original notice provenance changed")
    version, modules = build_modules(actual_info.encode(), source)
    actual = value.get("modules")
    if type(actual) is not list or len(actual) != len(modules):
        raise ValueError("incomplete Go dependency material closure")
    for expected, row in zip(modules, actual):
        if (type(row) is not dict or set(row) != {"path", "version", "sum", "archiveSha256", "materials"}
                or any(row.get(key) != item for key, item in expected.items())
                or type(row.get("archiveSha256")) is not str
                or re.fullmatch(r"[0-9a-f]{64}", row["archiveSha256"]) is None):
            raise ValueError("Go material identity mismatch")
        check_materials(row.get("materials"))
    standard = value.get("stdlib", {})
    if (type(standard) is not dict or set(standard) != {"version", "archiveSha256", "materials"}
            or standard.get("version") != version or type(standard.get("archiveSha256")) is not str
            or re.fullmatch(r"[0-9a-f]{64}", standard["archiveSha256"]) is None):
        raise ValueError("matching Go standard-library materials required")
    check_materials(standard.get("materials"))
    check_stdlib_coverage(standard["materials"])
    return value


def check_materials(rows):
    if type(rows) is not list or not 1 <= len(rows) <= 2048:
        raise ValueError("complete notice material list required")
    names = set()
    for row in rows:
        if type(row) is not dict or set(row) != {"path", "sha256", "text"} or type(row["text"]) is not str:
            raise ValueError("closed full notice material schema required")
        actual = material(row["path"], row["text"].encode("utf-8"))
        if actual != row or row["path"] in names or not MATERIAL.fullmatch(PurePosixPath(row["path"]).name):
            raise ValueError("notice material changed/duplicate/not-license")
        names.add(row["path"])
    if not any(row["text"].strip() and re.match(r"(?:licen[cs]e|copying)(?:[._-]|$)", PurePosixPath(row["path"]).name, re.I) for row in rows):
        raise ValueError("full nonempty component license missing")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--go", type=Path, required=True, help="explicit trusted local official Go tool; no auto-download")
    parser.add_argument("--stdlib-source-input", type=Path, help="explicit public official-source record with adjacent version/SHA-bound source archive; no ambient cache")
    parser.add_argument("--platform", choices=("linux-amd64","linux-arm64"), default="linux-amd64")
    args = parser.parse_args()
    collect(args.binary.resolve(strict=True), args.output, args.go, args.stdlib_source_input, args.platform)


if __name__ == "__main__":
    main()
