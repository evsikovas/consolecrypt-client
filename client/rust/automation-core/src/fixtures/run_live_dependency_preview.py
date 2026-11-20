"""Run a built actual-Core test with one owned offline helper/TLS fixture.

No Cargo/pulls/user profiles/WSL/default Docker changes or exposed host port.
"""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import uuid

IMAGE='sha256:4caa423c25f0ee4ba967d1b0fae556c6fbdc504c0c0f906477b415b396908a56'
LABEL='ans05-native-live-helper'
ENV=dict(os.environ,DOCKER_HOST='npipe:////./pipe/dockerDesktopLinuxEngine',DOCKER_CONTEXT='')
HERE=Path(__file__).resolve().parent
ROOT=next((parent for parent in HERE.parents if (parent/'automation-e1-next/runner/src').is_dir()),None)
assert ROOT is not None

def docker(*args,check=True):
    result=subprocess.run(['docker.exe',*args],env=ENV,capture_output=True,text=True,encoding='utf-8',timeout=30)
    if check and result.returncode:raise RuntimeError('Owned live fixture Docker operation failed')
    return result

def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--test-exe',type=Path,required=True)
    args=parser.parse_args();binary=args.test_exe.resolve(strict=True)
    assert binary.is_relative_to(ROOT) and re.fullmatch(r'cc_automation_core-[0-9a-f]+(?:\.exe)?',binary.name)
    assert json.loads(docker('image','inspect',IMAGE).stdout)[0]['Id']==IMAGE
    token=str(uuid.uuid4());name='cc-dependency-live-'+token;attempted=False;log=[]
    try:
        attempted=True
        docker('create','--pull=never','--name',name,'--label','consolecrypt.test='+LABEL,'--label','consolecrypt.fixture='+token,
            '--user','10001:10001','--read-only','--network','none','--memory','768m','--memory-swap','768m',
            '--cpus','2','--pids-limit','128','--cap-drop','ALL','--security-opt','no-new-privileges=true',
            '--security-opt','seccomp=unconfined','--security-opt','apparmor=unconfined',
            '--tmpfs','/tmp:rw,nosuid,nodev,size=384m,mode=1777',
            '--mount',f'type=bind,source={ROOT/"automation-e1-next/runner/src"},target=/opt/consolecrypt/src,readonly',
            '--mount',f'type=bind,source={ROOT/"automation-e1-next/runner/tests"},target=/opt/consolecrypt/tests,readonly',
            '--mount',f'type=bind,source={HERE},target=/fixture,readonly',
            '--entrypoint','/opt/cc-runtime/bin/python',IMAGE,'-I','-B','-c','import time;time.sleep(240)')
        docker('start',name)
        row=json.loads(docker('inspect',name).stdout)[0]
        assert row['HostConfig']['NetworkMode']=='none' and not row['HostConfig']['PortBindings']
        assert row['Config']['User']=='10001:10001' and row['HostConfig']['ReadonlyRootfs']
        env=dict(ENV,CC_DEPENDENCY_LIVE_CONTAINER=name,CC_DEPENDENCY_LIVE_TOKEN=token)
        run=subprocess.run([str(binary),'--ignored','--exact',
            'dependency_preview::tests::live::live_dependency_only_cli_tls_worker_and_native_prepare_preview',
            '--test-threads=1','--nocapture'],env=env,capture_output=True,text=True,encoding='utf-8',timeout=180)
        log=[f'Owned fixture {name}; pinned image {IMAGE}\n',
             'UID10001/readonly/network-none/no-host-port/768MiB/2CPU/128PID; namespaces permitted only here for real trusted worker.\n',
             run.stdout,run.stderr,f'ExitCode={run.returncode}\n']
        print(''.join(log),end='');assert run.returncode==0
    finally:
        if attempted:
            inspected=docker('inspect',name,check=False)
            if inspected.returncode==0:
                row=json.loads(inspected.stdout)[0]
                assert row['Config']['Labels']['consolecrypt.test']==LABEL and row['Config']['Labels']['consolecrypt.fixture']==token
                assert row['Config']['Image']==IMAGE
                docker('rm','--force',name);assert docker('inspect',name,check=False).returncode!=0
                log.append('Label/image verified; owned container removed; absence verified.\n')
        (ROOT/'automation-e1-next/evidence/live-native-dependency-preview.log').write_text(''.join(log),encoding='utf-8')

if __name__=='__main__':main()
