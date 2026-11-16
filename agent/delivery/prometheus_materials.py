"""Fixed maintained3.16.0 normal-ELF notice binding. No signing/download/ELF run."""
from pathlib import Path,PurePosixPath
from contextlib import contextmanager
import hashlib,io,json,os,re,subprocess,tarfile,tempfile,time
import collector_materials as go_materials
import release_platforms

SOURCE="third_party/prometheus-agent/SOURCE.json"
SOURCE_SHA="5f240d398937096d7ae77c0f9617e11f6e72a642d6737e88f295771600f2185e"
ARCHIVE_SHA="2709a3bd43f9d15e0cecfc33ab473d011e2a69ab29265b91398cc1b8561403c0"
GO_ARCHIVE_SHA="7226736c2339719cda28fbf3a54d71aa63bcefe376e98cef00034268dab24840"
GO_VERSION="go1.27.4"
# Exact official go.dev mode=json observations; no archive was downloaded to
# invent the ARM row. Each actual reader must match its held archive bytes.
GO_ARCHIVES={
    "amd64":dict(filename="go1.27.4.linux-amd64.tar.gz",sha256=GO_ARCHIVE_SHA,bytes=70680823),
    "arm64":dict(filename="go1.27.4.linux-arm64.tar.gz",sha256="3392582988a273a87ab5043ee1b2c9c17a2c3c00580bf9326c47a2e0aaaeac92",bytes=67129258),
}
PATCHES=("7781ad10ff47ff43c435646845eeb0e8f9ffcea9e658cf9db43eb9599edf7821",
         "0edf8febc02164257b71732f2e15f1b47fa0b6272f92ddfcebd1628a9296a04a")
MAX_JSON=16*1024*1024

def need(value):
    if not value:raise ValueError("maintained normal Prometheus notice input refused")
def digest(raw):return hashlib.sha256(raw).hexdigest()
def decode(raw):
    need(type(raw)is bytes and len(raw)<=MAX_JSON)
    def pairs(items):
        result={}
        for key,value in items:need(key not in result);result[key]=value
        return result
    return json.loads(raw,object_pairs_hook=pairs,parse_float=lambda _:need(False),parse_constant=lambda _:need(False))
def hash_value(value):return type(value)is str and re.fullmatch("[0-9a-f]{64}",value)is not None

def source_basis(agent,archive,source):
    raw=source(agent/SOURCE,128*1024);need(digest(raw)==SOURCE_SHA)
    value=decode(raw);need(value["upstream_tag"]=="v3.16.0"and value["source_archive_sha256"]==ARCHIVE_SHA)
    need(digest(archive)==ARCHIVE_SHA and tuple(p["sha256"]for p in value["patches"])==PATCHES)
    expected={};total=0
    with tarfile.open(fileobj=io.BytesIO(archive),mode="r:gz")as tar:
        members=tar.getmembers();need(len(members)<=10000 and len({m.name for m in members})==len(members))
        for item in members:
            if item.name=="prometheus-3.16.0"and item.isdir():continue
            need(item.name.startswith("prometheus-3.16.0/"))
            name=item.name[len("prometheus-3.16.0/"):]
            if item.isdir():continue
            # The fixed archive includes tsdb/testdata/20kseries.json (10,003,571B).
            # This source-hash reader does not change packaged artifact caps.
            need(item.isfile()and go_materials.safe_relative(name)and item.size<=16*1024*1024)
            total+=item.size;need(total<=40*1024*1024)
            expected[name]=digest(tar.extractfile(item).read(16*1024*1024+1))
    for name,h in value["upstream_pins"].items():need(expected.get(name)==h)
    pins={SOURCE:SOURCE_SHA}
    for patch in value["patches"]:
        need(digest(source(agent/patch["path"],1024*1024))==patch["sha256"]);pins[patch["path"]]=patch["sha256"]
        for row in patch["files"]:
            need(expected.get(row["path"])==row["base_sha256"]);expected[row["path"]]=row["result_sha256"]
    for row in value["additions"]:
        need(row["path"]not in expected and digest(source(agent/row["source"],2*1024*1024))==row["sha256"])
        pins[row["source"]]=row["sha256"];expected[row["path"]]=row["sha256"]
    need(len(pins)==21)
    for name in("go.mod","go.sum","LICENSE","NOTICE"):
        need(digest(source(agent/"third_party/prometheus-agent/upstream"/name,2*1024*1024))==value["upstream_pins"][name])
    return expected,pins

def compiler_basis(raw):
    need(any(digest(raw)==row["sha256"]and len(raw)==row["bytes"]for row in GO_ARCHIVES.values()));materials=[]
    with tarfile.open(fileobj=io.BytesIO(raw),mode="r:gz")as tar:
        members=tar.getmembers();need(len(members)<=30000 and len({m.name for m in members})==len(members))
        version=tar.extractfile("go/VERSION").read(128);need(version.splitlines()[0].decode()==GO_VERSION)
        tool=tar.getmember("go/bin/go");need(tool.isfile()and 0<tool.size<=32*1024*1024)
        tool_hash=digest(tar.extractfile(tool).read(32*1024*1024+1))
        for item in members:
            if item.name=="go"and item.isdir():continue
            need(item.name.startswith("go/"))
            name=item.name[3:]
            if item.isdir():continue
            need(item.isfile()and go_materials.safe_relative(name))
            if go_materials.MATERIAL.fullmatch(PurePosixPath(name).name):
                need(item.size<=128*1024)
                materials.append(go_materials.material(name,tar.extractfile(item).read(128*1024+1)))
    need({"LICENSE","PATENTS"}<={r["path"]for r in materials})
    return tool_hash,sorted(materials,key=lambda r:r["path"])

@contextmanager
def official_reader(go,source,tool_hash,until=None):
    # Only the explicit official reader whose bytes match the fixed toolchain
    # archive runs. The selected Prometheus input ELF is never executed here.
    need(Path(go).is_absolute()and os.name=="posix")
    tool=source(Path(go),32*1024*1024);need(digest(tool)==tool_hash)
    with tempfile.TemporaryDirectory(prefix="cc-prometheus-buildinfo-")as work:
        # Execute a private snapshot of the already hash-verified official
        # reader, so replacing the selected input path cannot swap code later.
        reader=Path(work)/"go-reader"
        fd=os.open(reader,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_CLOEXEC,0o500)
        try:
            pending=memoryview(tool)
            while pending:
                n=os.write(fd,pending);need(n>0);pending=pending[n:]
            os.fchmod(fd,0o500);os.fsync(fd)
        finally:os.close(fd)
        env={name:os.environ[name]for name in("SYSTEMROOT","WINDIR")if name in os.environ}
        env.update(HOME=work,USERPROFILE=work,TMPDIR=work,TMP=work,TEMP=work,
            PATH=str(Path(go).parent)+os.pathsep+os.defpath,GOROOT=str(Path(go).parent.parent),
            GOENV="off",GOTOOLCHAIN="local",GOWORK="off",GOPROXY="off",GOSUMDB="off",
            GOVCS="*:off",GOAUTH="off",GOFLAGS="",GOMAXPROCS="1",GOMEMLIMIT="64MiB")
        env.update(GOPATH=str(Path(work)/"gopath"),GOMODCACHE=str(Path(work)/"modcache"),
            GOCACHE=str(Path(work)/"buildcache"),GOTELEMETRY="off",GOTELEMETRYDIR=str(Path(work)/"telemetry"))
        def run(args,timeout=15):
            if until is not None:
                timeout=min(timeout,until-time.monotonic());need(timeout>0)
            result=subprocess.run([str(reader),*args],cwd=work,env=env,stdin=subprocess.DEVNULL,capture_output=True,timeout=timeout,check=True)
            need(len(result.stdout)<=256*1024 and len(result.stderr)<=4096 and result.stderr==b"")
            return result.stdout
        run(["telemetry","off"])
        yield Path(work),run


def actual_build_info(go,binary,source,tool_hash,until=None):
    reader=official_reader(go,source,tool_hash)if until is None else official_reader(go,source,tool_hash,until)
    with reader as(work,run):
        # The parser sees the SAME bytes already bound to the descriptor.
        # Input pathname replacement cannot change the ELF being inspected.
        snapshot=work/"input.elf"
        with snapshot.open("xb")as stream:stream.write(binary)
        return run(["version","-m","-json",str(snapshot)])


def verify_dependencies(go,source,tool_hash,rows,until=None):
    # The graph-only .mod stubs below select EXACT supplied ZIPs, not their
    # dependency graphs. They are never packaged or claimed as source material.
    # Go's own verifyMod calls dirhash.HashZip and compares with .ziphash even
    # when the expanded module directory is absent. No extraction/download.
    reader=official_reader(go,source,tool_hash)if until is None else official_reader(go,source,tool_hash,until)
    with reader as(work,run):
        requirements=[];sums=[];total=0
        for row in rows:
            identity=go_materials.module_identity(dict(Path=row["path"],Version=row["version"],Sum=row["sum"]))
            need(hash_value(row["archiveSha256"]))
            raw=source(Path(row["archivePath"]),128*1024*1024)
            total+=len(raw);need(total<=2*1024**3 and digest(raw)==row["archiveSha256"])
            escape=lambda s:"".join("!"+c.lower()if"A"<=c<="Z"else c for c in s)
            base=work/"modcache"/"cache"/"download"/escape(identity["path"])/"@v"
            base.mkdir(parents=True,exist_ok=True)
            version=escape(identity["version"])
            for suffix,body in((".zip",raw),(".ziphash",identity["sum"].encode()),
                (".mod",("module "+identity["path"]+"\ngo 1.27.4\n").encode())):
                with(base/(version+suffix)).open("xb")as stream:stream.write(body)
            requirements.append("\t"+identity["path"]+" "+identity["version"])
            sums.append(identity["path"]+" "+identity["version"]+" "+identity["sum"])
        need(1<=len(requirements)<=251 and len(set(requirements))==len(requirements))
        module=work/"module";module.mkdir(mode=0o700)
        (module/"go.mod").write_text("module cc.invalid/prometheus-notice-verification\n\ngo 1.27.4\n\nrequire (\n"+"\n".join(requirements)+"\n)\n")
        (module/"go.sum").write_text("\n".join(sums)+"\n")
        need(run(["-C",str(module),"mod","verify"],120)==b"all modules verified\n")

def normal_platform(binary,report):
    # Old reports remain their original AMD64 contract. ARM requires an
    # explicit new report and actual ELF/official-Go settings binding below.
    target=report.get("target","x86_64-unknown-linux-gnu")
    selected=release_platforms.select(target,"target")
    need(binary[:6]==b"\x7fELF\x02\x01"and len(binary)>=64
         and int.from_bytes(binary[18:20],"little")==selected["elfMachine"])
    if "target"in report:need(report.get("platform")==selected["release"])
    return selected


def version_observation(report,raw,selected):
    if report.get("normalCLIExecutedForVersionAndHelp")is False:
        need(report.get("versionObservation")=="cross-built-target-not-executed"and raw==b"")
        return None
    if "target"in report:
        need(report.get("normalCLIExecutedForVersionAndHelp")is True
             and report.get("versionObservation")=="target-cli")
    need(raw and b"prometheus"in raw.lower()and b"version"in raw.lower())
    return raw.decode("utf-8")


def binding(binary,report,source_map,expected_map,recorded_info,observed_info):
    selected=normal_platform(binary,report)
    need(report.get("passed")is True and report.get("normalProductBuild")is True
        and report.get("sourceUnchanged")is True and report.get("noDaemonStarted")is True
        and report.get("binarySha256")==digest(binary)and type(report.get("binaryBytes"))is int
        and report["binaryBytes"]==len(binary)and report.get("sourceMapSha256")==digest(source_map)
        and report.get("buildInfoSha256")==digest(recorded_info))
    need(decode(source_map)==expected_map and recorded_info==observed_info)
    info=decode(observed_info);main=info.get("Main")
    need(info.get("GoVersion")==GO_VERSION and info.get("Path")=="github.com/prometheus/prometheus/cmd/prometheus"
        and type(main)is dict and main.get("Path")=="github.com/prometheus/prometheus"
        and main.get("Version")=="(devel)"and main.get("Replace")is None)
    need(type(info.get("Deps"))is list and 1<=len(info["Deps"])<=251)
    if "target"in report:
        settings=info.get("Settings");need(type(settings)is list)
        pairs=[(r.get("Key"),r.get("Value"))for r in settings if type(r)is dict]
        need(len(pairs)==len(settings)and len({key for key,_ in pairs})==len(pairs))
        actual=dict(pairs)
        need(actual.get("GOOS")=="linux"and actual.get("GOARCH")==selected["goarch"]
             and actual.get("CGO_ENABLED")=="0")
    return info

def checked_dependency(expected,row,raw,sums):
    identity=go_materials.module_identity(expected)
    need(type(row)is dict and set(row)=={"path","version","sum","archivePath","archiveSha256","materials"}
        and all(row[k]==v for k,v in identity.items())and sums.get((row["path"],row["version"]))==row["sum"]
        and digest(raw)==row["archiveSha256"])
    full=go_materials.zip_materials(raw,row["path"],row["version"]);need(row["materials"]==full)
    return identity,full

def component(agent,spec,source,notice,until=None):
    required={"path","sha256","bytes","asBuiltReportPath","asBuiltReportSha256",
        "sourceMapPath","sourceMapSha256","buildInfoPath","buildInfoSha256",
        "versionOutputPath","versionOutputSha256","materialsPath","materialsSha256",
        "sourceArchivePath","goArchivePath","goPath"}
    need(type(spec)is dict and set(spec)==required and hash_value(spec["sha256"])and type(spec["bytes"])is int
        and 64<=spec["bytes"]<=256*1024*1024)
    need(all(type(spec[n])is str and Path(spec[n]).is_absolute()for n in required if n.endswith("Path")or n=="path"))
    need(all(hash_value(spec[n])for n in required if n.endswith("Sha256")))
    binary=source(Path(spec["path"]),256*1024*1024);need(digest(binary)==spec["sha256"]and len(binary)==spec["bytes"])
    def file(field,cap):
        raw=source(Path(spec[field+"Path"]),cap);need(digest(raw)==spec[field+"Sha256"]);return raw
    report=decode(file("asBuiltReport",256*1024));source_map=file("sourceMap",8*1024*1024)
    recorded=file("buildInfo",256*1024);version=file("versionOutput",4096)
    need(report.get("versionOutputSha256")==digest(version))
    selected=normal_platform(binary,report)
    observed_version=version_observation(report,version,selected)
    archive=source(Path(spec["sourceArchivePath"]),8*1024*1024)
    expected,pins=source_basis(agent,archive,source)
    compiler=source(Path(spec["goArchivePath"]),80*1024*1024);tool_hash,standard=compiler_basis(compiler)
    actual=(actual_build_info(spec["goPath"],binary,source,tool_hash)if until is None
            else actual_build_info(spec["goPath"],binary,source,tool_hash,until))
    info=binding(binary,report,source_map,expected,recorded,actual)
    # Full ZIP materials AND recomputed official h1 are tied to the exact ELF
    # dependency and original upstream go.sum before any component is returned.
    material_raw=file("materials",MAX_JSON);material=decode(material_raw)
    need(type(material)is dict and set(material)=={"format","binarySha256","buildInfoSha256","modules","stdlib","legalCertification","binaryExecuted"}
        and material["format"]=="cc-prometheus-go-materials/v1"and material["binarySha256"]==digest(binary)
        and material["buildInfoSha256"]==digest(recorded)and material["legalCertification"]is False and material["binaryExecuted"]is False)
    sums={}
    for line in source(agent/"third_party/prometheus-agent/upstream/go.sum",2*1024*1024).decode().splitlines():
        path,version_,h=line.split();need((path,version_)not in sums);sums[path,version_]=h
    modules=[go_materials.module_identity(row)for row in info["Deps"]]
    need(len({r["path"]for r in modules})==len(modules)and type(material["modules"])is list and len(material["modules"])==len(modules))
    groups=[];public_modules=[];total=0
    for original,row in zip(info["Deps"],material["modules"]):
        need(type(row)is dict and type(row.get("archivePath"))is str and Path(row["archivePath"]).is_absolute())
        raw=source(Path(row["archivePath"]),128*1024*1024);total+=len(raw);need(total<=2*1024**3 and digest(raw)==row["archiveSha256"])
        identity,full=checked_dependency(original,row,raw,sums)
        groups.append((row["path"]+"@"+row["version"],full))
        public_modules.append({**identity,"archiveSha256":digest(raw),"materialSha256":{m["path"]:m["sha256"]for m in full}})
    if until is None:verify_dependencies(spec["goPath"],source,tool_hash,material["modules"])
    else:verify_dependencies(spec["goPath"],source,tool_hash,material["modules"],until)
    need(material["stdlib"]=={"version":GO_VERSION,"archiveSha256":digest(compiler),"materials":standard})
    groups.append((GO_VERSION,standard));notices=[];chunk=b""
    for name,rows in groups:
        for row in rows:
            section=("=== "+name+"/"+row["path"]+" ===\n").encode()+row["text"].encode()+b"\n"
            need(len(section)<=128*1024)
            if len(chunk)+len(section)>128*1024:notices.append(notice("actual maintained Prometheus Go materials: full text",chunk));chunk=b""
            chunk+=section
    if chunk:notices.append(notice("actual maintained Prometheus Go materials: full text",chunk))
    for name in("LICENSE","NOTICE"):
        notices.append(notice("Prometheus/v3.16.0/"+name,source(agent/"third_party/prometheus-agent/upstream"/name,128*1024)))
    notices.append(notice("ConsoleCrypt modifications/LICENSE",source(agent/"LICENSE",128*1024)))
    return dict(name="prometheus",version="3.16.0",kind="bundled-collector-binary",platform=selected["collector"],
        package_path="collector/prometheus-hardmode",binary_sha256=digest(binary),binary_bytes=len(binary),
        build_descriptor_sha256=spec["asBuiltReportSha256"],build_info_sha256=spec["buildInfoSha256"],
        actual_main_module_version=info["Main"]["Version"],actual_version_output=observed_version,
        target_cli_executed=observed_version is not None,
        version_basis="fixed upstream source3.16.0 plus reviewed patches; embedded release version is not invented",
        source21_sha256=pins,assembled_source_map_sha256=spec["sourceMapSha256"],upstream_archive_sha256=ARCHIVE_SHA,
        actual_go_version=GO_VERSION,compiler_archive_sha256=digest(compiler),go_modules=public_modules,
        stdlib_material_sha256={r["path"]:r["sha256"]for r in standard},
        license_declared="Apache-2.0 upstream; ConsoleCrypt modifications AGPL-3.0-only",notices=notices,
        license_compatibility_certified=False,reproducible_build_claimed=False,authenticated_signer_claimed=False)
