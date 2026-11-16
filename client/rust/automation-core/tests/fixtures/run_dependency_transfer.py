"""Run reviewed built Core test against one owned private-stdio Helper fixture.

No Cargo, pulls, user WSL/profile, network service or public activation.
"""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import uuid

ROOT=Path(__file__).resolve().parents[6]
IMAGE='sha256:4caa423c25f0ee4ba967d1b0fae556c6fbdc504c0c0f906477b415b396908a56'
ENV=dict(os.environ,DOCKER_HOST='npipe:////./pipe/dockerDesktopLinuxEngine',DOCKER_CONTEXT='')
LABEL='ans05-core-transfer'
def docker(*args,check=True):
    result=subprocess.run(['docker.exe',*args],env=ENV,capture_output=True,text=True,encoding='utf-8',timeout=30)
    if check and result.returncode:raise RuntimeError('owned fixture Docker operation failed')
    return result
def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--test-exe',type=Path,required=True)
    args=parser.parse_args();binary=args.test_exe.resolve(strict=True)
    assert binary.is_relative_to(ROOT) and re.fullmatch(r'dependency_transfer-[0-9a-f]+(?:\.exe)?',binary.name)
    assert json.loads(docker('image','inspect',IMAGE).stdout)[0]['Id']==IMAGE
    token=str(uuid.uuid4());name='cc-ans05-core-'+token;created=False
    fixtures=Path(__file__).resolve().parent
    source=ROOT/'automation-e1-next/runner/src'
    try:
        docker('create','--pull=never','--name',name,'--label','consolecrypt.test='+LABEL,'--label','consolecrypt.fixture='+token,
               '--user','10001:10001','--read-only','--network','none','--memory','384m','--memory-swap','384m','--cpus','1',
               '--pids-limit','64','--cap-drop','ALL','--security-opt','no-new-privileges=true',
               '--tmpfs','/tmp:rw,nosuid,nodev,size=268435456,mode=1777',
               '--mount',f'type=bind,source={source},target=/opt/consolecrypt/src,readonly',
               '--mount',f'type=bind,source={fixtures},target=/fixture,readonly',
               '--entrypoint','/opt/cc-runtime/bin/python',IMAGE,'-I','-c','import time;time.sleep(180)')
        created=True;docker('start',name)
        env=dict(ENV,CC_DEPENDENCY_HELPER_CONTAINER=name,CC_DEPENDENCY_HELPER_TOKEN=token)
        result=subprocess.run([str(binary),'--ignored','--exact','live_private_python_helper_and_default_gate',
                               '--test-threads=1','--nocapture'],env=env,capture_output=True,text=True,encoding='utf-8',timeout=120)
        print(result.stdout,end='');print(result.stderr,end='');assert result.returncode==0
    finally:
        if created:
            inspected=docker('inspect',name,check=False)
            if inspected.returncode==0:
                row=json.loads(inspected.stdout)[0]
                assert row['Config']['Labels'].get('consolecrypt.test')==LABEL and row['Config']['Labels'].get('consolecrypt.fixture')==token
                docker('rm','--force',name)
                print('Owned labelled private-stdio fixture removed; no public activation.')
if __name__=='__main__':main()
