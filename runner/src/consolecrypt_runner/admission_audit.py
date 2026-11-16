"""Private admission provenance DATA; never a grant, signer lookup or replay gate."""
import os
import stat
from .errors import require
from .protocol import decode, encode, uuid_field, MAX_FRAME_BYTES

FIELDS=frozenset({'version','job_id','manifest_digest','signer_id','scope','tenant_id','runner_id','project_id','revision_digest','issued_at','production','owner_evidence_digest','targets'})

def derive(manifest,digest,signer,owner_digest):
    # Called only AFTER the existing manifest/optional owner signatures pass.
    return encode({'version':1,'job_id':manifest['job_id'],'manifest_digest':digest,
        'signer_id':signer,'scope':'jobs.execute','tenant_id':manifest['tenant_id'],
        'runner_id':manifest['runner_id'],'project_id':manifest['project_id'],
        'revision_digest':manifest['revision_digest'],'issued_at':manifest['issued_at'],
        'production':manifest['policy']['production'],'owner_evidence_digest':owner_digest,
        'targets':[{'host_id':target['host_id'],'route_digest':target['route_digest']}for target in manifest['targets']]})

def checked(raw,manifest,digest):
    require(type(raw)is bytes and len(raw)<=MAX_FRAME_BYTES,'admission_audit','Private admission audit unavailable')
    value=decode(raw)
    require(type(value)is dict and set(value)==FIELDS and type(value['version'])is int and value['version']==1
        and value['scope']=='jobs.execute','admission_audit','Private admission audit unavailable')
    uuid_field(value['signer_id'],'original admission signer')
    owner=value['owner_evidence_digest']
    require(owner is None or type(owner)is str and len(owner)==64 and all(c in '0123456789abcdef'for c in owner),
        'admission_audit','Private admission audit unavailable')
    require(type(value['production'])is bool and (owner is not None)==value['production'],
        'admission_audit','Private admission audit unavailable')
    require(raw==derive(manifest,digest,value['signer_id'],owner),
        'admission_audit','Private admission audit differs from original signed scope')
    return value

def read(path,manifest,digest):
    try:fd=os.open(path,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK|os.O_CLOEXEC)
    except FileNotFoundError:return None
    with os.fdopen(fd,'rb')as source:
        before=os.fstat(source.fileno())
        require(stat.S_ISREG(before.st_mode) and before.st_uid==os.getuid() and before.st_nlink==1
            and stat.S_IMODE(before.st_mode)==0o600 and before.st_size<=MAX_FRAME_BYTES,
            'admission_audit','Private admission audit unavailable')
        raw=source.read(MAX_FRAME_BYTES+1);after=os.fstat(source.fileno())
        named=os.stat(path,follow_symlinks=False)
        def identity(info):return (info.st_dev,info.st_ino,info.st_size,info.st_mtime_ns,info.st_ctime_ns,info.st_mode,info.st_uid,info.st_nlink)
        require(identity(before)==identity(after)==identity(named),
            'admission_audit','Private admission audit changed')
    return checked(raw,manifest,digest)

def persist_once(path,raw,manifest,digest):
    checked(raw,manifest,digest)
    existing=read(path,manifest,digest)
    if existing is not None:
        require(encode(existing)==raw,'admission_audit','Original admission audit cannot be replaced')
        return
    # Exclusive, never replace/adopt or delete incomplete metadata. A failure
    # precedes state/ACK; existing journal recovery keeps the job interrupted.
    fd=os.open(path,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW|os.O_CLOEXEC,0o600)
    with os.fdopen(fd,'wb')as output:output.write(raw);output.flush();os.fsync(output.fileno())
    parent=os.open(path.parent,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC)
    try:os.fsync(parent)
    finally:os.close(parent)
