"""Internal fixed monitoring-config mapping; single writer, no crash atomicity."""
import json
from pathlib import Path
import re

from stage import _safe_path,_write,digest

MANAGED=('etc/consolecrypt-monitoring/prometheus.yml','etc/consolecrypt-monitoring/rules.yml',
         'monitoring-command.json','monitoring-plan.json')
LEDGER='.monitoring-managed.json'
MAX_FILE_BYTES=64*1024
MAX_LEDGER_BYTES=16*1024


class MonitoringStateUnknown(RuntimeError):
    state='unknown'


def read_file(path,maximum=MAX_FILE_BYTES):
    path=_safe_path(path)
    if not path.is_file() or path.stat().st_size>maximum:raise ValueError('missing/oversized snapshot or managed file')
    if path.stat().st_nlink!=1:raise ValueError('hardlinked snapshot or managed input refused')
    with path.open('rb') as stream:value=stream.read(maximum+1)
    if len(value)>maximum:raise ValueError('snapshot or managed file byte budget')
    return value


def load_previous(prefix,*,recover_missing=False,allow_initial=False):
    if type(recover_missing) is not bool or type(allow_initial) is not bool:raise ValueError('explicit boolean recovery/initial option required')
    ledger_path=_safe_path(prefix/LEDGER)
    if not ledger_path.exists():
        if not allow_initial:raise ValueError('missing managed ledger')
        if any(_safe_path(prefix/name).exists() for name in MANAGED):raise ValueError('unowned configuration without managed ledger')
        return {},[*MANAGED,LEDGER]
    raw=read_file(ledger_path,MAX_LEDGER_BYTES);ledger=json.loads(raw)
    if (not isinstance(ledger,dict) or set(ledger)!=set(MANAGED)
            or any(not isinstance(v,str) or not re.fullmatch('[0-9a-f]{64}',v) for v in ledger.values())
            or raw!=json.dumps(ledger,sort_keys=True).encode()):raise ValueError('unowned or modified monitoring ledger')
    files={LEDGER:raw};missing=[]
    for name in MANAGED:
        path=_safe_path(prefix/name)
        if not path.exists():
            if not recover_missing:raise ValueError('missing managed configuration; explicit recovery required')
            missing.append(name);continue
        value=read_file(path)
        if digest(value)!=ledger[name]:raise ValueError('unowned or modified monitoring configuration')
        files[name]=value
    return files,missing


def target_mapping(wanted):
    if set(wanted)!=set(MANAGED) or any(type(value) is not bytes or len(value)>MAX_FILE_BYTES for value in wanted.values()):
        raise ValueError('bounded fixed monitoring configuration required')
    return {**wanted,LEDGER:json.dumps({name:digest(value) for name,value in wanted.items()},sort_keys=True).encode()}


def _validate_mapping(prefix,previous,missing,target,*,during_rollback):
    for name in (*MANAGED,LEDGER):
        path=_safe_path(prefix/name)
        if path.exists():
            actual=read_file(path,MAX_LEDGER_BYTES if name==LEDGER else MAX_FILE_BYTES)
            allowed=(previous.get(name),target[name]) if during_rollback else (previous.get(name),)
            if actual not in allowed:raise ValueError('unknown managed bytes during monitoring transaction')
        elif name not in missing:raise ValueError('unexpected missing managed input during monitoring transaction')


def _rollback(prefix,previous,missing,target):
    # All files are checked before the first recovery write/delete.
    _validate_mapping(prefix,previous,missing,target,during_rollback=True)
    for name in (*MANAGED,LEDGER):
        path=_safe_path(prefix/name)
        if name in missing:
            if path.exists():path.unlink()
        elif read_file(path,MAX_LEDGER_BYTES if name==LEDGER else MAX_FILE_BYTES)!=previous[name]:
            _write(path,previous[name],0o600)
    _validate_mapping(prefix,previous,missing,target,during_rollback=False)


def commit(prefix,wanted,previous,missing):
    """Commit fixed config bytes; restore old bytes/absence on caught errors."""
    target=target_mapping(wanted)
    if set(previous)|set(missing)!=set(target) or set(previous)&set(missing):raise ValueError('incomplete original managed mapping')
    _validate_mapping(prefix,previous,missing,target,during_rollback=False)
    try:
        for name in (*MANAGED,LEDGER):
            if previous.get(name)!=target[name]:_write(prefix/name,target[name],0o600)
        actual,absent=load_previous(prefix)
        if absent or actual!=target:raise ValueError('committed monitoring mapping differs')
    except Exception:
        try:_rollback(prefix,previous,missing,target)
        except Exception as rollback_error:
            raise MonitoringStateUnknown('monitoring write failed; managed configuration state unknown; no success reported') from rollback_error
        raise
