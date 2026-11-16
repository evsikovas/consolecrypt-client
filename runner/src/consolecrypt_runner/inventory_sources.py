"""No-credential source normalization, with bounded immutable chunk transfer."""
import base64
import hashlib
import os
from pathlib import Path
import re
import shutil
import stat
import sys
import tempfile

from .errors import RunnerError,require
from .inventory import MAX_INVENTORY_BYTES,StaticInventory
from .inventory_worker import validate_sources
from .protocol import decode,encode,uuid_field
from .sandbox import bubblewrap_argv,run_bounded
from .store import _private_directory,atomic_json

MAX_CACHE_BYTES=32*1024*1024
MAX_REVISION_BYTES=12*1024*1024
MAX_CAPTURES=8


class InventoryReceiver:
    def __init__(self):
        self.buffer=bytearray();self.body=bytearray();self.expected=None;self.complete=False
    def feed(self,chunk):
        require(len(self.buffer)+len(chunk)<=1024*1024,'inventory_frame','Inventory worker frame exceeds its limit')
        self.buffer.extend(chunk)
        while (end:=self.buffer.find(b'\n'))>=0:
            value=decode(bytes(self.buffer[:end]));del self.buffer[:end+1]
            require(isinstance(value,dict) and not self.complete,'inventory_frame','Unexpected inventory worker output')
            if value.get('type')=='error':
                require(set(value)=={'type','code','message'} and isinstance(value['code'],str) and
                        re.fullmatch(r'[a-z0-9_]{1,80}',value['code']) is not None and isinstance(value['message'],str)
                        and len(value['message'])<=1024,'inventory_frame','Invalid inventory worker error')
                raise RunnerError(value['code'],value['message'])
            if self.expected is None:
                require(set(value)=={'type','bytes'} and value['type']=='inventory' and type(value['bytes']) is int
                    and 0<value['bytes']<=MAX_INVENTORY_BYTES,'inventory_size','Invalid normalized inventory size')
                self.expected=value['bytes'];continue
            if value.get('type')=='chunk':
                require(set(value)=={'type','offset','data'} and type(value['offset']) is int and value['offset']==len(self.body)
                    and isinstance(value['data'],str) and len(value['data'])<=175000,'inventory_chunk','Invalid inventory chunk')
                try:body=base64.b64decode(value['data'],validate=True)
                except ValueError:raise RunnerError('inventory_chunk','Invalid inventory encoding')
                require(0<len(body)<=128*1024 and len(self.body)+len(body)<=self.expected,
                        'inventory_size','Inventory body exceeds its declared size')
                self.body.extend(body);continue
            require(set(value)=={'type','sha256'} and value['type']=='complete' and len(self.body)==self.expected
                    and value['sha256']==hashlib.sha256(self.body).hexdigest(),
                    'inventory_changed','Inventory worker output is incomplete or changed')
            self.complete=True


def _cache_usage(repository):
    total=0
    for revision in repository.root.iterdir():
        uuid_field(revision.name,'revision ID');_private_directory(revision)
        directory=revision/'inventory-previews'
        if not directory.exists() and not directory.is_symlink():continue
        _private_directory(directory)
        for path in directory.iterdir():
            require(re.fullmatch(r'[0-9a-f]{64}\.json',path.name) is not None,
                    'inventory_storage','Unexpected inventory artifact')
            info=path.lstat()
            require(stat.S_ISREG(info.st_mode) and info.st_nlink==1 and info.st_uid==os.getuid(),
                    'inventory_storage','Invalid inventory artifact')
            total+=info.st_size
    return total


def capture_inventory(repository,revision_id,sources,cancel):
    sources=validate_sources(sources)
    root,snapshot=repository.read(revision_id)
    files={entry['path']:entry for entry in snapshot.files}
    selected={}
    for source in sources:
        entry=files.get(source['path'])
        require(entry is not None and entry['mode']==0o600,'inventory_source','Select a captured non-executable inventory file')
        selected[source['path']]={'path':entry['path'],'bytes':entry['bytes'],'sha256':entry['sha256']}
    require(sum(entry['bytes'] for entry in selected.values())<=MAX_INVENTORY_BYTES,
            'inventory_size','Combined inventory sources exceed their byte limit')
    require(not cancel.is_set(),'cancelled','Inventory preview was cancelled')
    with tempfile.TemporaryDirectory(prefix='inventory-',dir=repository.root.parent) as temporary:
        specification=Path(temporary)/'input.json';atomic_json(specification,{'sources':sources,'entries':selected})
        receiver=InventoryReceiver()
        result=run_bounded(bubblewrap_argv(root,[sys.executable,'-m','consolecrypt_runner.inventory_worker'],
            extra_readonly=[(specification,'/tool/inventory-input.json')]),timeout=60,cancel=cancel,
            grace=15,output_limit=0,on_chunk=receiver.feed)
    require(result.exit_code==0 and not result.cancelled and not result.timed_out and not cancel.is_set()
            and receiver.complete and not receiver.buffer,'inventory_preview_failed',
            'Static inventory could not be normalized; no preview was published')
    body=bytes(receiver.body)
    value=decode(body,max_bytes=MAX_INVENTORY_BYTES)
    normalized=StaticInventory.from_snapshot(value)
    require(encode(normalized.to_snapshot())==body and all(host.host_id is None and not host.credential_refs
            for host in normalized.hosts.values()) and normalized.source_digests=={path:entry['sha256'] for path,entry in selected.items()},
            'inventory_authority','Static inventory cannot grant a host identity or credentials or replace its sources')
    key=hashlib.sha256(encode({'revision_digest':snapshot.digest,'sources':sources})).hexdigest()
    with repository.lock:
        current_root,current=repository.read(revision_id)
        require(current.digest==snapshot.digest and not cancel.is_set(),'inventory_changed','Project changed during inventory preview')
        directory=current_root.parent/'inventory-previews';_private_directory(directory)
        paths=list(directory.iterdir());target=directory/(key+'.json')
        previous=target.lstat().st_size if target.exists() else 0
        used=_cache_usage(repository)
        local=sum(path.lstat().st_size for path in paths)
        require((target.exists() or len(paths)<MAX_CAPTURES) and local-previous+len(body)<=MAX_REVISION_BYTES
                and used-previous+len(body)<=MAX_CACHE_BYTES,'inventory_quota','Release unused project previews before reading more sources')
        require(shutil.disk_usage(directory).free>=128*1024*1024+len(body),'inventory_disk','Insufficient reserved disk space')
        atomic_json(target,value,max_bytes=MAX_INVENTORY_BYTES)
    return {'version':1,'revision_id':revision_id,'revision_digest':snapshot.digest,'key':key,
        'digest':hashlib.sha256(body).hexdigest(),'bytes':len(body),'host_count':len(normalized.hosts),
        'group_count':len(normalized.groups),'sources':sources}


def inventory_chunk(repository,revision_id,key,offset,limit):
    require(isinstance(key,str) and re.fullmatch('[0-9a-f]{64}',key) is not None and type(offset) is int
            and offset>=0 and type(limit) is int and 1<=limit<=128*1024,'inventory_chunk','Invalid inventory artifact request')
    with repository.lock:
        root,snapshot=repository.read(revision_id)
        directory=root.parent/'inventory-previews';_private_directory(directory)
        fd=os.open(directory/(key+'.json'),os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK)
        with os.fdopen(fd,'rb') as source:
            info=os.fstat(source.fileno())
            require(stat.S_ISREG(info.st_mode) and info.st_nlink==1 and info.st_uid==os.getuid() and 0<info.st_size<=MAX_INVENTORY_BYTES
                    and offset<=info.st_size,'inventory_storage','Invalid inventory artifact')
            source.seek(offset);body=source.read(limit)
        return {'revision_digest':snapshot.digest,'key':key,'offset':offset,
                'data':base64.b64encode(body).decode('ascii'),'eof':offset+len(body)==info.st_size}
