"""Native AArch64 afterimage of the existing canonical packet sealer.
Fresh same-command build only. Existing schemas/export limits and native
verifier apply; no historical normal/helper reuse or runtime acceptance.
Imported by the existing owner's native ARM build entry; no action on import.
"""
from pathlib import Path
import hashlib,json,os,sys
from release_verifier import ReleaseVerifier
from notice_generate import read
from current_release_inputs import EXPORT_FILES,EXPORT_TOTAL,export_limit
sha=lambda raw:hashlib.sha256(raw).hexdigest()

def write(path,value):
    with path.open('xb')as f:f.write((json.dumps(value,indent=2)+'\n').encode())

def seal(root,agent,disk_input,target_dir,reviewed_pin):
    root,agent,disk_input,target_dir,reviewed_pin=map(Path,(root,agent,disk_input,target_dir,reviewed_pin))
    assert sys.platform=='linux' and os.uname().machine=='aarch64'
    assert all(p.is_absolute() and not p.is_symlink() for p in(root,agent,disk_input,target_dir,reviewed_pin))
    assert root.name=='agent-ram-canonical' and root.is_dir()
    assert not any((root/name).exists() for name in('RESUME_SOURCE.json','helpers-cargo-build.jsonl','compiled-helpers'))
    import release_pipeline,prototype_package
    disk_input=release_pipeline.disk_input(disk_input,'aarch64-unknown-linux-gnu')
    runtime_pin=prototype_package.public_runtime_pin(reviewed_pin)
    entries=[];normal=None;key=None;first=None
    for number,display,letter in((1,'0.1.0','a'),(2,'0.1.1','b'),(3,'0.1.2','disk')):
        base=root/letter;build=json.loads(read(base/'build-public.json',16*1024*1024))
        public=json.loads(read(base/'package-public.json',16*1024*1024))
        binary=read(base/'cc-agent-identity',64*1024*1024)
        assert build['sourceBefore']==build['sourceAfter']and build['dependencySourceBefore']==build['dependencySourceAfter']
        assert build['target']=='aarch64-unknown-linux-gnu' and build['actualRuntimeVersion']is None and ('prometheus'in build)==(letter=='disk')
        assert build['supervisorRuntime']['sha256']==sha(runtime_pin)
        assert read(base/'supervisor-runtime.input.json',4096)==runtime_pin
        if letter=='disk':
            assert all(build['prometheus'][name]==disk_input[name]for name in('sha256','bytes','path'))
        assert build['runtimeVersionStamp']['runtimeVersion']==display and public['runtimeVersionStamp']==build['runtimeVersionStamp']
        h=sha(binary);assert h==build['binary']['sha256']
        if normal is None:normal=h;first=build
        else:assert normal==h and first['sourceBefore']==build['sourceBefore']and first['dependencySourceBefore']==build['dependencySourceBefore']
        raw=read(base/'signed/release-public-key.json',4096)
        if key is None:key=sha(raw)
        else:assert key==sha(raw)
        verifier=ReleaseVerifier(base/'cc-agent-identity',h,base/'signed/release-public-key.json',base/'signed/release-manifest.json',platform='linux-aarch64',current_version=number-1)
        assert verifier(base/'candidate.zip')is True
        entries.append(dict(sequence=number,version=display,artifactSha256=sha(read(base/'candidate.zip',export_limit(letter+'/candidate.zip'))),
            manifestSha256=sha(read(base/'signed/release-manifest.json',65536)),sourceSha256=public['inputSha256']))
    assert entries[0]['sourceSha256']['runtime/cc_agent/__init__.py']!=entries[1]['sourceSha256']['runtime/cc_agent/__init__.py']
    cargo_raw=read(root/'a/cargo-build.jsonl',16*1024*1024)
    assert cargo_raw==read(root/'b/cargo-build.jsonl',16*1024*1024)==read(root/'disk/cargo-build.jsonl',16*1024*1024)
    cargo=[json.loads(line)for line in cargo_raw.splitlines()if line.strip().startswith(b'{')]
    assert [r['success']for r in cargo if r.get('reason')=='build-finished']==[True]
    normal_rows=[r for r in cargo if r.get('reason')=='compiler-artifact'and r.get('executable')and r['target']['name']=='cc-agent-identity']
    assert len(normal_rows)==1 and normal_rows[0]['features']==[]and normal_rows[0]['profile']['test']is False
    assert normal_rows[0]['fresh']is False and normal_rows[0]['profile']['opt_level']=='3'
    assert Path(normal_rows[0]['executable'])==target_dir/'aarch64-unknown-linux-gnu/release/cc-agent-identity'
    assert int.from_bytes(read(root/'a/cc-agent-identity',64*1024*1024)[18:20],'little')==183
    # Keep canonical shipping/runtime source and actual compiled dependency
    # frames separately named. sourceFiles is a package-input compatibility map,
    # never a claim that every Python/license file was compiled into the ELF.
    source_files={}
    for name,digest in first['sourceBefore'].items():
        path=agent/name;raw=read(path,32*1024*1024);assert sha(raw)==digest
        source_files['agent/'+name]={'sha256':digest,'bytes':len(raw)}
    for name,row in first['dependencySourceBefore']['files'].items():
        assert name not in source_files;source_files[name]=row
    normal_receipt=dict(format='cc-normal-agent-as-built/v1',path='a/cc-agent-identity',sha256=normal,
        bytes=len(read(root/'a/cc-agent-identity',64*1024*1024)),target='aarch64-unknown-linux-gnu',features=[],
        profile=normal_rows[0]['profile'],fresh=normal_rows[0]['fresh'],cargoLogSha256=sha(cargo_raw),
        sourceFiles=source_files,sourceFilesScope='Canonical packaging inputs +separate original compiled dependency frame, NOT exact ELF linkage/compiler capture of Python.',
        shippingSourceBefore=first['sourceBefore'],shippingSourceAfter=first['sourceAfter'],
        compiledDependencySourceBefore=first['dependencySourceBefore'],compiledDependencySourceAfter=first['dependencySourceAfter'],
        actualRuntimeVersion=None,runtimeExecuted=False,signed=False,installed=False,legacyNineteenCaseProofClaimed=False,
        originalCanonicalResultSha256=json.loads(read(root/'RESUME_SOURCE.json'))['originalResultSha256']if(root/'RESUME_SOURCE.json').exists()else None,
        normalCompiledInThisRun=not(root/'RESUME_SOURCE.json').exists())
    write(root/'NORMAL_AS_BUILT.json',normal_receipt)
    source_frame=sha(json.dumps(source_files,sort_keys=True).encode())
    helper_log=root/'helpers-cargo-build.jsonl'
    separate=helper_log.exists()
    helper_raw=read(helper_log,16*1024*1024)if separate else cargo_raw
    helper_cargo=[json.loads(line)for line in helper_raw.splitlines()if line.strip().startswith(b'{')]
    assert [r['success']for r in helper_cargo if r.get('reason')=='build-finished']==[True]
    authorities={}
    for target,source_sha,folder,kind,scope in(
        ('synthetic_node_authority','7f403db17f36d26ad4c084b2dda0e794f7b7b6b81cb010f2de500241c766da02','actor-authority','cc-agent-install-actor-authority/v1','TelemetryRead default +explicit MetricsOnly/finite CollectorService typed-purpose methods'),
        ('synthetic_exporter_authority','755e16a4355a8606f653caa2fbfb99a30e28727e8d3770579239cdf91e597601','exporter-authority','cc-exporter-fixture-authority-as-built/v1','ExportersManage-only owned node_exporter fixture')):
        source=agent/'identity/examples'/(target+'.rs')
        assert sha(read(source,262144))==source_sha
        artifacts=[r for r in helper_cargo if r.get('reason')=='compiler-artifact'and r.get('executable')and r['target']['name']==target]
        assert len(artifacts)==1
        actual=artifacts[0]
        assert actual['target']['kind']==['example']and actual['profile']['test']is False and actual['profile']['opt_level']=='3'and actual['features']==[]
        assert not separate and actual['fresh']is False
        artifact=Path(actual['executable']);expected=target_dir/'aarch64-unknown-linux-gnu/release/examples'
        assert artifact==expected/target
        if separate:assert actual['fresh']is False
        saved=root/'compiled-helpers'/target
        if saved.exists():
            capture=json.loads(read(root/'compiled-helpers/receipt.json'));row=capture['artifacts'][target]
            assert capture['cargoLogSha256']==sha(helper_raw)and capture['separateCargo']==separate
            assert row['originalExecutable']==str(artifact)and row['sourceSha256']==source_sha
            body=read(saved,64*1024*1024);assert sha(body)==row['sha256']and len(body)==row['bytes']
        else:body=read(artifact,64*1024*1024)
        assert body[:6]==b'\x7fELF\x02\x01'and int.from_bytes(body[18:20],'little')==183
        directory=root/folder;directory.mkdir(mode=0o700);output=directory/target
        with output.open('xb')as f:f.write(body)
        output.chmod(0o700);assert output.stat().st_nlink==1 and sha(read(output,64*1024*1024))==sha(body)
        receipt=dict(format=kind,target=target,compiled=True,sourceUnchanged=sha(read(source,262144))==source_sha,
            sourceSha256=source_sha,normalBundleBuildSha256=sha(read(root/'a/build-public.json',16*1024*1024)),
            normalSha256=normal,normalReceiptSha256=sha(read(root/'NORMAL_AS_BUILT.json',16*1024*1024)),
            cargoLogSha256=sha(helper_raw),normalCargoLogSha256=sha(cargo_raw),sameCargoCommandAsNormal=not separate,
            cargoLogEvidencePath=str(root/'a/cargo-build.jsonl'),
            buildRelationship='separate actual Cargo command, matching current source frame'if separate else'same actual Cargo command',
            sha256=sha(body),bytes=len(body),profile=actual['profile'],features=[],fresh=actual['fresh'],
            sourceFiles=source_files,sourceFrameSha256=source_frame,scope=scope,runtimeExecuted=False,
            installed=False,releasePayload=False,compatibilityRuntimeObserved=False)
        assert receipt['sourceUnchanged']is True
        write(directory/'receipt.json',receipt);authorities[folder]=receipt
    value=dict(format='cc-canonical-agent-ram-fixture/v1',version=1,fixtureOnly=True,normalElfSha256=normal,
        publicKeySha256=key,fixtureAuthoritySha256=authorities['actor-authority']['sha256'],exporterAuthoritySha256=authorities['exporter-authority']['sha256'],
        installedLifecycleSourceInputsPresent=True,installedLifecycleRuntimeAccepted=False,packages=entries[:2],optionalDiskPackage=entries[2],
        syntheticReleaseSignerOnly=True,productionReleaseAuthorityClaimed=False,diskIncluded=False,optionalDiskPackageIncluded=True,
        runtimeExecuted=False,installed=False,actualRuntimeVersion=None,originalC267IsNotThisBuild=True)
    write(root/'fixture-packages.json',value)
    export=root.parent/'installed-export';export.mkdir(mode=0o700);total=0
    for name in EXPORT_FILES:
        body=read(root/name,export_limit(name));total+=len(body);assert total<=EXPORT_TOTAL
        target=export/name;target.parent.mkdir(parents=True,exist_ok=True)
        with target.open('xb')as f:f.write(body)
        assert target.stat().st_nlink==1 and sha(read(target,export_limit(name)))==sha(body)
    assert len(EXPORT_FILES)==34 and total<=512*1024*1024
    return value
