"""Build the fixed normal hard-budget collector and its SAME-run notice input.

No fixture imports, signer, key, installation or service operation. Network is
limited to the fixed upstream archives and official Go module proxy/SumDB.
"""
from pathlib import Path
import argparse
import ctypes
import hashlib
import io
import json
import os
import re
import shutil
import signal
import stat
import subprocess
import sys
import tarfile
import tempfile
import time
import urllib.parse
import urllib.request

# Fixed sibling source directory also works under the owned -I interpreter.
sys.path.insert(0,str(Path(__file__).resolve().parent))
import collector_materials as materials
import prometheus_materials as consumer
import release_platforms

AGENT = Path(__file__).resolve().parent.parent
GIB = 1024**3
SOURCE_URL = "https://github.com/prometheus/prometheus/archive/refs/tags/v3.16.0.tar.gz"


def need(value, why):
    if not value:
        raise ValueError(why)


def read(path, cap):
    """Existing full-notice reader semantics, including held file identity."""
    path = Path(path)
    need(not any(p.is_symlink() for p in (path, *path.parents))and path.resolve(strict=True)==path.absolute(), "linked public input")
    fd = os.open(path, os.O_RDONLY | getattr(os,"O_NOFOLLOW",0) | getattr(os,"O_NONBLOCK",0)
                 | getattr(os,"O_CLOEXEC",0) | getattr(os,"O_BINARY",0))
    stamp = lambda s: (s.st_dev, s.st_ino, s.st_mode, s.st_nlink, s.st_size, s.st_mtime_ns, s.st_ctime_ns)
    try:
        before = os.fstat(fd)
        need(stat.S_ISREG(before.st_mode) and before.st_nlink == 1 and 0 <= before.st_size <= cap
             and not getattr(before,"st_file_attributes",0)&0x400,
             "bounded regular public input")
        with os.fdopen(os.dup(fd), "rb") as stream:
            raw = stream.read(cap + 1)
        need(len(raw) == before.st_size and stamp(before) == stamp(os.fstat(fd))
             == stamp(path.stat(follow_symlinks=False)), "public input changed")
        return raw
    finally:
        os.close(fd)


def write(path, raw):
    path = Path(path)
    with path.open("xb") as stream:
        stream.write(raw)
        stream.flush()
        os.fsync(stream.fileno())
    path.chmod(0o400)


def document(path, value):
    write(path, materials.encoded(value))


def fetch(url, expected, cap, until):
    go_urls={"https://go.dev/dl/"+row["filename"]for row in consumer.GO_ARCHIVES.values()}
    need(url in go_urls|{SOURCE_URL}, "fixed archive origin")
    need(time.monotonic() < until, "original producer deadline")
    # No inherited proxy, credential, cookie or URL from caller metadata.
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    with opener.open(url, timeout=min(30, until - time.monotonic())) as stream:
        final = urllib.parse.urlsplit(stream.geturl())
        allowed = {"go.dev", "dl.google.com"} if url in go_urls else {"github.com", "codeload.github.com"}
        need(final.scheme == "https" and final.hostname in allowed, "archive redirect origin")
        raw = stream.read(cap + 1)
    need(len(raw) <= cap and materials.digest(raw) == expected, "fixed archive checksum/budget")
    return raw


def extract(raw, destination, prefix, expanded, member_cap):
    """Same fixed regular-only archive contract as the existing normal builder."""
    destination.mkdir(mode=0o700)
    with tarfile.open(fileobj=io.BytesIO(raw), mode="r:gz") as archive:
        rows = archive.getmembers()
        need(len(rows) <= 30000 and len({r.name for r in rows}) == len(rows), "archive member count")
        total = 0
        for row in rows:
            if row.name == prefix and row.isdir():
                continue
            need(row.name.startswith(prefix + "/"), "archive prefix")
            name = row.name[len(prefix) + 1:]
            need(materials.safe_relative(name) and (row.isdir() or row.isfile()), "archive path/type")
            path = destination / name
            if row.isdir():
                path.mkdir(mode=0o700, parents=True, exist_ok=True)
                continue
            total += row.size
            need(0 <= row.size <= member_cap and total <= expanded, "archive expanded budget")
            path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
            with archive.extractfile(row) as source, path.open("xb") as output:
                shutil.copyfileobj(source, output, 65536)
            need(path.stat().st_size == row.size, "archive member length")
            path.chmod(0o500 if row.mode & 0o111 else 0o400)


def apply_patch(source, raw, rows):
    """The existing bounded normal producer's exact before/after text codec."""
    need(len(raw) <= 2 * 1024**2, "patch budget")
    lines = raw.decode("utf-8").splitlines(keepends=True)
    expected = {r["path"]: r for r in rows}
    need(len(expected) == len(rows) and rows, "patch closure")
    changed = set()
    index = 0
    while index < len(lines):
        if not lines[index].startswith("--- a/"):
            need(not lines[index].startswith(("--- ", "+++ ", "@@ ")), "patch framing")
            index += 1
            continue
        name = lines[index][6:].strip()
        need(materials.safe_relative(name) and name in expected and name not in changed, "patch path")
        need(lines[index + 1].rstrip("\n") == "+++ b/" + name, "patch destination")
        index += 2
        path = source / name
        before = read(path, 16 * 1024**2)
        need(materials.digest(before) == expected[name]["base_sha256"], "patch original hash")
        before = before.decode("utf-8").splitlines(keepends=True)
        result = []
        cursor = 0
        while index < len(lines) and not lines[index].startswith(("--- a/", "diff --git ")):
            head = re.fullmatch(r"@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@[^\n]*\n", lines[index])
            need(head is not None, "patch hunk")
            start = int(head[1]) - 1
            need(cursor <= start <= len(before), "patch cursor")
            result += before[cursor:start]
            cursor = start
            index += 1
            while index < len(lines) and lines[index][:1] in {" ", "+", "-"} and not lines[index].startswith("--- a/"):
                row = lines[index]
                index += 1
                if row[0] != "+":
                    need(cursor < len(before) and before[cursor] == row[1:], "patch original line")
                    cursor += 1
                if row[0] != "-":
                    result.append(row[1:])
        result += before[cursor:]
        output = "".join(result).encode()
        need(materials.digest(output) == expected[name]["result_sha256"], "patch result hash")
        path.chmod(0o600)
        path.write_bytes(output)
        path.chmod(0o400)
        changed.add(name)
    need(changed == set(expected), "patch missing member")


def source_map(source):
    return {p.relative_to(source).as_posix(): materials.digest(read(p, 16 * 1024**2))
            for p in source.rglob("*") if p.is_file()}


def assemble(agent, archive, source):
    expected, pins = consumer.source_basis(agent, archive, read)
    extract(archive, source, "prometheus-3.16.0", 40 * 1024**2, 16 * 1024**2)
    recipe = consumer.decode(read(agent / consumer.SOURCE, 128 * 1024))
    for patch in recipe["patches"]:
        apply_patch(source, read(agent / patch["path"], 2 * 1024**2), patch["files"])
    for row in recipe["additions"]:
        raw = read(agent / row["source"], 2 * 1024**2)
        need(materials.digest(raw) == row["sha256"], "addition hash")
        path = source / row["path"]
        path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        write(path, raw)
    need(source_map(source) == expected, "assembled source map")
    return expected, pins


def json_stream(raw):
    need(len(raw) <= 16 * 1024**2, "official metadata stream budget")
    decoder = json.JSONDecoder(object_pairs_hook=materials.unique)
    text = raw.decode("utf-8")
    offset = 0
    rows = []
    while offset < len(text):
        while offset < len(text) and text[offset].isspace():
            offset += 1
        if offset == len(text):
            break
        row, offset = decoder.raw_decode(text, offset)
        need(type(row) is dict and len(rows) < 1024, "official metadata object count")
        rows.append(row)
    return rows


def original_sums(raw, mod):
    values = {}
    for line in raw.decode("utf-8").splitlines():
        path, version, value = line.split()
        key = (path, version)
        need(key not in values, "duplicate upstream checksum")
        values[key] = value
    # Reuse the original dependency_plan derivation: the251 explicit root
    # requirements, NOT every historical ZIP checksum retained in go.sum.
    pairs=[];inside=False
    for line in mod.decode("utf-8").splitlines():
        line=line.split("//",1)[0].strip()
        need(not line.startswith(("replace ","exclude ")),"no replacement/exclusion")
        if line=="require (":inside=True;continue
        if line==")":inside=False;continue
        if inside and line:
            fields=line.split();need(len(fields)==2,"root requirement shape")
            path,version=fields
            need((path,version)in values and(path,version+"/go.mod")in values,"original BOTH sums required")
            materials.module_identity(dict(Path=path,Version=version,Sum=values[path,version]))
            pairs.append((path,version))
    need(len(pairs)==len(set(pairs))==251 and sum(v.endswith("/go.mod")for _,v in values)==484,
         "fixed upstream module closure")
    return values, sorted(pairs)


def import_modules(original,destination,sums,tuples,until):
    """Copy only original named public ZIP/mod/info tuples, never GOCACHE.

    Input cache readiness/receipts are ignored. The real official downloader,
    BOTH sums, mod verify and offline list run fresh before compilation.
    """
    original=Path(original)
    need(original.is_absolute()and original.is_dir(),"explicit public module cache root")
    escape=lambda value:"".join("!"+c.lower()if c.isupper()else c for c in value)
    names={}
    for path,version in sums:
        if not version.endswith("/go.mod"):continue
        version=version[:-7]
        base=Path("cache/download")/escape(path)/"@v"/escape(version)
        names[str(base)+".mod"]=(128*1024,None)
        names[str(base)+".info"]=(32768,None)
    for path,version in tuples:
        base=Path("cache/download")/escape(path)/"@v"/escape(version)
        names[str(base)+".zip"]=(128*1024**2,None)
        names[str(base)+".ziphash"]=(128,sums[path,version])
    need(len(names)<=2000,"public cache finite file count")
    total=0
    for name,(cap,h1)in sorted(names.items()):
        need(time.monotonic()<until,"public cache import original deadline")
        original_path=original/name;before=original_path.stat(follow_symlinks=False)
        raw=read(original_path,cap);after=original_path.stat(follow_symlinks=False)
        stamp=lambda st:(st.st_dev,st.st_ino,st.st_size,st.st_mtime_ns,st.st_ctime_ns)
        need(stamp(before)==stamp(after),"public module DATA replaced")
        total+=max(len(raw),getattr(after,"st_blocks",0)*512);need(total<=4*GIB,"public cache import budget")
        if h1 is not None:need(raw.strip()==h1.encode(),"original module ziphash DATA changed")
        path=destination/name;path.parent.mkdir(mode=0o700,parents=True,exist_ok=True)
        write(path,raw)
    return total


def tree_bytes(path, until):
    """Fresh no-follow FD census, bounded by the original whole deadline."""
    total = count = 0
    flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC
    root = os.open(path, flags)
    identity = lambda s: (s.st_dev, s.st_ino)
    original = identity(os.fstat(root))
    def walk(parent):
        nonlocal total, count
        with os.scandir(parent) as rows:
            for row in rows:
                need(time.monotonic() < until, "producer census deadline")
                try:
                    info = row.stat(follow_symlinks=False)
                except FileNotFoundError:
                    continue
                need(stat.S_ISREG(info.st_mode) or stat.S_ISDIR(info.st_mode), "producer census type")
                count += 1
                need(count <= 250000, "producer census entry count")
                total += max(info.st_size, getattr(info, "st_blocks", 0) * 512)
                if stat.S_ISDIR(info.st_mode):
                    try:
                        child = os.open(row.name, flags, dir_fd=parent)
                    except FileNotFoundError:
                        continue
                    try:
                        need(identity(os.fstat(child)) == identity(info), "producer directory replaced")
                        walk(child)
                    finally:
                        os.close(child)
    try:
        walk(root)
        need(original == identity(os.fstat(root)) == identity(path.lstat()), "producer root replaced")
        return total
    finally:
        os.close(root)


def group_members(group):
    result = {}
    entries = list(Path("/proc").iterdir())
    need(len(entries) <= 32768, "process census bound")
    for path in entries:
        if not path.name.isdigit():
            continue
        try:
            raw = (path / "stat").read_text()
            fields = raw[raw.rfind(")") + 2:].split()
            if int(fields[2]) == group:
                need(int(fields[3]) == group and path.stat().st_uid == os.getuid(), "owned Go group identity")
                result[int(path.name)] = int(fields[19])
        except (FileNotFoundError, ProcessLookupError):
            continue
    return result


def compiler_failure(argv,group):
    """Failure-only closed package/RSS fields; no argv/env/path output."""
    result=dict(records=[],unavailable=0)
    if len(argv)<2 or argv[1]!="build":return result
    compiler=Path(argv[0]).parent.parent/"pkg/tool"/("linux_"+release_platforms.select("linux-"+os.uname().machine,"release")["goarch"])/"compile"
    for pid,start in list(sorted(group_members(group).items()))[:8]:
        try:
            base=Path("/proc")/str(pid)
            if os.readlink(base/"exe")!=str(compiler):continue
            with(base/"cmdline").open("rb")as stream:raw=stream.read(65537)
            words=raw[:65536].split(b"\0")[:-1];places=[i for i,w in enumerate(words)if w==b"-p"]
            need(len(places)==1 and places[0]+1<len(words),"compiler package field")
            package=words[places[0]+1].decode("ascii")
            need(re.fullmatch(r"[A-Za-z0-9_][A-Za-z0-9_./+~\-]{0,511}",package)
                 and all(part not in{"",".",".."}for part in package.split("/")),"compiler public package")
            with(base/"status").open("rb")as stream:status=stream.read(8193)
            need(len(status)<=8192,"compiler status bound")
            rss=[line for line in status.splitlines()if line.startswith(b"VmRSS:")]
            need(len(rss)==1,"compiler RSS field");match=re.fullmatch(rb"VmRSS:\s*(0|[1-9][0-9]*)\s+kB",rss[0]);need(match is not None,"compiler RSS format")
            need(group_members(group).get(pid)==start,"compiler original identity")
            result["records"].append(dict(pid=pid,startTicks=start,package=package,rssBytes=int(match[1])*1024,argvPrefixTruncated=len(raw)>65536))
        except(OSError,ValueError,UnicodeError):result["unavailable"]+=1
    return result


def run(argv, cwd, env, label, until, output, cache, scratch, inputs_bytes, limit):
    """Same original process-group lifetime, finite logs and quota boundaries."""
    log = output / (label + ".log")
    began = time.monotonic()
    failure = None
    joined = False
    local=tree_bytes(cache,min(until,time.monotonic()+15))
    sample=dict(localTreeBytes=local,readonlyInputBytes=inputs_bytes,
        aggregateBytes=local+inputs_bytes,limitBytes=limit)
    need(sample["aggregateBytes"]<=limit,"producer precommand cache quota")
    need(tree_bytes(scratch,min(until,time.monotonic()+15))<=2*GIB-128*1024**2,"producer precommand scratch quota")
    error_log=output/(label+"-stderr.log")
    with log.open("xb") as stream, error_log.open("xb")as errors:
        process = subprocess.Popen(argv, cwd=cwd, env=env, stdin=subprocess.DEVNULL,
            stdout=stream, stderr=stream if label in {"normal-version","normal-help"} else errors,
            start_new_session=True)
        try:
            while process.poll() is None:
                need(time.monotonic() < until, "original producer deadline")
                need(log.stat().st_size <= 16 * 1024**2 and error_log.stat().st_size<=4*1024**2,
                     "producer log budget")
                # No recursive scan of hot compiler files. This fixed trusted
                # build policy measures complete quotas before/after joins;
                # live checks are O(1) filesystem-free/log/deadline/kernelOOM.
                need(shutil.disk_usage(scratch).free >= 128 * 1024**2, "producer filesystem free")
                cgroup=Path("/sys/fs/cgroup")
                if(cgroup/"memory.events").is_file():
                    memory={key:(cgroup/key).read_text()for key in
                            ("memory.max","memory.current","memory.peak","memory.events","memory.swap.max","memory.swap.current")}
                    events=dict(line.split()for line in memory["memory.events"].splitlines())
                    if label=="normal-build"and time.monotonic()-locals().get('last_memory_sample',0)>=7:
                        # Bounded raw O(1) samples survive even a controller kill.
                        # memory.current alone does not distinguish anon/file.
                        last_memory_sample=time.monotonic()
                        sample_record=dict(elapsedSeconds=last_memory_sample-began,raw=memory,
                            memoryStat=(cgroup/'memory.stat').read_text()[:16384])
                        with(output/'normal-build-memory.jsonl').open('ab')as samples:
                            need(samples.tell()<2*1024**2,"compiler memory diagnostic budget")
                            samples.write(materials.encoded(sample_record)+b'\n');samples.flush()
                    need(all(int(v)==0 for k,v in events.items()if k.startswith("oom")),"producer kernel OOM")
                    # events.max means reclaim/pressure, never an OOM or a
                    # reason to reset/hide the actual kernel observation.
                time.sleep(.5)
            need(process.returncode == 0, "public Go command failed")
            need(not group_members(process.pid), "leader exited with live Go descendants")
        except BaseException as error:
            failure = type(error).__name__
            if label=="normal-build":
                try:compiler_observation=compiler_failure(argv,process.pid)
                except BaseException:compiler_observation={"records":[],"unavailable":1}
            raise
        finally:
            # Reap the original leader before interpreting a zombie as a child.
            process.poll()
            for sig in (signal.SIGTERM, signal.SIGKILL):
                members = group_members(process.pid)
                if not members:
                    break
                os.killpg(process.pid, sig)
                end = time.monotonic() + 3
                while time.monotonic() < end:
                    process.poll()
                    try:
                        while os.waitpid(-process.pid, os.WNOHANG)[0]:
                            pass
                    except ChildProcessError:
                        pass
                    if not group_members(process.pid):
                        break
                    time.sleep(.05)
            process.wait(timeout=3)
            joined = not group_members(process.pid)
            final_error=None;final=None
            try:
                local=tree_bytes(cache,min(until,time.monotonic()+15))
                final=dict(localTreeBytes=local,readonlyInputBytes=inputs_bytes,
                    aggregateBytes=local+inputs_bytes,limitBytes=limit)
                need(final["aggregateBytes"]<=limit,"producer final cache quota")
                need(tree_bytes(scratch,min(until,time.monotonic()+15))<=2*GIB-128*1024**2,
                     "producer final scratch quota")
            except BaseException as error:
                final_error=error
            document(output / (label + "-result.json"), dict(exit=process.returncode,
                errorKind=failure, joined=joined, seconds=time.monotonic() - began,
                cacheObservation=locals().get("sample"),finalCacheObservation=final,
                finalQuotaError=None if final_error is None else str(final_error),
                memoryObservation=locals().get("memory"),compilerFailureObservation=locals().get("compiler_observation"),quotaPolicy="quiescent-before-after-no-live-recursive-census",
                logSha256=materials.digest(read(log, 16 * 1024**2)),
                stderrSha256=materials.digest(read(error_log,4*1024**2))))
            need(joined, "Go process group not joined")
            if final_error is not None and failure is None:raise final_error
    need(time.monotonic() < until, "original producer deadline after join")
    return read(log, 16 * 1024**2)


def query_module_metadata(command, sums, cache, env, label):
    """Reuse the existing official-Go graph metadata H1 check, never ZIPs."""
    wanted={(path,version[:-7]):checksum for (path,version),checksum in sums.items()
            if version.endswith("/go.mod")}
    need(len(wanted)==484,"original graph metadata checksum set")
    argv=["list","-m","-json","-mod=readonly",*[p+"@"+v for p,v in sorted(wanted)]]
    need(sum(len(part.encode())+1 for part in argv)<=64*1024,"graph metadata argv bound")
    raw=command(argv,label,env)
    need(len(raw)<=4*1024**2,"graph metadata JSON bound")
    seen=set()
    escape=lambda value:"".join("!"+c.lower()if c.isupper()else c for c in value)
    for row in json_stream(raw):
        pair=(row.get("Path"),row.get("Version"))
        need(pair in wanted and pair not in seen and not row.get("Error")
             and row.get("Replace")is None,"actual graph metadata tuple/error")
        need(row.get("GoModSum")==wanted[pair],"fresh official graph GoModSum mismatch")
        path,version=pair
        expected=cache/"modcache/cache/download"/escape(path)/"@v"/(escape(version)+".mod")
        need(row.get("GoMod")==str(expected),"actual graph metadata cache path")
        seen.add(pair)
    need(seen==set(wanted),"missing original graph metadata result")


def produce(output,source_archive=None,go_archive=None,module_cache=None,offline=False,target="x86_64-unknown-linux-gnu"):
    need(sys.platform == "linux" and os.uname().machine in {"x86_64","aarch64"}, "pinned compiler requires native Linux build host")
    need(os.getuid()==os.geteuid()!=0,"unprivileged build user")
    selected=release_platforms.select(target,"target")
    host=release_platforms.select("linux-"+os.uname().machine,"release")
    compiler_pin=consumer.GO_ARCHIVES[host["goarch"]]
    need(not offline or all(p is not None for p in(source_archive,go_archive,module_cache)),"explicit complete offline inputs required")
    need(output.is_absolute() and not output.exists() and output.parent.resolve() == output.parent,
         "fresh absolute producer output")
    need(shutil.disk_usage(output.parent).free >= 11 * GIB, "producer disk reserve11GiB")
    until = time.monotonic() + 1200
    output.mkdir(mode=0o700)
    for name in ("inputs", "cache", "scratch", "artifacts"):
        (output / name).mkdir(mode=0o700)
    inputs, cache, scratch, artifacts = (output / n for n in ("inputs", "cache", "scratch", "artifacts"))
    need(ctypes.CDLL(None, use_errno=True).prctl(36, 1, 0, 0, 0) == 0, "Go group subreaper")
    archive = fetch(SOURCE_URL, consumer.ARCHIVE_SHA, 8 * 1024**2, until)if source_archive is None else read(source_archive,8*1024**2)
    compiler = fetch("https://go.dev/dl/"+compiler_pin["filename"],compiler_pin["sha256"],80*1024**2,until)if go_archive is None else read(go_archive,80*1024**2)
    need(materials.digest(archive)==consumer.ARCHIVE_SHA and materials.digest(compiler)==compiler_pin["sha256"]
         and len(compiler)==compiler_pin["bytes"],"explicit archive pins")
    write(inputs / "source.tar.gz", archive)
    write(inputs / "go.tar.gz", compiler)
    tool_hash, standard = consumer.compiler_basis(compiler)
    source = cache / "source"
    expected, pins = assemble(AGENT, archive, source)
    extract(compiler, cache / "go", "go", 270 * 1024**2, 32 * 1024**2)
    go = cache / "go/bin/go"
    need(materials.digest(read(go, 32 * 1024**2)) == tool_hash, "official compiler identity")
    env = materials.environment(cache, go)
    env.update(TMPDIR=str(scratch), TMP=str(scratch), TEMP=str(scratch), CGO_ENABLED="0",
        GOFLAGS="-mod=readonly -p=1 -buildvcs=false", GOMAXPROCS="1", GOMEMLIMIT="256MiB",
        GOOS="linux",GOARCH=selected["goarch"])
    if offline:env.update(GOPROXY="off",GOSUMDB="off")
    fixed_inputs = tree_bytes(inputs,min(until,time.monotonic()+15))
    def command(args, name, selected_env=env, limit=4 * GIB):
        return run([str(go), *args], source, selected_env, name, until,
                   artifacts, cache, scratch, fixed_inputs, limit)
    command(["telemetry", "off"], "telemetry-off")
    need(command(["telemetry"], "telemetry") == b"off\n", "telemetry disabled")
    version = command(["version"], "compiler-version")
    need(("go1.27.4 linux/"+host["goarch"]).encode() in version,"actual compiler version")
    sums, tuples = original_sums(read(source / "go.sum", 128 * 1024),read(source / "go.mod",128*1024))
    if module_cache is not None:
        fixed_inputs+=import_modules(module_cache,cache/"modcache",sums,tuples,until)
    downloaded = json_stream(command(["mod", "download", "-json",
        *[p + "@" + v for p, v in tuples]], "module-download"))
    need(len(downloaded) == len(tuples), "all original module download records")
    by_tuple={(row.get("Path"),row.get("Version")):row for row in downloaded}
    need(len(by_tuple)==len(tuples)and set(by_tuple)==set(tuples),"exact original module set")
    for original in tuples:
        row=by_tuple[original]
        path, version = original
        need(not row.get("Error") and (row.get("Path"), row.get("Version")) == original
            and row.get("Sum") == sums[path, version]
            and row.get("GoModSum") == sums[path, version + "/go.mod"], "fresh official BOTH module sums")
    # The fresh path must fetch the original metadata-only go.sum entries,
    # just as the accepted imported-cache path does. Official Go checks each
    # H1; these484 metadata tuples never enlarge the251 source ZIP set.
    query_module_metadata(command,sums,cache,env,"module-metadata")
    offline = {**env, "GOPROXY": "off", "GOSUMDB": "off"}
    query_module_metadata(command,sums,cache,offline,"module-metadata-offline")
    # Unlike JSON module-info enrichment, this official MVS graph does not
    # query .info/Time for metadata-only labels outside source go.sum tuples.
    command(["mod", "graph"], "module-graph", offline)
    command(["mod", "verify"], "module-verify", offline)
    command(["list", "-deps", "-mod=readonly", "./cmd/prometheus"], "offline-dependencies", offline)
    need(source_map(source) == expected, "original assembled source changed during validation")
    return compile_and_finish(output,cache,scratch,source,artifacts,inputs,go,command,offline,fixed_inputs,
                              selected,expected,pins,tool_hash,compiler_pin,standard,sums,until)


def compile_and_finish(output,cache,scratch,source,artifacts,inputs,go,command,offline,fixed_inputs,
                       selected,expected,pins,tool_hash,compiler_pin,standard,sums,until,resumed=None):
    binary = artifacts / "prometheus"
    compile_env = {**offline, "GOMAXPROCS": "2", "GOFLAGS": "-mod=readonly -p=1 -buildvcs=false", "GOMEMLIMIT":"1GiB"}
    origin=dict(format="cc-normal-prometheus-public-compiler-cache-origin/v1",
        source21=pins,assembledSourceMapSha256=materials.digest(materials.encoded(expected)),
        sourceArchiveSha256=consumer.ARCHIVE_SHA,compilerArchiveSha256=compiler_pin["sha256"],
        compilerToolSha256=tool_hash,target=selected["target"],goVersion=consumer.GO_VERSION,
        producerSourceSha256=materials.digest(read(Path(__file__).resolve(),1024*1024)),
        flags=["-mod=readonly","-p=1","-trimpath","-buildvcs=false"],
        goMaxProcs=2,goMemLimit="1GiB",resumedFrom=resumed,
        cacheRelativePath="cache/buildcache",dataOnly=True,acceptanceArtifact=False)
    document(output/"compiler-cache-origin.json",origin)
    try:
        command(["build", "-mod=readonly", "-p=1", "-trimpath", "-buildvcs=false",
            "-o", str(binary), "./cmd/prometheus"], "normal-build", compile_env, 6 * GIB)
    except BaseException as error:
        # Public cache stays under this SAME owned output, even on failure.
        # Container/process cleanup must not erase it or relabel it as an ELF.
        document(output/"compile-failure.json",dict(format="cc-normal-prometheus-compile-failure/v1",
            errorKind=type(error).__name__,compilerCacheOriginSha256=materials.digest(read(output/"compiler-cache-origin.json",32768)),
            retainedPublicCache="cache/buildcache",dataOnly=True,acceptanceArtifact=False,
            freshValidationRequired=True,compiledELFAccepted=False))
        raise
    raw_binary = read(binary, 256 * 1024**2)
    need(raw_binary[:6] == b"\x7fELF\x02\x01" and int.from_bytes(raw_binary[18:20], "little") == selected["elfMachine"],
         "actual normal selected ELF")
    info = command(["version", "-m", "-json", str(binary)], "buildinfo", offline, 6 * GIB)
    need(command(["version", "-m", str(binary)], "buildinfo-text", offline, 6 * GIB),
         "normal Go metadata")
    # Cross ARM bytes are never executed on the AMD64 build host. The
    # official Go reader still binds actual target settings and full ELF bytes.
    target_executed=selected["machine"]==os.uname().machine
    if target_executed:
        observed_version = run([str(binary), "--version"], source, offline, "normal-version",
            min(until, time.monotonic() + 10), artifacts, cache, scratch, fixed_inputs, 6 * GIB)
        help_text = run([str(binary), "--help"], source, offline, "normal-help",
            min(until, time.monotonic() + 10), artifacts, cache, scratch, fixed_inputs, 6 * GIB)
        need(b"storage.agent.hard-budget" in help_text, "actual hard-budget flag")
    else:
        observed_version=b""
        write(artifacts/"normal-version.log",observed_version)
    need(source_map(source) == expected, "original source changed during normal compile")
    document(artifacts / "source-map.json", expected)
    report = dict(passed=True, normalProductBuild=True, noDaemonStarted=True, sourceUnchanged=True,
        binarySha256=materials.digest(raw_binary), binaryBytes=len(raw_binary),
        sourceMapSha256=materials.digest(read(artifacts / "source-map.json", 8 * 1024**2)),
        buildInfoSha256=materials.digest(info), versionOutputSha256=materials.digest(observed_version),
        source21=pins,target=selected["target"],platform=selected["release"],
        normalCLIExecutedForVersionAndHelp=target_executed,
        versionObservation="target-cli"if target_executed else"cross-built-target-not-executed")
    document(artifacts / "build-report.json", report)
    parsed = consumer.decode(info)
    need(parsed.get("GoVersion") == consumer.GO_VERSION and parsed.get("Path")
        == "github.com/prometheus/prometheus/cmd/prometheus", "normal buildinfo identity")
    rows = []
    for item in parsed["Deps"]:
        identity = materials.module_identity(item)
        need(sums.get((identity["path"], identity["version"])) == identity["sum"], "ELF original checksum")
        escape = lambda value: "".join("!" + c.lower() if c.isupper() else c for c in value)
        path = cache / "modcache/cache/download" / escape(identity["path"]) / "@v" / (escape(identity["version"]) + ".zip")
        raw = read(path, 128 * 1024**2)
        rows.append({**identity, "archivePath": str(path), "archiveSha256": materials.digest(raw),
            "materials": materials.zip_materials(raw, identity["path"], identity["version"])})
    record = dict(format="cc-prometheus-go-materials/v1", binarySha256=report["binarySha256"],
        buildInfoSha256=report["buildInfoSha256"], modules=rows,
        stdlib=dict(version=consumer.GO_VERSION, archiveSha256=compiler_pin["sha256"], materials=standard),
        legalCertification=False, binaryExecuted=False)
    material_raw = materials.encoded(record)
    need(len(material_raw) <= consumer.MAX_JSON, "complete notice JSON budget")
    write(artifacts / "materials.json", material_raw)
    spec = dict(path=str(binary), sha256=report["binarySha256"], bytes=len(raw_binary),
        asBuiltReportPath=str(artifacts / "build-report.json"),
        asBuiltReportSha256=materials.digest(read(artifacts / "build-report.json", 256 * 1024)),
        sourceMapPath=str(artifacts / "source-map.json"), sourceMapSha256=report["sourceMapSha256"],
        buildInfoPath=str(artifacts / "buildinfo.log"), buildInfoSha256=report["buildInfoSha256"],
        versionOutputPath=str(artifacts / "normal-version.log"), versionOutputSha256=report["versionOutputSha256"],
        materialsPath=str(artifacts / "materials.json"), materialsSha256=materials.digest(material_raw),
        sourceArchivePath=str(inputs / "source.tar.gz"), goArchivePath=str(inputs / "go.tar.gz"), goPath=str(go))
    # Invoke the SAME maintained consumer, including fresh official HashZip and
    # official ELF buildinfo, BEFORE publishing a usable component descriptor.
    saved = tempfile.tempdir
    tempfile.tempdir = str(scratch)
    try:
        consumer.component(AGENT, spec, read, lambda origin, body: dict(
            original_path=origin, package_path="notices/text/" + materials.digest(body) + ".txt",
            sha256=materials.digest(body)),until=until)
    finally:
        tempfile.tempdir = saved
    need(time.monotonic() < until and source_map(source) == expected, "original producer deadline/source")
    document(output / "input.json", spec)
    document(output / "receipt.json", dict(format="cc-normal-prometheus-ci-producer/v1",
        passed=True, descriptorSha256=materials.digest(read(output / "input.json", 16384)),
        binarySha256=report["binarySha256"], modules=len(rows), source21=pins,
        original251BothH1=True, officialModuleVerify=True, offlineDependencyList=True,
        reusedValidation=resumed,
        fullMaterialConsumerAccepted=True, normalCLIExecutedForVersionAndHelp=target_executed,
        target=selected["target"],runtimeReady=False,
        daemonStarted=False, installed=False, signed=False, legalCertification=False))


def resume_environment(cache,go,scratch,goarch):
    # environment() is deliberately fresh-only. Keep that shared contract;
    # only the two checked, owned public caches are reused in this path.
    work=Path(tempfile.mkdtemp(prefix='compile-resume-env-',dir=cache))
    offline=materials.environment(work,go)
    offline.update(GOMODCACHE=str(cache/'modcache'),GOCACHE=str(cache/'buildcache'),
        TMPDIR=str(scratch),TMP=str(scratch),TEMP=str(scratch),CGO_ENABLED='0',
        GOFLAGS='-mod=readonly -p=1 -buildvcs=false',GOMAXPROCS='2',GOMEMLIMIT='1GiB',
        GOOS='linux',GOARCH=goarch,GOPROXY='off',GOSUMDB='off')
    return offline


def resume_compile(output,origin_sha,target):
    """Continue this producer's owned compiler DATA; never import a GOCACHE.

    The owner must retain custody of the exact private workspace. The caller's
    SHA is an identity check, not signature authority or acceptance of an ELF.
    """
    need(sys.platform=='linux'and os.getuid()==os.geteuid()!=0,"native nonroot resume")
    need(output.is_absolute()and output.is_dir()and not output.is_symlink(),"retained producer output")
    need(shutil.disk_usage(output.parent).free>=11*GIB,"producer disk reserve11GiB")
    need(re.fullmatch('[0-9a-f]{64}',origin_sha)is not None,"original compiler origin SHA")
    until=time.monotonic()+1200
    inputs,cache,scratch,artifacts=(output/name for name in('inputs','cache','scratch','artifacts'))
    original=read(output/'compiler-cache-origin.json',32768)
    need(materials.digest(original)==origin_sha,"owned compiler origin changed")
    origin=materials.decode(original);selected=release_platforms.select(target,'target')
    release_platforms.require_native(selected);compiler_pin=consumer.GO_ARCHIVES[selected['goarch']]
    archive=read(inputs/'source.tar.gz',8*1024**2);compiler=read(inputs/'go.tar.gz',80*1024**2)
    expected,pins=consumer.source_basis(AGENT,archive,read);tool_hash,standard=consumer.compiler_basis(compiler)
    source=cache/'source';go=cache/'go/bin/go'
    need(materials.digest(compiler)==compiler_pin['sha256']and len(compiler)==compiler_pin['bytes']
         and materials.digest(read(go,32*1024**2))==tool_hash,"retained official compiler identity")
    need(source_map(source)==expected,"retained source21 changed")
    need(origin.get('format')=='cc-normal-prometheus-public-compiler-cache-origin/v1'
         and origin.get('dataOnly')is True and origin.get('acceptanceArtifact')is False
         and origin.get('source21')==pins and origin.get('target')==target
         and origin.get('assembledSourceMapSha256')==materials.digest(materials.encoded(expected))
         and origin.get('compilerArchiveSha256')==compiler_pin['sha256']and origin.get('compilerToolSha256')==tool_hash
         and origin.get('sourceArchiveSha256')==consumer.ARCHIVE_SHA and origin.get('goVersion')==consumer.GO_VERSION
         and origin.get('flags')==['-mod=readonly','-p=2','-trimpath','-buildvcs=false'],"exact original compile provenance")
    need(not any((output/name).exists()for name in('receipt.json','input.json','compile-failure.json'))
         and not(artifacts/'prometheus').exists(),"unfinished compile only, never reuse an ELF")
    gates={}
    for label in('module-download','module-graph','module-verify','offline-dependencies'):
        raw=read(artifacts/(label+'-result.json'),65536);value=materials.decode(raw)
        need(value.get('exit')==0 and value.get('joined')is True and value.get('finalQuotaError')is None
             and value['logSha256']==materials.digest(read(artifacts/(label+'.log'),16*1024**2))
             and value['stderrSha256']==materials.digest(read(artifacts/(label+'-stderr.log'),4*1024**2)),"original joined validation evidence changed")
        gates[label]=materials.digest(raw)
    fixed_inputs=value['cacheObservation']['readonlyInputBytes']
    need(type(fixed_inputs)is int and 0<fixed_inputs<=4*GIB,"original bounded input accounting")
    resumed=dict(originSha256=origin_sha,gateResultSha256=gates,validationRepeated=False,
                 existingPrivateCompilerData=True,artifactAccepted=False)
    offline=resume_environment(cache,go,scratch,selected['goarch'])
    # Preserve all original result/log bytes before opening a new compiler log.
    artifacts.rename(output/'artifacts-before-compile-resume');artifacts.mkdir(mode=0o700)
    (output/'compiler-cache-origin.json').rename(output/'compiler-cache-origin-before-resume.json')
    need(ctypes.CDLL(None,use_errno=True).prctl(36,1,0,0,0)==0,"Go group subreaper")
    def command(args,name,selected_env=offline,limit=6*GIB):
        return run([str(go),*args],source,selected_env,name,until,artifacts,cache,scratch,fixed_inputs,limit)
    sums,_=original_sums(read(source/'go.sum',128*1024),read(source/'go.mod',128*1024))
    return compile_and_finish(output,cache,scratch,source,artifacts,inputs,go,command,offline,fixed_inputs,
                              selected,expected,pins,tool_hash,compiler_pin,standard,sums,until,resumed)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--source-archive",type=Path)
    parser.add_argument("--go-archive",type=Path)
    parser.add_argument("--module-cache",type=Path,help="explicit public Go module-cache DATA; fresh official checks always required")
    parser.add_argument("--offline",action="store_true")
    parser.add_argument("--resume-compile-origin-sha256",help="explicit original owned-workspace origin; no cache import or supply download")
    parser.add_argument("--target",choices=("x86_64-unknown-linux-gnu","aarch64-unknown-linux-gnu"),default="x86_64-unknown-linux-gnu")
    args = parser.parse_args()
    if args.resume_compile_origin_sha256:
        need(args.offline and all(p is None for p in(args.source_archive,args.go_archive,args.module_cache)),"compile-only resume cannot accept other DATA inputs")
        resume_compile(args.output,args.resume_compile_origin_sha256,args.target)
    else:produce(args.output,args.source_archive,args.go_archive,args.module_cache,args.offline,args.target)
