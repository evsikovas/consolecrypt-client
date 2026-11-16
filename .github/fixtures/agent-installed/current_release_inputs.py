"""Fixed public input validation for the existing installed-Agent owner.
No subprocess, package construction, signer, service, Docker or VM invocation.
The normal receipt remains original; current runtime inputs have a separate map.
"""
from pathlib import Path, PurePosixPath
import hashlib,json,os,stat
NORMAL_SHA='ab02631816e1a019ed0b35bd8397626f02852a6990595983a4a091c5f187d27d'
NORMAL_BYTES=23993536
INPUT_SHA='7e3532eb98492de17c4e37147bc272e22c93ea9aa63c50072383aa13cee5e7fc'

EXPORT_FILES=(
    'fixture-packages.json','NORMAL_AS_BUILT.json',
    'actor-authority/receipt.json','actor-authority/synthetic_node_authority',
    'exporter-authority/receipt.json','exporter-authority/synthetic_exporter_authority',
    'a/cc-agent-identity',
    *tuple(part+'/'+name for part in('a','b','disk')for name in(
        'candidate.zip','build-public.json','package-public.json','source-before.json',
        'dependency-metadata-before.json','dependency-metadata-after.json','cargo-build.jsonl',
        'signed/release-manifest.json','signed/release-public-key.json')),
)
EXPORT_TOTAL=512*1024*1024

def export_limit(name):
    assert name in EXPORT_FILES
    if name=='disk/candidate.zip':return 96*1024*1024
    if name.endswith('candidate.zip'):return 32*1024*1024
    if name in('a/cc-agent-identity','actor-authority/synthetic_node_authority','exporter-authority/synthetic_exporter_authority'):return 64*1024*1024
    return 16*1024*1024


def safe(root,name):
    p=PurePosixPath(name)
    assert name and not p.is_absolute() and '..'not in p.parts and "\\"not in name
    value=Path(root).joinpath(*p.parts)
    assert value.is_relative_to(root)
    for parent in(value,*value.parents):
        info=parent.lstat()
        assert not stat.S_ISLNK(info.st_mode) and not getattr(info,'st_file_attributes',0)&0x400
        if parent==Path(root):break
    return value

def read(path,limit=16*1024*1024):
    path=Path(path);before=path.lstat()
    assert stat.S_ISREG(before.st_mode)and before.st_nlink==1 and not getattr(before,'st_file_attributes',0)&0x400
    assert 0<=before.st_size<=limit
    fd=os.open(path,os.O_RDONLY|getattr(os,'O_NOFOLLOW',0)|getattr(os,'O_BINARY',0))
    try:
        held=os.fstat(fd);assert(held.st_dev,held.st_ino)==(before.st_dev,before.st_ino)
        chunks=[];total=0
        while True:
            chunk=os.read(fd,min(1024*1024,limit+1-total))
            if not chunk:break
            chunks.append(chunk);total+=len(chunk);assert total<=limit
        after=os.fstat(fd);named=path.lstat()
        identity=lambda s:(s.st_dev,s.st_ino,s.st_size,s.st_mtime_ns,s.st_ctime_ns,s.st_nlink)
        assert identity(before)==identity(after)==identity(named)
        return b''.join(chunks)
    finally:os.close(fd)

def pin(root,row,limit=64*1024*1024):
    path=safe(root,row['path']);raw=read(path,limit)
    assert len(raw)==row['bytes']and hashlib.sha256(raw).hexdigest()==row['sha256'],row['path']
    return path,raw

def validate(root,agent,input_path,require_tools=True):
    root=Path(root);agent=Path(agent)
    raw=read(input_path);assert hashlib.sha256(raw).hexdigest()==INPUT_SHA
    spec=json.loads(raw);assert spec['format']=='cc-installed-agent-ram-inputs/v1'
    _,original=pin(root,spec['normalReceipt']);normal=json.loads(original)
    assert normal['format']=='cc-normal-agent-as-built/v1'and normal['sha256']==NORMAL_SHA and normal['bytes']==NORMAL_BYTES
    assert normal['features']==[]and normal['profile']['test']is False and normal['target']=='x86_64-unknown-linux-gnu'
    evidence={name:pin(root,row)[1]for name,row in spec['normalEvidence'].items()}
    result=json.loads(evidence['RESULT.json']);actual=json.loads(evidence['AGENT_RESULT.json'])
    assert all(result[k]is True for k in('passed','sourceUnchanged','removed','volumeRemoved'))
    assert actual['passed']is True and actual['normal']['sha256']==NORMAL_SHA and actual['normal']['bytes']==NORMAL_BYTES
    captured=json.loads(evidence['source-before.json']);assert len(captured)==229
    assert {n:r['sha256']for n,r in normal['sourceFiles'].items()}==captured
    # C267 remains its original accepted OLD-cap compiler frame. The new
    # canonical pass must capture current before/after; it never reuses this ELF.
    historical_changes=[]
    for name,digest in captured.items():
        payload=read(safe(root/'client-automation-monitoring',name),16*1024*1024)
        if hashlib.sha256(payload).hexdigest()!=digest:historical_changes.append(name)
    binary,payload=pin(root,spec['normalBinary']);assert payload[:6]==b'\x7fELF\x02\x01'and int.from_bytes(payload[18:20],'little')==62
    assert hashlib.sha256(payload).hexdigest()==NORMAL_SHA and len(payload)==NORMAL_BYTES
    cargo=[json.loads(line)for line in evidence['normal-agent-build.json'].splitlines()if line.strip().startswith(b'{')]
    built=[r for r in cargo if r.get('reason')=='compiler-artifact'and r.get('executable')and r['target']['name']=='cc-agent-identity']
    assert len(built)==1 and built[0]['features']==[]and built[0]['profile']['test']is False
    rebind=spec['runtimeInputRepin'];assert rebind['commit']=='73579aafebe82a3873530e47ac6a9035042763e9'
    _,delta_raw=pin(root,rebind['trackedChanges']);delta=json.loads(delta_raw);assert len(delta['files'])==13
    for row in delta['files']:
        payload=read(safe(root,row['target']),16*1024*1024)
        assert len(payload)==row['bytes']and hashlib.sha256(payload).hexdigest()==row['sha256']
    reviewed=spec['reviewedSupervisorRuntime']
    _,pin_raw=pin(root,reviewed['pin']);runtime_pin=json.loads(pin_raw)
    assert runtime_pin=={'format':'cc-tunnel-supervisor-runtime/v1','pythonExecutable':'/usr/bin/python3.12','pythonSha256':'e50d468e8b0adfb05733f5b87b3cff34829c4a8c1aea50c865aa8bdfe4bb150f'}
    _,tools_raw=pin(root,reviewed['toolsProof']);tools=json.loads(tools_raw)
    assert tools['passed']is True and tools['tools']['/usr/bin/python3.12']=={'bytes':8020928,'mode':'0o755','nlink':1,'sha256':runtime_pin['pythonSha256'],'uid':0}
    _,image_raw=pin(root,reviewed['imageProof'])
    assert json.loads(image_raw)['sshImage']=='sha256:68c3ce7f0afca48f6edae37e69dedaecd1e577ca8c4d69cf9e1416106ba32c00'
    runtime=spec['currentRuntimeSourceMap'];assert runtime and len(runtime)<=512
    for name,row in runtime.items():
        payload=read(safe(agent,name),16*1024*1024)
        assert len(payload)==row['bytes']and hashlib.sha256(payload).hexdigest()==row['sha256'],'runtime input changed: '+name
    assert sum(r['bytes']for r in runtime.values())<=64*1024*1024
    vmagent,_=pin(root,spec['vmagent']);_,materials=pin(root,spec['materials'])
    _,receipt=pin(root,spec['materialReceipt']);_,outcome=pin(root,spec['materialResult'])
    assert json.loads(receipt)['materialsSha256']==spec['materials']['sha256']
    assert json.loads(receipt)['binarySha256']==spec['vmagent']['sha256']and json.loads(outcome)['cleanup']is True
    for source in spec['fixtureSources'].values():pin(root,source)
    # Original c267 never captured dependency metadata/before+after. Do not
    # retrofit it. Canonical RAM packaging must record those around ONE build.
    missing=['Actual ONE canonical build/package+metadata/verifier run remains UNRUN',
             'Same-pass fullActor+Exporter authority artifact/compatibility observations remain UNRUN']
    if require_tools:assert not missing,'; '.join(missing)
    return dict(spec=spec,normal=normal,binary=binary,vmagent=vmagent,materials=materials,missing=missing,
        compilerSources=captured,historicalCompilerDifferences=historical_changes,normalReusedInCanonicalPass=False,currentRuntimeSourceMap=runtime,originalReceipt=original)
