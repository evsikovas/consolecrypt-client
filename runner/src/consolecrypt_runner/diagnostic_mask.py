"""Per-job trusted-parent diagnostic privacy BEFORE first durable append.

SSH/Vault values stay Native; only this private parent calls existing scoped
control to obtain a redacted data copy. Guest JobBroker never forwards it.
Known extra-vars already exist in checked ephemeral submission memory here.
No manifest/receipt/store mask list, new resolver, credential send or registry.
Python copies/swap/secure erasure are not guaranteed; own bytearrays are wiped.
"""
import json
from .protocol import encode
from .event_privacy import diagnostic_result

KINDS=frozenset(('runner_on_ok','runner_on_failed','runner_on_unreachable','runner_on_skipped',
    'playbook_on_task_start','playbook_on_play_start','playbook_on_stats','runner_summary','worker_error'))
STATUSES=frozenset(('successful','failed','timeout','canceled','running','starting','unstarted'))

class KnownJobSecrets:
    def __init__(self,parameters):
        self.values=[];self.size=0;self.live=True
        try:self._collect(parameters['extra_vars'],0)
        except Exception:self.close()
        self.values.sort(key=len,reverse=True)
    def _remember(self,value):
        if not value:return
        raw=bytearray(value.encode('utf-8'))
        if raw in self.values:
            raw[:]=b'\x00'*len(raw);return
        if len(raw)>128*1024 or len(self.values)>=512 or self.size+len(raw)>1024*1024:raise ValueError('privacy budget')
        self.size+=len(raw);self.values.append(raw)
    def _collect(self,value,depth):
        if depth>12:raise ValueError('privacy budget')
        if type(value) is dict:
            for child in value.values():self._collect(child,depth+1)
        elif type(value) is list:
            for child in value:self._collect(child,depth+1)
        elif type(value) is str:self._remember(value)
        elif value is not None:self._remember(json.dumps(value,separators=(',',':')))
    def close(self):
        for value in self.values:value[:]=b'\x00'*len(value)
        self.values.clear();self.size=0;self.live=False
    def text(self,value,work=None):
        work=[2_000_000] if work is None else work
        if not self.live:raise ValueError('privacy unavailable')
        if value.endswith('[TRUNCATED]'):return '[CENSORED]'
        raw=value.encode('utf-8')
        if len(raw)>128*1024:raise ValueError('privacy budget')
        output=bytearray();at=0
        while at<len(raw):
            matched=None
            for secret in self.values:
                cost=min(len(secret),len(raw)-at)+1
                if work[0]<cost:raise ValueError('privacy budget')
                work[0]-=cost
                if raw.startswith(secret,at):matched=len(secret);break
            if matched is not None:output.extend(b'[REDACTED]');at+=matched
            else:output.append(raw[at]);at+=1
            if len(output)>128*1024:raise ValueError('privacy budget')
        return output.decode('utf-8')
    def project(self,value):
        nodes=1024;work=[2_000_000]
        def visit(item,depth):
            nonlocal nodes
            nodes-=1
            if nodes<0 or depth>12:raise ValueError('privacy budget')
            if type(item) is dict:return {self.text(k,work):visit(v,depth+1) for k,v in item.items()}
            if type(item) is list:return [visit(v,depth+1) for v in item]
            if type(item) is str:return self.text(item,work)
            if item is None:return None
            raw=json.dumps(item,separators=(',',':'));masked=self.text(raw,work)
            return item if masked==raw else masked
        result=visit(value,0)
        if len(encode(result))>96*1024:raise ValueError('privacy budget')
        return result

def structural_summary(original,safe,aliases):
    # Execution status/exit and validated counters are structural observations,
    # not arbitrary diagnostic text. They are NEVER taken from censor replies.
    status=original.get('status');code=original.get('exit_code')
    result={'status':status if type(status) is str and status in STATUSES else 'unknown',
        'exit_code':code if type(code) is int and 0<=code<=255 else None}
    stats=original.get('stats')
    if type(stats) is dict:
        result['stats']={}
        for field in ('ok','changed','failures','dark','skipped','rescued','ignored'):
            values=stats.get(field)
            if type(values) is dict:
                result['stats'][field]={host:n for host,n in values.items()
                    if host in aliases and type(n) is int and 0<=n<=1_000_000_000}
    return result

def project_event(channel,verified,kind,payload,mask,*,cancel):
    kind=kind if kind in KINDS else 'worker_diagnostic'
    aliases={target['alias'] for target in verified.value['targets']}
    try:
        # Reapply the pinned no_log/facts/module-input projection in the trusted
        # parent before any log call; guest stdout remains untrusted DATA.
        local=mask.project(diagnostic_result(payload))
        if kind=='runner_summary':local=structural_summary(payload,local,aliases)
        safe=channel._redact_event(verified,kind,local,cancel=cancel,timeout=5)
        if type(safe) is not dict or len(encode(safe))>96*1024:raise ValueError('privacy response')
        safe=mask.project(safe)
    except Exception:
        # Do not persist original values, exception strings or diagnostic RPC
        # body on timeout/expiry/disconnect/budget failure. No job revoke here.
        safe={'censored':True,'reason':'privacy_unavailable'}
    if kind=='runner_summary':safe=structural_summary(payload,safe,aliases)
    return kind,safe
