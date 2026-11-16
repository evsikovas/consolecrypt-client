"""Bounded native-owned inventory transfer; artifacts carry no execution grant."""
import base64
import hashlib
import os
import re
import shutil
import stat
import time
import uuid

from .errors import require
from .inventory import MAX_INVENTORY_BYTES,StaticInventory
from .inventory_sources import _cache_usage,MAX_CACHE_BYTES,MAX_CAPTURES,MAX_REVISION_BYTES
from .protocol import decode,encode,uuid_field
from .store import _private_directory,atomic_json

def read_artifact(repository,revision_id,reference):
    require(isinstance(reference,dict) and set(reference)=={'key','digest'} and
        all(isinstance(v,str) and re.fullmatch('[0-9a-f]{64}',v) for v in reference.values()),
        'inventory_reference','Invalid inventory reference')
    with repository.lock:
        root,_=repository.read(revision_id)
        directory=root.parent/'inventory-previews';_private_directory(directory)
        fd=os.open(directory/(reference['key']+'.json'),os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK)
        with os.fdopen(fd,'rb') as source:
            info=os.fstat(source.fileno())
            require(stat.S_ISREG(info.st_mode) and info.st_nlink==1 and info.st_uid==os.getuid()
                and 0<info.st_size<=MAX_INVENTORY_BYTES,'inventory_storage','Invalid inventory artifact')
            raw=source.read(MAX_INVENTORY_BYTES+1)
        require(hashlib.sha256(raw).hexdigest()==reference['digest'],'inventory_changed','Inventory artifact changed')
        value=decode(raw,max_bytes=MAX_INVENTORY_BYTES)
        inventory=StaticInventory.from_snapshot(value)
        require(encode(inventory.to_snapshot())==raw,'inventory_changed','Inventory artifact is not normalized')
        return inventory,raw


class InventoryUploads:
    def __init__(self,repository):
        self.repository=repository;self.pending={}
    def close(self):
        with self.repository.lock:self.pending.clear()
    def request(self,operation,body,cancel):
        with self.repository.lock:
            self.pending={key:value for key,value in self.pending.items() if value['deadline']>time.monotonic()}
            if operation=='inventory_upload_begin':
                require(set(body)=={'revision_id','digest','bytes'} and type(body['bytes']) is int
                    and 0<body['bytes']<=MAX_INVENTORY_BYTES and isinstance(body['digest'],str)
                    and re.fullmatch('[0-9a-f]{64}',body['digest']),'inventory_upload','Invalid inventory upload')
                self.repository.read(body['revision_id'])
                require(not cancel.is_set() and len(self.pending)<2,'inventory_upload_limit','Inventory transfer limit reached')
                transfer=str(uuid.uuid4());self.pending[transfer]={**body,'data':bytearray(),'deadline':time.monotonic()+120}
                return {'transfer_id':transfer}
            require(isinstance(body.get('transfer_id'),str),'inventory_upload','Missing transfer ID')
            transfer=uuid_field(body['transfer_id'],'inventory transfer ID')
            if operation=='inventory_upload_abort':
                require(set(body)=={'transfer_id'},'inventory_upload','Invalid abort request')
                self.pending.pop(transfer,None);return {'aborted':True}
            require(transfer in self.pending,'inventory_upload','Inventory transfer expired')
            pending=self.pending[transfer]
            if operation=='inventory_upload_chunk':
                require(set(body)=={'transfer_id','offset','data'} and type(body['offset']) is int
                    and body['offset']==len(pending['data']) and isinstance(body['data'],str)
                    and len(body['data'])<=175000,'inventory_upload','Invalid inventory chunk')
                try:part=base64.b64decode(body['data'],validate=True)
                except ValueError:require(False,'inventory_upload','Invalid chunk encoding')
                require(not cancel.is_set() and 0<len(part)<=128*1024 and len(pending['data'])+len(part)<=pending['bytes'],
                    'inventory_upload','Invalid inventory chunk size')
                pending['data'].extend(part);return {'offset':len(pending['data'])}
            require(operation=='inventory_upload_commit' and set(body)=={'transfer_id'},'inventory_upload','Invalid transfer operation')
            self.pending.pop(transfer)
            raw=bytes(pending['data'])
            require(not cancel.is_set() and len(raw)==pending['bytes'] and hashlib.sha256(raw).hexdigest()==pending['digest'],
                'inventory_upload','Inventory transfer is incomplete or changed')
            inventory=StaticInventory.from_snapshot(decode(raw,max_bytes=MAX_INVENTORY_BYTES))
            value=inventory.to_snapshot();normalized=encode(value)
            require(len(normalized)<=MAX_INVENTORY_BYTES,'inventory_size','Normalized inventory exceeds its budget')
            root,snapshot=self.repository.read(pending['revision_id'])
            key=hashlib.sha256(normalized).hexdigest();directory=root.parent/'inventory-previews';_private_directory(directory)
            paths=list(directory.iterdir());target=directory/(key+'.json');previous=target.lstat().st_size if target.exists() else 0
            require((target.exists() or len(paths)<MAX_CAPTURES) and sum(p.lstat().st_size for p in paths)-previous+len(normalized)<=MAX_REVISION_BYTES
                and _cache_usage(self.repository)-previous+len(normalized)<=MAX_CACHE_BYTES,'inventory_quota','Inventory cache quota exceeded')
            require(not cancel.is_set() and shutil.disk_usage(directory).free>=128*1024*1024+len(normalized),
                'inventory_disk','Insufficient reserved disk space')
            atomic_json(target,value,max_bytes=MAX_INVENTORY_BYTES)
            return {'version':1,'revision_digest':snapshot.digest,'key':key,'digest':key,'bytes':len(normalized)}
