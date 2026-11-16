"""Native ARM entry for the existing canonical packet build/seal owner.

Source only until reviewed CI execution. No services, production signer/key,
push or publication. Existing pipeline owns source/material/package checks.
"""
import argparse
import ctypes
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys
import time

HERE = Path(__file__).resolve().parent
TARGET = 'aarch64-unknown-linux-gnu'
# Reviewed collector afterimage in ARM_CI_FIX.json (644c3810397381c8...).
# Main's parser-only ARM branch cannot consume the native official-Go record.
COLLECTOR_MATERIALS_SHA256 = 'f529131dfd912f5cb339e9251a65008116094f591b86d32d1b9157d5ea30570f'
FIXTURES = {
    'synthetic_node_authority': '7f403db17f36d26ad4c084b2dda0e794f7b7b6b81cb010f2de500241c766da02',
    'synthetic_exporter_authority': '755e16a4355a8606f653caa2fbfb99a30e28727e8d3770579239cdf91e597601',
    'synthetic_release_fixture': '9a1885366921b4ef18f291fa4729a39f22af1b34fb676bf1a2dcdb6c87763f63',
}


def fixture_sources(agent, paths, read, digest):
    assert agent.resolve(strict=True) == agent, 'physical Agent shipping root required'
    sources = {}
    for name,path in zip(FIXTURES,paths):
        path = path.absolute()
        assert path.resolve(strict=True) == path, 'physical fixture source required'
        assert not path.is_relative_to(agent), 'fixture source must be outside Agent shipping root'
        raw = read(path,262144)
        assert digest(raw) == FIXTURES[name], 'reviewed fixture source hash required'
        sources[name] = raw
    assert set(sources) == set(FIXTURES), 'all three fixture sources required'
    return sources


def compiler_input(path, expected, role, phase, observations, row):
    """Only this joined Cargo command's fixed outputs may have one Cargo alias."""
    record=dict(role=role,phase=phase,passed=False,cargoFresh=row['fresh'],
                features=row['features'],optLevel=row['profile']['opt_level'],targetKind=row['target']['kind'])
    observations.append(record)
    fd=None
    identity=lambda m:(m.st_dev,m.st_ino,m.st_mode,m.st_uid,m.st_gid,m.st_nlink,
                       m.st_size,m.st_mtime_ns,m.st_ctime_ns)
    try:
        assert role in (*FIXTURES,'cc-agent-identity','cc-agent-release-sign')
        assert path==expected and path.is_absolute()
        for parent in (path,*path.parents):
            info=parent.lstat()
            assert not stat.S_ISLNK(info.st_mode)and not getattr(info,'st_file_attributes',0)&0x400
        named=path.lstat()
        fd=os.open(path,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK|os.O_CLOEXEC)
        before=os.fstat(fd)
        record.update(regular=stat.S_ISREG(before.st_mode),links=before.st_nlink,
                      bytes=before.st_size,ownerMatches=before.st_uid==os.geteuid())
        assert identity(before)==identity(named)
        assert stat.S_ISREG(before.st_mode)and before.st_uid==os.geteuid()
        assert before.st_nlink in (1,2)and 64<=before.st_size<=64*1024**2
        assert before.st_mode&stat.S_IXUSR and not before.st_mode&0o022
        alias=None
        if before.st_nlink==2:
            directory=path.parent if role in FIXTURES else path.parent/'deps'
            assert directory.resolve(strict=True)==directory and stat.S_ISDIR(directory.lstat().st_mode)
            pattern=re.compile(re.escape(role.replace('-','_'))+'-[0-9a-f]{16}')
            matches=[];count=0
            with os.scandir(directory)as entries:
                for entry in entries:
                    count+=1;assert count<=8192
                    if not pattern.fullmatch(entry.name):continue
                    candidate=directory/entry.name;metadata=candidate.lstat()
                    if (metadata.st_dev,metadata.st_ino)==(before.st_dev,before.st_ino):
                        assert identity(metadata)==identity(before)
                        matches.append(candidate)
            assert len(matches)==1,'original Cargo hardlink alias must be inside its private target'
            alias=matches[0]
        chunks=[];used=0;digest=hashlib.sha256()
        while True:
            block=os.read(fd,min(65536,before.st_size+1-used))
            if not block:break
            used+=len(block);assert used<=before.st_size
            chunks.append(block);digest.update(block)
        assert used==before.st_size
        assert identity(before)==identity(os.fstat(fd))==identity(path.lstat())
        if alias is not None:assert identity(alias.lstat())==identity(before)
        record.update(sha256=digest.hexdigest(),cargoAliasVerified=alias is not None,passed=True)
        return b''.join(chunks),identity(before)
    except BaseException as error:
        record['errorKind']=type(error).__name__
        raise
    finally:
        if fd is not None:os.close(fd)


def build(args):
    assert sys.platform == 'linux' and os.uname().machine == 'aarch64' and os.getuid() == os.geteuid() != 0
    assert os.environ.get('GITHUB_ACTIONS') == 'true' and os.environ.get('RUNNER_ARCH') == 'ARM64'
    assert os.environ.get('RUNNER_ENVIRONMENT') == 'github-hosted'
    agent,output = args.agent.absolute(),args.output.absolute()
    assert agent == Path(os.environ['GITHUB_WORKSPACE']).absolute()/'agent'
    assert agent.parent.joinpath('.git').is_dir() and not agent.parent.joinpath('.git').is_symlink()
    assert output.is_relative_to(Path(os.environ['RUNNER_TEMP']).absolute()) and not output.exists()
    assert output.parent.resolve(strict=True) == output.parent
    assert re.fullmatch('[0-9a-f]{40}',args.source_commit)
    commit = subprocess.check_output(['git','-C',str(agent.parent),'rev-parse','HEAD'],timeout=10).decode().strip()
    assert commit == args.source_commit
    sys.path[:0] = [str(agent/'delivery'),str(HERE)]
    from arm64_host import command,read,digest
    # Refuse unpromoted prerequisites before imports, source capture, output,
    # Git exclusions or compilation. Original fixture inputs must never enter
    # the shipping frame or its corresponding-source archive.
    assert digest(read(agent/'delivery/collector_materials.py',128*1024)) == COLLECTOR_MATERIALS_SHA256, \
        'reviewed ARM_CI_FIX collector_materials prerequisite required'
    fixture_bytes = fixture_sources(agent,
        (args.actor_source,args.exporter_source,args.synthetic_signer_source),read,digest)
    import release_pipeline as pipeline
    import prototype_package
    # Independent public pin, never measured/synthesized from this build host.
    pin_raw = read(args.runtime_pin.absolute(),4096)
    assert digest(pin_raw) == args.runtime_pin_sha256
    pin = json.loads(prototype_package.public_runtime_pin(args.runtime_pin.absolute()))
    assert pin['pythonExecutable'] == '/usr/bin/python3.12'
    python = read(Path(pin['pythonExecutable']),32*1024**2)
    assert digest(python) == pin['pythonSha256'] and int.from_bytes(python[18:20],'little') == 183
    # Original native producer validation, including actual ELF/material data.
    pipeline.disk_input(args.disk_input.absolute(),TARGET)
    original_frame = pipeline.source_map()
    assert ctypes.CDLL(None,use_errno=True).prctl(36,1,0,0,0) == 0
    examples = agent/'identity/examples'
    assert not examples.exists() and not examples.is_symlink()
    output.mkdir(mode=0o700)
    # Exactly the existing owner's independent fixture exclusion policy, in a
    # disposable checkout only. No broad pattern or shipping-source waiver.
    exclude = agent.parent/'.git/info/exclude'
    previous = read(exclude,16384) if exclude.exists() and exclude.stat().st_size else b''
    with exclude.open('ab') as stream:
        stream.write(b'\n'+''.join('/agent/identity/examples/'+name+'.rs\n' for name in FIXTURES).encode())
    examples.mkdir(mode=0o755)
    for name,raw in fixture_bytes.items():
        with (examples/(name+'.rs')).open('xb') as stream:
            stream.write(raw)
    assert pipeline.source_map() == original_frame
    (output/'fixture-sources.json').write_text(json.dumps({name:dict(sha256=digest(raw),bytes=len(raw),
        cargoSource=str(examples/(name+'.rs'))) for name,raw in fixture_bytes.items()},sort_keys=True)+'\n')
    target = output/'native-target'
    assert not target.exists()
    environment = dict(os.environ,CARGO_TARGET_DIR=str(target),CARGO_BUILD_JOBS='1',CARGO_INCREMENTAL='0',
        CARGO_NET_OFFLINE='true',RUSTUP_TOOLCHAIN='1.98.1',CARGO_PROFILE_RELEASE_DEBUG='0',CC_AGENT_RELEASE_APPROVED_FLOOR='0')
    os.environ.update({k:v for k,v in environment.items() if k.startswith(('CARGO_','RUSTUP_','CC_AGENT_RELEASE_'))})
    root = output/'agent-ram-canonical'
    root.mkdir(mode=0o700)
    first = root/'a'
    pipeline.before(first,TARGET)
    argv = ['cargo','build','--release','--offline','--locked','--manifest-path',agent/'identity/Cargo.toml',
        '--target',TARGET,'-j1','--no-default-features','--bin','cc-agent-identity','--bin','cc-agent-release-sign']
    for name in FIXTURES:
        argv += ['--example',name]
    # This is the one actual build log, including unchanged Cargo diagnostics.
    # Every compiler-artifact and successful build-finished must come from it.
    observations=[];compiler_passed=False;cargo_raw=None;cargo_finished=False;bin_tests_passed=False
    try:
        command([*argv,'--message-format=json'],first/'cargo-build.jsonl',900,environment)
        cargo_raw=read(first/'cargo-build.jsonl')
        cargo = [json.loads(line) for line in cargo_raw.splitlines() if line.strip().startswith(b'{')]
        assert [row['success'] for row in cargo if row.get('reason') == 'build-finished'] == [True]
        cargo_finished=True
        copied=output/'compiler-binaries';copied.mkdir(mode=0o700)
        compiled = {};originals = {};compiled_fingerprints = {};cargo_rows={}
        for name in (*FIXTURES,'cc-agent-identity','cc-agent-release-sign'):
            rows = [row for row in cargo if row.get('reason') == 'compiler-artifact'
                    and row.get('executable') and row['target']['name'] == name]
            assert len(rows) == 1
            row = rows[0]
            assert row['features'] == [] and row['fresh'] is False and row['profile']['test'] is False
            assert row['profile']['opt_level'] == '3'
            path = Path(row['executable'])
            expected = target/TARGET/'release'/('examples/'+name if name in FIXTURES else name)
            assert path == expected and row['filenames']==[str(expected)]
            if name in FIXTURES:
                assert row['target']['kind'] == ['example'] and row['target']['src_path'] == str(examples/(name+'.rs'))
                assert digest(read(examples/(name+'.rs'),262144)) == FIXTURES[name]
            else:
                assert row['target']['kind']==['bin']and row['target']['src_path']==str(agent/'identity/src/bin'/(name+'.rs'))
            raw,stamp=compiler_input(path,expected,name,'capture',observations,row)
            assert raw[:7] == b'\x7fELF\x02\x01\x01' and int.from_bytes(raw[18:20],'little') == 183
            destination=copied/name
            with destination.open('xb')as stream:stream.write(raw)
            destination.chmod(0o700)
            assert read(destination,64*1024**2)==raw
            compiled[name] = destination
            originals[name] = (path,stamp)
            cargo_rows[name] = row
            compiled_fingerprints[name] = (digest(raw),len(raw))
        command(['cargo','test','--release','--offline','--locked','--manifest-path',agent/'identity/Cargo.toml',
            '--target',TARGET,'-j1','--no-default-features','--bins'],output/'native-rust-tests.log',600,environment)
        bin_tests_passed=True
        for name,path in compiled.items():
            raw = read(path,64*1024**2)
            assert (digest(raw),len(raw)) == compiled_fingerprints[name]
            original,stamp=originals[name]
            source,current=compiler_input(original,original,name,'after-bin-tests',observations,cargo_rows[name])
            assert current==stamp and source==raw
        compiler_passed=True
    finally:
        report=dict(format='cc-arm64-compiler-capture/v1',passed=compiler_passed,
                    actualOneCargoBuild=True,cargoBuildFinished=cargo_finished,nativeBinTestsPassed=bin_tests_passed,
                    cargoLogSha256=None if cargo_raw is None else digest(cargo_raw),artifacts=observations)
        encoded=(json.dumps(report,sort_keys=True)+'\n').encode();assert len(encoded)<=65536
        with(output/'COMPILER_RESULT.json').open('xb')as stream:stream.write(encoded)
    for number,letter in enumerate(('a','b','disk'),1):
        work = root/letter
        if number != 1:
            work.mkdir(mode=0o700)
            for name in ('source-before.json','dependency-metadata-before.json','cargo-build.jsonl'):
                shutil.copyfile(first/name,work/name)
        os.environ.update(CC_RELEASE_VERSION=str(number),CC_RELEASE_DISPLAY_VERSION='0.1.'+str(number-1),
                          CC_AGENT_RELEASE_APPROVED_FLOOR=str(number-1))
        pipeline.package(work,str(number),'0.1.'+str(number-1),str(number-1),args.vmagent_binary.absolute(),
            args.collector_materials.absolute(),args.disk_input.absolute() if number == 3 else None,args.runtime_pin.absolute())
        signed = work/'signed'
        signed.mkdir(mode=0o700)
        command([compiled['synthetic_release_fixture'],'--out',signed,'--artifact',work/'candidate.zip',
            '--now',str(int(time.time())),'--version',str(number),'--display-version','0.1.'+str(number-1),
            '--platform','linux-aarch64'],output/('fixture-sign-%d.log'%number),20,environment)
    sealer = HERE/'seal_ram_bundle.arm64.py'
    spec = importlib.util.spec_from_file_location('same_native_arm_sealer',sealer)
    module = importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
    value = module.seal(root,agent,args.disk_input.absolute(),target,args.runtime_pin.absolute())
    # Bind the actual sealed package/authority bytes back to the held Cargo
    # captures; package/sealer retain their existing readers and schemas.
    for name,path in compiled.items():
        raw=read(path,64*1024**2)
        assert (digest(raw),len(raw))==compiled_fingerprints[name]
    for letter in ('a','b','disk'):
        for name in ('cc-agent-identity','cc-agent-release-sign'):
            raw=read(root/letter/name,64*1024**2)
            assert (digest(raw),len(raw))==compiled_fingerprints[name]
    for name,folder in (('synthetic_node_authority','actor-authority'),
                        ('synthetic_exporter_authority','exporter-authority')):
        raw=read(root/folder/name,64*1024**2)
        receipt=json.loads(read(root/folder/'receipt.json',16*1024**2))
        fingerprint=compiled_fingerprints[name]
        assert (digest(raw),len(raw))==fingerprint
        assert (receipt['sha256'],receipt['bytes'])==fingerprint
        assert receipt['sourceSha256']==FIXTURES[name]and receipt['cargoLogSha256']==digest(cargo_raw)
    assert pipeline.source_map() == original_frame
    result = dict(passed=True,packetPrepared=True,installedAcceptanceExecuted=False,productionReleaseClaimed=False,
        nativeArchitecture='aarch64',sourceCommit=commit,normalElfSha256=value['normalElfSha256'],
        packetSha256=digest(read(root/'fixture-packages.json',1024**2)),fixtureSources=FIXTURES,
        actualOneCargoBuild=True,nativeBinTestsExecuted=True,normalOrHelpersReused=False)
    (output/'BUILD_RESULT.json').write_text(json.dumps(result,sort_keys=True,indent=2)+'\n')
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--execute-native-arm-build',action='store_true',required=True)
    for name in ('agent','output','runtime-pin','disk-input','vmagent-binary','collector-materials',
                 'actor-source','exporter-source','synthetic-signer-source'):
        parser.add_argument('--'+name,type=Path,required=True)
    for name in ('source-commit','runtime-pin-sha256'):
        parser.add_argument('--'+name,required=True)
    print(json.dumps(build(parser.parse_args()),sort_keys=True))


if __name__ == '__main__':
    main()
