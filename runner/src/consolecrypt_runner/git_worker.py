"""Trusted Git reader inside a disposable namespace, never a project checkout."""
import base64
import hashlib
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys

from .errors import require
from .protocol import decode, encode
from .snapshot import relative_path, MAX_FILES, MAX_BYTES


def emit(value):
    sys.stdout.buffer.write(encode(value)+b'\n')
    sys.stdout.buffer.flush()


def main():
    spec=decode(Path('/tool/git-input.json').read_bytes())
    git=shutil.which('git')
    require(git is not None,'git_missing','Git is not installed in this Runner')
    Path('/work/home').mkdir(mode=0o700,exist_ok=True)
    Path('/work/no-hooks').mkdir(mode=0o700)
    # These are separate argv entries. No shell, inherited home, agent, credential
    # helper, global/system config, submodule update, checkout filter or hooks.
    options=[git,'--no-pager','-c','protocol.allow=never','-c','protocol.https.allow=always','-c','protocol.http.allow=always',
        '-c','credential.helper=','-c','core.hooksPath=/work/no-hooks','-c','core.fsmonitor=false',
        '-c','http.followRedirects=false','-c','http.extraHeader=','-c','http.lowSpeedLimit=1',
        '-c','http.lowSpeedTime=15','-c','transfer.fsckObjects=true','-c','fetch.fsckObjects=true']
    env={'PATH':'/usr/bin:/bin','HOME':'/work/home','LANG':'C.UTF-8','LC_ALL':'C.UTF-8',
         'GIT_CONFIG_NOSYSTEM':'1','GIT_CONFIG_GLOBAL':'/dev/null','GIT_ATTR_NOSYSTEM':'1',
         'GIT_TERMINAL_PROMPT':'0','GIT_ASKPASS':'','GIT_NO_REPLACE_OBJECTS':'1',
         'GIT_ALLOW_PROTOCOL':'http:https','GIT_SSH_COMMAND':'/usr/bin/false'}
    if spec['custom_ca']:env['GIT_SSL_CAINFO']='/tool/git-ca.pem'

    def read(args,limit):
        with subprocess.Popen(options+args,env=env,stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,
                              stderr=subprocess.DEVNULL) as child:
            try:
                data=child.stdout.read(limit+1)
                require(len(data)<=limit,'git_size','Git metadata exceeds limit')
                require(child.wait()==0,'git_failed','Git could not read the selected revision')
                return data
            finally:
                if child.poll() is None:child.kill();child.wait()

    read(['clone','--bare','--no-tags','--depth=1','--single-branch','--template=',
          '--branch',spec['reference'],'--',spec['url'],'/work/repository'],4096)
    options+=['--git-dir=/work/repository']
    commit=read(['rev-parse','--verify','HEAD^{commit}'],128).strip().decode('ascii')
    require(re.fullmatch(r'[0-9a-f]{40}|[0-9a-f]{64}',commit) is not None,'git_commit','Invalid commit')
    listing=read(['ls-tree','-r','-t','-l','-z',commit],32*1024*1024)
    entries=[];byte_count=0;files=0
    for raw in listing.split(b'\0'):
        if not raw:continue
        require(len(entries)<20000,'git_entries','Git tree exceeds entry limit')
        metadata,name=raw.split(b'\t',1)
        mode,kind,oid,size=metadata.split()
        path=relative_path(name.decode('utf-8')).as_posix()
        require(all(part.casefold()!='.git' for part in path.split('/')),'git_path','Git metadata is not project content')
        require(re.fullmatch(rb'[0-9a-f]{40}|[0-9a-f]{64}',oid) is not None,'git_object','Invalid object')
        if mode==b'040000' and kind==b'tree':
            entries.append({'type':'directory','path':path})
        else:
            require(mode in (b'100644',b'100755') and kind==b'blob','git_entry',
                    'Symlinks and submodules require a separately reviewed dependency')
            length=int(size);files+=1;byte_count+=length
            require(0<=length<=MAX_BYTES and files<=MAX_FILES and byte_count<=MAX_BYTES,
                    'git_size','Git project exceeds file or byte limit')
            entries.append({'type':'file','path':path,'mode':0o700 if mode==b'100755' else 0o600,
                            'bytes':length,'object':oid.decode('ascii')})
    emit({'type':'source','url':spec['url'],'reference':spec['reference'],'commit':commit,'ca_sha256':spec['ca_sha256']})
    with subprocess.Popen(options+['cat-file','--batch'],env=env,stdin=subprocess.PIPE,
                          stdout=subprocess.PIPE,stderr=subprocess.DEVNULL) as child:
        try:
            for entry in entries:
                if entry['type']=='directory':emit(entry);continue
                child.stdin.write(entry['object'].encode()+b'\n');child.stdin.flush()
                header=child.stdout.readline(256).strip().split()
                require(header==[entry['object'].encode(),b'blob',str(entry['bytes']).encode()],
                        'git_object','Git object differs from its tree entry')
                emit({key:value for key,value in entry.items() if key!='object'})
                remaining=entry['bytes'];offset=0;sha=hashlib.sha256()
                while remaining:
                    chunk=child.stdout.read(min(128*1024,remaining))
                    require(bool(chunk),'git_object','Incomplete Git object')
                    sha.update(chunk)
                    emit({'type':'chunk','offset':offset,'data':base64.b64encode(chunk).decode('ascii')})
                    remaining-=len(chunk);offset+=len(chunk)
                require(child.stdout.read(1)==b'\n','git_object','Invalid object boundary')
                emit({'type':'file_end','sha256':sha.hexdigest()})
            child.stdin.close()
            require(child.wait()==0,'git_failed','Git reader did not finish')
        finally:
            if child.poll() is None:child.kill();child.wait()
    emit({'type':'complete','files':files,'bytes':byte_count})


if __name__=='__main__':
    try:main()
    except Exception:
        # Git stderr/URLs/prompts never become protocol diagnostics.
        raise SystemExit(70)
