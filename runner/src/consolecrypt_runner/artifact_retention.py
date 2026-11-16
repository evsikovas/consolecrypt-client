"""Fixed private history payload retirement; metadata is NEVER job authority.

Raw output expires after 30 days even when Native summary/history is pinned.
Nonce, manifest, approval, custody, cursor and unresolved target facts remain.
"""
from datetime import datetime, timezone
import hashlib,math,os,stat,uuid
from .errors import require,RunnerError
from .protocol import encode,decode,MAX_FRAME_BYTES,uuid_field
from .approval import DOMAIN,MANIFEST_FIELDS

SECONDS=30*24*60*60
MAX_OUTPUT=50*1024*1024
TERMINAL=frozenset({'succeeded','failed','cancelled','interrupted','unknown'})
FIELDS=frozenset({'version','job_id','manifest_digest','state','sequence','output_bytes',
    'output_truncated','accepted_at','updated_at','result'})
EXTRA=frozenset({'environment','history_activity_at','artifact_retirement','local_quiescence'})

def timestamp(value):
    require(type(value) is str and 1<=len(value)<=64 and value.endswith('Z'),'history_schema','Unknown history time')
    try:parsed=datetime.fromisoformat(value[:-1]+'+00:00')
    except ValueError:require(False,'history_schema','Unknown history time')
    require(parsed.utcoffset().total_seconds()==0,'history_schema','Unknown history time')
    return math.ceil(parsed.timestamp())

def validate_state(state):
    require(type(state) is dict and FIELDS<=set(state)<=FIELDS|EXTRA and type(state['version']) is int
        and state['version']==1 and type(state['state']) is str and state['state'] in TERMINAL|{'queued','running','cancelling'}
        and type(state['sequence']) is int and 0<=state['sequence']<=1_000_000_000
        and type(state['output_bytes']) is int and 0<=state['output_bytes']<=MAX_OUTPUT
        and type(state['output_truncated']) is bool,'history_schema','Unsupported history state')
    uuid_field(state['job_id'],'job ID')
    require(type(state['manifest_digest']) is str and len(state['manifest_digest'])==64 and all(c in '0123456789abcdef' for c in state['manifest_digest']),'history_schema','Unknown manifest digest')
    timestamp(state['accepted_at']);timestamp(state['updated_at'])
    if 'history_activity_at' in state:timestamp(state['history_activity_at'])
    if 'local_quiescence' in state:_validate_quiescence(state)
    marker=state.get('artifact_retirement')
    if marker is None:
        require('artifact_retirement' not in state,'history_schema','Unsupported history retirement')
        return None
    require(type(marker) is dict and {'version','retired_at','activity_at','last_sequence','output_bytes','events_sha256','cause'}<=set(marker)
        <={'version','retired_at','activity_at','last_sequence','output_bytes','events_sha256','cause','original_result_sha256','retained_result_sha256'}
        and type(marker['version']) is int and marker['version']==1
        and marker['cause'] in ('age_limit','service_policy_expired','service_revoked','service_quiescent')
        and state['state'] in TERMINAL,'history_schema','Unsupported history retirement')
    for field in ('retired_at','activity_at'):
        require(type(marker[field]) is int and 0<marker[field]<=253402300799,'history_schema','Unsupported history retirement time')
    require(marker['retired_at']>=marker['activity_at']
        and marker['activity_at']==timestamp(state.get('history_activity_at',state['updated_at']))
        and (marker['cause'] not in ('age_limit','service_quiescent') or marker['retired_at']>=marker['activity_at']+SECONDS)
        and type(marker['last_sequence']) is int and marker['last_sequence']==state['sequence']
        and type(marker['output_bytes']) is int and marker['output_bytes']==state['output_bytes']
        and type(marker['events_sha256']) is str and len(marker['events_sha256'])==64
        and all(c in '0123456789abcdef' for c in marker['events_sha256']),
        'history_schema','History retirement differs from original cursor')
    hashes={'original_result_sha256','retained_result_sha256'}
    require(not (hashes&set(marker)) or hashes<=set(marker),'history_schema','Incomplete result retirement trace')
    if hashes<=set(marker):
        for name in hashes:
            require(type(marker[name]) is str and len(marker[name])==64 and all(c in '0123456789abcdef' for c in marker[name]),
                'history_schema','Unknown result retirement digest')
        require(hashlib.sha256(encode(state['result'])).hexdigest()==marker['retained_result_sha256'],
            'history_schema','Retained structured result changed')
    if marker['cause']=='service_quiescent':
        require('local_quiescence' in state and marker['retired_at']>=state['local_quiescence']['observed_at'],
            'history_schema','Missing original local quiescence observation')
    return marker


def _validate_quiescence(state):
    """Current local OS facts only, never execution/replay/remote authority."""
    q=state['local_quiescence']
    fields={'version','source','identity','policy_digest','runtime_device','runtime_inode','identity_device','identity_inode',
        'cgroup_parent_device','cgroup_parent_inode','cgroup_device','cgroup_inode','daemon_pid','observed_at',
        'original_state','original_updated_at','original_result_sha256'}
    require(type(q) is dict and set(q)==fields and type(q['version']) is int and q['version']==1
        and q['source'] in ('systemdInactiveCgroupAbsent','systemdPidfdCgroupV2'),
        'history_schema','Unknown local quiescence schema')
    uuid_field(q['identity'],'service identity')
    for name in ('policy_digest','original_result_sha256'):
        require(type(q[name]) is str and len(q[name])==64 and all(c in '0123456789abcdef' for c in q[name]),
            'history_schema','Unknown local quiescence digest')
    for name in ('runtime_device','runtime_inode','identity_device','identity_inode','cgroup_parent_device',
                 'cgroup_parent_inode','cgroup_device','cgroup_inode','daemon_pid'):
        require(type(q[name]) is int and 0<=q[name]<2**64,'history_schema','Unknown local quiescence identity')
    require(all(q[name]>0 for name in ('runtime_inode','identity_inode','cgroup_parent_inode'))
        and type(q['observed_at']) is int and 0<q['observed_at']<=253402300799
        and q['original_state'] in TERMINAL|{'queued','running','cancelling'}
        and timestamp(q['original_updated_at'])<=q['observed_at']
        and _quiescence_result_bound(state,q),
        'history_schema','Original diagnosis differs from quiescence observation')
    if q['source']=='systemdInactiveCgroupAbsent':
        require(q['daemon_pid']==q['cgroup_device']==q['cgroup_inode']==0,'history_schema','Absent cgroup is not a PID observation')
    else:require(q['daemon_pid']>0 and q['cgroup_inode']>0,'history_schema','Original live process/cgroup required')
    observed='interrupted' if q['original_state']=='queued' else 'unknown' if q['original_state'] in ('running','cancelling') else q['original_state']
    require(state['state']==observed,'history_schema','Quiescence cannot grant a terminal success')
    return q


def _quiescence_result_bound(state,q):
    current=hashlib.sha256(encode(state['result'])).hexdigest();marker=state.get('artifact_retirement')
    if type(marker) is dict and 'original_result_sha256' in marker and 'retained_result_sha256' in marker:
        return marker['original_result_sha256']==q['original_result_sha256'] and marker['retained_result_sha256']==current
    return current==q['original_result_sha256']


def recover_quiescent(directory,state,metadata,now):
    """Private service caller already holds actual kernel+original file locks.
    Same authenticated recovery semantics; preserve original result verbatim and
    retain original state/time/hash. No fake worker interval or success receipt.
    """
    validate_state(state)
    if 'local_quiescence' in state:return state
    value=dict(state);value.setdefault('history_activity_at',state['updated_at'])
    value['local_quiescence']={**metadata,'original_state':state['state'],'original_updated_at':state['updated_at'],
        'original_result_sha256':state.get('artifact_retirement',{}).get('original_result_sha256',hashlib.sha256(encode(state['result'])).hexdigest())}
    if state['state'] in ('queued','running','cancelling'):
        value['state']='interrupted' if state['state']=='queued' else 'unknown'
        value['updated_at']=datetime.fromtimestamp(now,timezone.utc).isoformat().replace('+00:00','Z')
    validate_state(value)
    fd=os.open(directory,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC)
    try:
        identity=_identity(fd);_named(directory,fd,identity);_record(fd,directory,state)
        _commit_state(fd,state,value);_named(directory,fd,identity)
    finally:os.close(fd)
    return value

def _identity(fd):
    info=os.fstat(fd)
    require(stat.S_ISDIR(info.st_mode) and info.st_uid==os.getuid() and stat.S_IMODE(info.st_mode)==0o700,
        'history_storage','Original private history directory required')
    return info.st_dev,info.st_ino

def _named(directory,fd,identity):
    info=directory.lstat()
    require(_identity(fd)==identity and stat.S_ISDIR(info.st_mode) and (info.st_dev,info.st_ino)==identity,
        'history_storage','Original history directory changed')

def _file(fd,name,maximum):
    child=os.open(name,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK|os.O_CLOEXEC,dir_fd=fd)
    try:
        info=os.fstat(child)
        require(stat.S_ISREG(info.st_mode) and info.st_nlink==1 and info.st_uid==os.getuid()
            and stat.S_IMODE(info.st_mode)==0o600 and info.st_size<=maximum,
            'history_storage','Unsupported private history artifact')
        return child,info
    except BaseException:os.close(child);raise

def _same_file(fd,name,expected):
    info=os.stat(name,dir_fd=fd,follow_symlinks=False)
    require(stat.S_ISREG(info.st_mode) and info.st_nlink==1 and info.st_uid==os.getuid()
        and stat.S_IMODE(info.st_mode)==0o600 and (info.st_dev,info.st_ino,info.st_size)==
        (expected.st_dev,expected.st_ino,expected.st_size),'history_storage','Original history file changed')

def _record(fd,directory,state):
    uuid_field(directory.name,'job ID')
    require(state['job_id']==directory.name,'history_schema','History belongs to another job')
    child,info=_file(fd,'manifest.json',MAX_FRAME_BYTES)
    with os.fdopen(child,'rb') as source:manifest=decode(source.read(MAX_FRAME_BYTES+1))
    require(type(manifest) is dict and set(manifest)==MANIFEST_FIELDS and type(manifest['version']) is int
        and manifest['version']==1 and manifest['job_id']==directory.name
        and hashlib.sha256(DOMAIN+encode(manifest)).hexdigest()==state['manifest_digest'],
        'history_schema','Unknown or changed immutable job manifest')
    _same_file(fd,'manifest.json',info)
    return manifest

def _events(fd,job):
    try:child,info=_file(fd,'events.jsonl',MAX_OUTPUT)
    except FileNotFoundError:return None,0,0,hashlib.sha256(b'').hexdigest()
    digest=hashlib.sha256();sequence=0;used=0
    with os.fdopen(child,'rb') as source:
        while raw:=source.readline(MAX_FRAME_BYTES+1):
            require(len(raw)<=MAX_FRAME_BYTES and raw.endswith(b'\n'),'history_corrupt','Incomplete history journal')
            value=decode(raw)
            require(type(value) is dict and set(value)=={'version','job_id','sequence','timestamp','type','payload'}
                and type(value['version']) is int and value['version']==1 and value['job_id']==job
                and type(value['sequence']) is int and value['sequence']==sequence+1
                and type(value['type']) is str and 1<=len(value['type'])<=64
                and all(32<=ord(c)<=126 for c in value['type']),
                'history_corrupt','History cursor or schema changed')
            timestamp(value['timestamp'])
            digest.update(raw);used+=len(raw);sequence+=1
        _same_file(fd,'events.jsonl',info)
    return info,used,sequence,digest.hexdigest()

def _commit_state(fd,expected,value):
    child,info=_file(fd,'state.json',MAX_FRAME_BYTES)
    with os.fdopen(child,'rb') as source:actual=decode(source.read(MAX_FRAME_BYTES+1))
    require(actual==expected,'history_changed','History changed before retirement')
    name='.history-'+str(uuid.uuid4());temporary=None
    try:
        temporary=os.open(name,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW|os.O_CLOEXEC,0o600,dir_fd=fd)
        with os.fdopen(temporary,'wb') as output:
            temporary=None;output.write(encode(value));output.flush();os.fsync(output.fileno())
        _same_file(fd,'state.json',info)
        os.rename(name,'state.json',src_dir_fd=fd,dst_dir_fd=fd);os.fsync(fd)
    finally:
        if temporary is not None:os.close(temporary)
        try:os.unlink(name,dir_fd=fd)
        except FileNotFoundError:pass

def _structured_result(result,manifest):
    # Old execution accepted arbitrary runner_summary dictionaries. They are
    # raw guest diagnostics even inside result, not immutable audit facts.
    if type(result) is not dict or result.get('summary') is None:return result
    require(type(result['summary']) is dict,'history_schema','Unknown legacy summary shape')
    from .diagnostic_mask import structural_summary
    aliases={target['alias'] for target in manifest['targets']}
    return dict(result,summary=structural_summary(result['summary'],{},aliases))


def reconcile_retired_result(directory,state,result):
    """Only the existing locked state writer calls after validating OLD state.
    Preserve original raw-result trace, close new summary before atomic commit.
    No journal reopen, cursor/activity refresh or independent result authority.
    """
    marker=validate_state(state)
    if marker is None:return result,None
    fd=os.open(directory,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC)
    try:
        original=_identity(fd);_named(directory,fd,original)
        manifest=_record(fd,directory,state)
        projected=_structured_result(result,manifest)
        updated=dict(marker)
        updated.setdefault('original_result_sha256',hashlib.sha256(encode(state['result'])).hexdigest())
        updated['retained_result_sha256']=hashlib.sha256(encode(projected)).hexdigest()
        _named(directory,fd,original)
        return projected,updated
    finally:os.close(fd)


def retire(directory,state,now,*,cause='age_limit'):
    """Caller owns SAME JobStore lock; service causes need genuine ended-policy
    caller plus joined workers. This never changes any authority/replay record.
    A committed marker completes exact events deletion after restart; unknown
    marker/file identity fails closed. No secure erase/swap/backups guarantee.
    """
    require(type(now) is int and 0<now<=253402300799,'history_time','Invalid history clock')
    marker=validate_state(state)
    if marker is None:
        if state['state'] not in TERMINAL:return state,False
        activity=timestamp(state.get('history_activity_at',state['updated_at']))
        if now<activity or (cause in ('age_limit','service_quiescent') and now<activity+SECONDS):return state,False
    fd=os.open(directory,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC)
    try:
        original=_identity(fd);_named(directory,fd,original)
        manifest=_record(fd,directory,state)
        info,used,sequence,digest=_events(fd,state['job_id'])
        # Validate an existing commitment BEFORE even minimizing legacy result
        # diagnostics. A changed journal preserves the whole original state.
        if marker is not None and info is not None:
            require(used==marker['output_bytes'] and sequence==marker['last_sequence'] and digest==marker['events_sha256'],
                'history_corrupt','Retained retirement journal changed')
        if marker is None:
            require(sequence==state['sequence'] and used==state['output_bytes'],'history_corrupt','Original journal differs from cursor')
            marker={'version':1,'retired_at':now,'activity_at':activity,'last_sequence':sequence,
                'output_bytes':used,'events_sha256':digest,'cause':cause}
            result=_structured_result(state['result'],manifest)
            original_hash=state.get('local_quiescence',{}).get('original_result_sha256',hashlib.sha256(encode(state['result'])).hexdigest())
            marker.update(original_result_sha256=original_hash,retained_result_sha256=hashlib.sha256(encode(result)).hexdigest())
            value=dict(state,result=result,artifact_retirement=marker)
            validate_state(value);_commit_state(fd,state,value);state=value
        elif 'original_result_sha256' not in marker:
            # Upgrade only a fully validated old retirement, preserving its
            # original cursor/activity/retirement time and result digest.
            result=_structured_result(state['result'],manifest)
            original_hash=state.get('local_quiescence',{}).get('original_result_sha256',hashlib.sha256(encode(state['result'])).hexdigest())
            marker=dict(marker,original_result_sha256=original_hash,retained_result_sha256=hashlib.sha256(encode(result)).hexdigest())
            value=dict(state,result=result,artifact_retirement=marker)
            validate_state(value);_commit_state(fd,state,value);state=value
        _named(directory,fd,original)
        if info is not None:
            _same_file(fd,'events.jsonl',info);os.unlink('events.jsonl',dir_fd=fd);os.fsync(fd)
        _named(directory,fd,original)
        return state,info is not None
    finally:os.close(fd)

def view(state):
    marker=validate_state(state)
    if marker is None:return None
    return {'version':1,'retired_at':marker['retired_at'],'last_sequence':marker['last_sequence'],'output_bytes':marker['output_bytes']}


def payload_reference_expired(state,manifest,now):
    """Known ended local worker only. UNKNOWN/crash/legacy facts stay pinned.

    This does not delete a snapshot: the existing repository must still check
    every other job and the service-template reference before releasing it.
    """
    try:return _payload_reference_expired(state,manifest,now)
    except RunnerError:return False # Refuse release; strict validator remains unchanged.


def _payload_reference_expired(state,manifest,now):
    if validate_state(state) is None:return False
    require(type(manifest) is dict and set(manifest)==MANIFEST_FIELDS
        and type(manifest['version']) is int and manifest['version']==1
        and manifest['job_id']==state['job_id']
        and hashlib.sha256(DOMAIN+encode(manifest)).hexdigest()==state['manifest_digest'],
        'history_schema','Unknown original snapshot authority')
    expires=manifest['access_expires_at']
    if type(now) is not int or type(expires) is not int or not 0<expires<=253402300799 or now<expires:return False
    return _known_worker_ended(state)


def _known_worker_ended(state):
    """Closed stored observations only; not a grant or a caller's joined flag.

    Service cleanup must additionally verify original policy/custody/targets,
    locks and runtime identity. UNKNOWN/legacy/future observations are denied.
    """
    validate_state(state)
    result=state['result']
    if (state['state']=='cancelled' and type(result) is dict
            and set(result)=={'started'} and result['started'] is False):return True
    if state['state'] not in ('succeeded','failed') or type(result) is not dict:return False
    expected={'exit_code','summary','timed_out','cancel_requested','remote_changes_may_be_partial','elapsed_ms','runner_execution'}
    if set(result)!=expected or type(result['exit_code']) is not int or type(result['summary']) is not dict:return False
    if any(result[name] is not False for name in ('timed_out','cancel_requested','remote_changes_may_be_partial')):return False
    elapsed=result['elapsed_ms'];interval=result['runner_execution']
    if type(elapsed) is not int or not 0<=elapsed<=86400000:return False
    if type(interval) is not dict or set(interval)!={'version','clockSource','scope','spawnObservedUtc','exitObservedUtc','elapsedMonotonicMs'}:return False
    if type(interval['version']) is not int or interval['version']!=1 or interval['clockSource']!='runner-wall' or interval['scope']!='runner-process':return False
    if type(interval['elapsedMonotonicMs']) is not int or interval['elapsedMonotonicMs']!=elapsed:return False
    try:
        return (timestamp(state['accepted_at'])<=timestamp(interval['spawnObservedUtc'])
                <=timestamp(interval['exitObservedUtc'])
                <=timestamp(state.get('history_activity_at',state['updated_at'])))
    except Exception:
        return False
