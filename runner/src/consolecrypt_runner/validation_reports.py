"""Bounded checks indexed by the complete immutable execution input tuple."""
import hashlib
from pathlib import Path
import shutil
import threading

from .errors import require
from .protocol import encode
from .snapshot import relative_path
from .store import _private_directory,atomic_json,read_json

FIELDS=('revision_digest','environment_digest','playbook','parameters_digest','inventory_digest','targets_digest')
MAX_REPORTS=32
MAX_BYTES=8*1024*1024
_LOCK=threading.RLock()

def identity(value):
    bound={name:value.get(name) for name in FIELDS}
    for name in FIELDS:
        item=bound[name]
        if name=='playbook':
            require(isinstance(item,str),'validation_identity','Invalid selected playbook')
            relative_path(item)
        elif item is not None:
            require(isinstance(item,str) and len(item)==64 and all(c in '0123456789abcdef' for c in item),
                    'validation_identity','Invalid validation identity')
    require(bound['revision_digest'] is not None and bound['environment_digest'] is not None and
            (all(bound[name] is None for name in FIELDS[3:]) or all(bound[name] is not None for name in FIELDS[3:])),
            'validation_identity','Incomplete validation identity')
    return bound

def report_key(value):
    return hashlib.sha256(encode(identity(value))).hexdigest()

def write_report(revision:Path,report):
    require('input_witnesses' not in report,'validation_private','Private witnesses must not be persisted')
    key=report_key(report)
    with _LOCK:
        directory=revision/'validations';_private_directory(directory)
        files=list(directory.iterdir())
        target=directory/(key+'.json')
        used=0;previous=0
        for path in files:
            require(path.is_file() and not path.is_symlink() and path.suffix=='.json',
                    'validation_storage','Unexpected validation record')
            size=path.stat().st_size;used+=size
            if path==target:previous=size
        size=len(encode(report))
        require((target.exists() or len(files)<MAX_REPORTS) and used-previous+size<=MAX_BYTES,
                'validation_quota','Release unused revisions before validating more input combinations')
        require(shutil.disk_usage(directory).free>=128*1024*1024+size,'validation_disk','Insufficient reserved disk space')
        atomic_json(target,report)
    return key

def read_report(revision:Path,inputs):
    bound=identity(inputs)
    path=revision/'validations'/(report_key(bound)+'.json')
    with _LOCK:
        report=read_json(path)
    require(identity(report)==bound,'validation_changed','Validation belongs to different execution inputs')
    return report
