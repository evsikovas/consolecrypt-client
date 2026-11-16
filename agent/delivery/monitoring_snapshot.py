"""Bounded named monitoring CONFIG snapshots; never TSDB backup or service control."""
import argparse
import json
from itertools import islice
from pathlib import Path
import re
import sys

DIRECTORY=Path(__file__).resolve().parent
sys.path.insert(0,str(DIRECTORY))
from monitoring_prefix import root,parse_plan,render,checked_binary,apply,MARKER
from stage import _safe_path,_write,digest
from monitoring_transaction import MANAGED,LEDGER,MAX_FILE_BYTES,read_file,load_previous,MonitoringStateUnknown

BLOBS=dict(zip((*MANAGED,LEDGER),('prometheus.yml','rules.yml','command.json','plan.json','ledger.json')))
STORE_MARKER='.cc-config-snapshots.json'
MAX_SNAPSHOTS=4
MAX_SNAPSHOT_BYTES=128*1024

RestoreStateUnknown=MonitoringStateUnknown


def encode(value):return json.dumps(value,sort_keys=True,separators=(',',':')).encode()+b'\n'


def name_checked(name):
    if not isinstance(name,str) or not re.fullmatch(r'[a-z0-9][a-z0-9_-]{0,31}',name):raise ValueError('bounded snapshot name required')
    return name


def live(prefix,*,recover_missing=False):
    return load_previous(prefix,recover_missing=recover_missing)


def store(prefix,*,create=False):
    path=_safe_path(prefix/'config-snapshots')
    expected={'format':'cc-monitoring-config-snapshot-store/v1','prefix':str(prefix.resolve()),
              'prefix_marker_sha256':digest(read_file(prefix/MARKER,1024))}
    if not path.exists():
        if not create:raise ValueError('owned snapshot store missing')
        path.mkdir(mode=0o700)
        _write(path/STORE_MARKER,encode(expected),0o600)
    if read_file(path/STORE_MARKER,2048)!=encode(expected):raise ValueError('unowned or foreign snapshot store')
    children=list(islice(path.iterdir(),MAX_SNAPSHOTS+2))
    if len(children)>MAX_SNAPSHOTS+1:raise ValueError('snapshot count budget')
    for child in children:
        _safe_path(child)
        if child.name!=STORE_MARKER and (not child.is_dir() or name_checked(child.name)!=child.name):
            raise ValueError('unowned snapshot store entry')
    return path,expected


def snapshot(prefix,name):
    prefix=root(prefix);name=name_checked(name)
    files,_=live(prefix)
    if sum(map(len,files.values()))>MAX_SNAPSHOT_BYTES:raise ValueError('snapshot byte budget')
    directory,binding=store(prefix,create=True)
    metadata={'format':'cc-monitoring-config-snapshot/v1','name':name,**{k:v for k,v in binding.items() if k!='format'},
              'files':{key:digest(value) for key,value in files.items()},
              'plan_sha256':digest(files['monitoring-plan.json'])}
    manifest=encode(metadata)
    if sum(map(len,files.values()))+len(manifest)>MAX_SNAPSHOT_BYTES:raise ValueError('snapshot byte budget includes manifest')
    target=_safe_path(directory/name)
    if target.exists():
        loaded,existing=load(prefix,name)
        if loaded!=files or existing!=metadata:raise ValueError('snapshot name already binds different configuration')
        return {'changed':False,'name':name,'snapshot_sha256':digest(manifest),'metrics_included':False}
    if len(list(islice(directory.iterdir(),MAX_SNAPSHOTS+2)))-1>=MAX_SNAPSHOTS:raise ValueError('snapshot count budget')
    target.mkdir(mode=0o700)
    for key,value in files.items():_write(target/BLOBS[key],value,0o600)
    _write(target/'snapshot.json',manifest,0o600)
    return {'changed':True,'name':name,'snapshot_sha256':digest(manifest),'metrics_included':False}


def load(prefix,name):
    directory,binding=store(prefix);target=_safe_path(directory/name_checked(name))
    if not target.is_dir() or {p.name for p in islice(target.iterdir(),len(BLOBS)+2)}!=set(BLOBS.values())|{'snapshot.json'}:
        raise ValueError('unowned/incomplete snapshot file mapping')
    manifest=read_file(target/'snapshot.json',4096);metadata=json.loads(manifest)
    expected_keys={'format','name','prefix','prefix_marker_sha256','files','plan_sha256'}
    if (not isinstance(metadata,dict) or set(metadata)!=expected_keys or manifest!=encode(metadata)
            or metadata['format']!='cc-monitoring-config-snapshot/v1' or metadata['name']!=name
            or metadata['prefix']!=binding['prefix'] or metadata['prefix_marker_sha256']!=binding['prefix_marker_sha256']
            or not isinstance(metadata['files'],dict) or set(metadata['files'])!=set(BLOBS)):
        raise ValueError('corrupt or foreign-prefix snapshot manifest')
    files={key:read_file(target/blob,16384 if key==LEDGER else MAX_FILE_BYTES) for key,blob in BLOBS.items()}
    if sum(map(len,files.values()))+len(manifest)>MAX_SNAPSHOT_BYTES:raise ValueError('snapshot byte budget includes manifest')
    if any(digest(value)!=metadata['files'][name] for name,value in files.items()) or digest(files['monitoring-plan.json'])!=metadata['plan_sha256']:
        raise ValueError('snapshot file digest mismatch')
    return files,metadata


def restore(prefix,name,data,binary,binary_sha256,promtool,promtool_sha256,port,*,recover_missing=False):
    prefix=root(prefix);name=name_checked(name)
    current,missing=live(prefix,recover_missing=recover_missing)
    files,metadata=load(prefix,name)
    plan=parse_plan(data);binary=checked_binary(binary,binary_sha256);promtool=checked_binary(promtool,promtool_sha256)
    wanted=render(prefix,plan,binary,port)
    expected_ledger=json.dumps({key:digest(value) for key,value in wanted.items()},sort_keys=True).encode()
    if any(files[key]!=value for key,value in wanted.items()) or files[LEDGER]!=expected_ledger:
        raise ValueError('snapshot differs from explicit reviewed plan/binary/port/rules; receiver is never implicit')
    # apply validates a temporary copy with pinned promtool BEFORE any write.
    # Its check=True path also avoids rewriting an already-restored ledger.
    unchanged=not missing and current==files
    apply(prefix,data,binary,binary_sha256,promtool,promtool_sha256,port,check=unchanged,create_data_directory=False,_recover_missing=recover_missing)
    if not unchanged:
        try:
            persisted,_=live(prefix)
            if persisted!=files:raise ValueError('restored configuration differs')
        except Exception as read_error:
            raise RestoreStateUnknown('post-commit configuration state unknown; no success reported') from read_error
    return {'format':'cc-monitoring-config-restore/v1','changed':not unchanged,'snapshot':name,
            'snapshot_sha256':digest(encode(metadata)),'recovered_missing':missing,'receiver_enabled':plan.receiver_enabled,
            'promtool_validated':True,'process_started':False,'metrics_backed_up':False,'metrics_deleted':False,
            'accounts_mounts_services_changed':False}


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('operation',choices=['snapshot','restore'])
    parser.add_argument('--prefix',type=Path,required=True);parser.add_argument('--name',required=True)
    parser.add_argument('--prometheus-binary',type=Path);parser.add_argument('--prometheus-sha256')
    parser.add_argument('--promtool',type=Path);parser.add_argument('--promtool-sha256');parser.add_argument('--port',type=int,default=9090)
    parser.add_argument('--recover-missing',action='store_true');args=parser.parse_args()
    if args.operation=='snapshot':
        if args.recover_missing:parser.error('snapshot cannot recover missing live inputs')
        result=snapshot(args.prefix,args.name)
    else:
        if any(value is None for value in [args.prometheus_binary,args.prometheus_sha256,args.promtool,args.promtool_sha256]):parser.error('restore requires both pinned binaries')
        raw=sys.stdin.buffer.read(65537)
        if len(raw)>65536:parser.error('reviewed plan byte budget')
        result=restore(args.prefix,args.name,json.loads(raw),args.prometheus_binary,args.prometheus_sha256,args.promtool,args.promtool_sha256,args.port,recover_missing=args.recover_missing)
    print(json.dumps(result,sort_keys=True))


if __name__=='__main__':main()
