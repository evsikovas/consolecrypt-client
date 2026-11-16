"""Explicit public HTTPS source import; no repository code or credential access."""
import base64
import hashlib
import os
from pathlib import Path
import re
import shutil
import sys
import tempfile
import threading
import ssl
from urllib.parse import urlsplit

from .errors import RunnerError,require
from .protocol import decode,encode
from .sandbox import bubblewrap_argv,run_bounded
from .snapshot import relative_path,MAX_BYTES,MAX_FILES
from .store import atomic_json

GIT_IMPORT_GATE=threading.BoundedSemaphore(1)
GIT_METADATA='.consolecrypt-git-source.json'


def validate_source(url,reference):
    require(isinstance(url,str) and 0<len(url)<=2048 and '\\' not in url and not any(c.isspace() or ord(c)<32 for c in url),
            'git_url','Use an HTTPS repository URL without embedded credentials')
    try:
        parsed=urlsplit(url)
        valid=(parsed.scheme=='https' and parsed.hostname and parsed.username is None and parsed.password is None
               and (parsed.port is None or 1<=parsed.port<=65535) and not parsed.query and not parsed.fragment)
    except ValueError:valid=False
    require(valid,'git_url','Use an HTTPS repository URL without embedded credentials')
    require(isinstance(reference,str) and 0<len(reference)<=240 and not reference.startswith(('-', '/'))
            and re.fullmatch(r'[A-Za-z0-9_./-]+',reference) is not None and '..' not in reference
            and '//' not in reference and not reference.endswith(('/','.','.lock')),
            'git_reference','Select an explicit branch or tag')


class GitStreamReceiver:
    """Incremental bounded framing writes only a newly-created private tree."""
    def __init__(self,root,url,reference,ca_sha256=None):
        self.root=root;self.url=url;self.reference=reference
        self.ca_sha256=ca_sha256
        self.buffer=bytearray();self.source=None;self.complete=False
        self.output=None;self.hash=None;self.remaining=0;self.offset=0
        self.paths=set();self.files=0;self.bytes=0
        self.executable_paths=[]
    def close(self):
        if self.output is not None:self.output.close();self.output=None
    def feed(self,data):
        self.buffer.extend(data)
        require(len(self.buffer)<=1024*1024,'git_frame','Git import frame exceeds limit')
        while True:
            end=self.buffer.find(b'\n')
            if end<0:return
            raw=bytes(self.buffer[:end]);del self.buffer[:end+1]
            self.message(decode(raw))
    def message(self,value):
        require(isinstance(value,dict) and not self.complete,'git_frame','Invalid Git import frame')
        kind=value.get('type')
        if self.source is None:
            require(set(value)=={'type','url','reference','commit','ca_sha256'} and kind=='source' and value['url']==self.url and
                    value['reference']==self.reference and isinstance(value['commit'],str) and
                    value['ca_sha256']==self.ca_sha256 and
                    re.fullmatch(r'[0-9a-f]{40}|[0-9a-f]{64}',value['commit']) is not None,
                    'git_source','Git source differs from the selected repository')
            self.source={key:value[key] for key in ('url','reference','commit','ca_sha256')};return
        if kind in ('directory','file'):
            require(self.output is None and len(self.paths)<20000,'git_entry','Incomplete file or entry limit')
            path=relative_path(value.get('path'))
            require(all(part.casefold()!='.git' for part in path.parts) and str(path) not in self.paths,
                    'git_path','Duplicate or metadata project path')
            require(str(path)!=GIT_METADATA,'git_path','Repository uses the reserved ConsoleCrypt source metadata path')
            require(path.parent==Path('.') or path.parent.as_posix() in self.paths,
                    'git_path','Parent directory was not declared')
            target=self.root.joinpath(*path.parts)
            self.paths.add(str(path))
            if kind=='directory':
                require(set(value)=={'type','path'},'git_entry','Invalid directory')
                target.mkdir(mode=0o700);return
            require(set(value)=={'type','path','mode','bytes'} and type(value['mode']) is int and value['mode'] in (0o600,0o700)
                    and type(value['bytes']) is int and 0<=value['bytes']<=MAX_BYTES-self.bytes and self.files<MAX_FILES,
                    'git_size','Git project exceeds file or byte limit')
            self.files+=1;self.bytes+=value['bytes'];self.offset=0;self.remaining=value['bytes'];self.hash=hashlib.sha256()
            if value['mode']==0o700:self.executable_paths.append(str(path))
            self.output=os.fdopen(os.open(target,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,value['mode']),'wb');return
        if kind=='chunk':
            require(set(value)=={'type','offset','data'} and self.output is not None and type(value['offset']) is int
                    and value['offset']==self.offset and isinstance(value['data'],str) and len(value['data'])<=175000,
                    'git_chunk','Invalid Git content chunk')
            try:chunk=base64.b64decode(value['data'],validate=True)
            except ValueError:raise RunnerError('git_chunk','Invalid Git chunk encoding')
            require(0<len(chunk)<=min(128*1024,self.remaining),'git_size','Git content exceeds declared size')
            self.output.write(chunk);self.hash.update(chunk);self.offset+=len(chunk);self.remaining-=len(chunk);return
        if kind=='file_end':
            require(set(value)=={'type','sha256'} and self.output is not None and self.remaining==0 and
                    value['sha256']==self.hash.hexdigest(),'git_digest','Incomplete or changed Git content')
            self.output.flush();os.fsync(self.output.fileno());self.close();return
        require(kind=='complete' and set(value)=={'type','files','bytes'} and self.output is None and
                type(value['files']) is int and value['files']==self.files and type(value['bytes']) is int and
                value['bytes']==self.bytes,'git_complete','Git import did not complete')
        self.complete=True


def capture_git(repository,url,reference,cancel,*,ca_pem=None):
    validate_source(url,reference)
    require(not cancel.is_set(),'cancelled','Git import was cancelled')
    if ca_pem is not None:
        require(isinstance(ca_pem,str) and ca_pem.isascii() and 0<len(ca_pem)<=64*1024 and
                re.fullmatch(r'(?:-----BEGIN CERTIFICATE-----\s+[A-Za-z0-9+/=\r\n]+-----END CERTIFICATE-----\s*)+',ca_pem) is not None,
                'git_ca','Only public PEM certificates are accepted as custom trust roots')
        try:ssl.create_default_context(cadata=ca_pem)
        except (ssl.SSLError,ValueError):raise RunnerError('git_ca','Invalid public trust certificate')
    ca_digest=hashlib.sha256(ca_pem.encode()).hexdigest() if ca_pem is not None else None
    require(shutil.which('git') is not None,'git_missing','Git is not installed in the selected Runner')
    require(GIT_IMPORT_GATE.acquire(blocking=False),'git_busy','Another source import is running')
    try:
        # Only temporary and snapshot roots owned by this helper. Nothing is
        # written to a user checkout, global Git settings, home or credentials.
        with tempfile.TemporaryDirectory(prefix='git-import-',dir=repository.root.parent) as temporary:
            temporary=Path(temporary);project=temporary/'project';project.mkdir(mode=0o700)
            spec=temporary/'source.json';atomic_json(spec,{'url':url,'reference':reference,'custom_ca':ca_pem is not None,'ca_sha256':ca_digest})
            mounts=[(spec,'/tool/git-input.json')]
            if ca_pem is not None:
                certificate=temporary/'ca.pem';certificate.write_text(ca_pem,encoding='ascii');certificate.chmod(0o600)
                mounts.append((certificate,'/tool/git-ca.pem'))
            receiver=GitStreamReceiver(project,url,reference,ca_digest)
            try:
                result=run_bounded(bubblewrap_argv(project,[sys.executable,'-m','consolecrypt_runner.git_worker'],
                    extra_readonly=mounts,git_download=True),timeout=180,cancel=cancel,
                    grace=15,output_limit=0,on_chunk=receiver.feed)
                require(result.exit_code==0 and not result.cancelled and not result.timed_out and receiver.complete
                        and not receiver.buffer and not cancel.is_set(),'git_failed','Git import failed or was cancelled; no project was published')
                metadata={'version':1,'kind':'git',**receiver.source,'executable_paths':sorted(receiver.executable_paths)}
                atomic_json(project/GIT_METADATA,metadata)
                source=dict(receiver.source,metadata_sha256=hashlib.sha256(encode(metadata)).hexdigest())
                with repository.lock:
                    captured=repository.capture(project)
                    try:
                        # Metadata is outside executable project content; callers
                        # bind commit+snapshot in their immutable review/manifest.
                        atomic_json(repository.root/captured['revision_id']/'git.json',dict(source,revision_digest=captured['digest']))
                    except BaseException:
                        repository.release(captured['revision_id'],referenced=lambda *_:False)
                        raise
                    return dict(captured,source=source)
            finally:receiver.close()
    finally:GIT_IMPORT_GATE.release()
