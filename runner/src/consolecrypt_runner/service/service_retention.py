"""Closed service input retirement, never resume/dispatch or revocation authority.

Only the same joined executor or a positively quiescent existing-only startup
may retire the fixed template. Unknown/orphan/future artifacts remain explicit
recovery cases. Encrypted enrollment/capsules, nonce/floors/custody remain.
This is unlink/replace lifetime management, not secure erase or key forgetting.
"""
import fcntl,hashlib,math,os,sqlite3,stat,time,uuid
from pathlib import Path
from consolecrypt_runner.errors import require
from consolecrypt_runner.protocol import decode,encode
from .cc_service_queue.queue import bounded_children,identifier,record,MAX_JOBS
from .service_authority_v4 import verify_delegation,retained_owner_status

TEMPLATE='service-template.json'
MAX_TEMPLATE=6*1024*1024
FIELDS=frozenset({'version','state','delegation','delegationDigest','revisionId','revisionDigest',
    'inventoryDigest','parametersDigest','retiredAt','cause','revocationStatus'})

def original_policy(value,owner,service_id):
    require(type(value) is dict and type(value.get('delegation')) is dict,'service_retention','Original template required')
    issued=value['delegation'].get('body',{}).get('issued_at')
    require(type(issued) is int,'service_retention','Original signed issue time required')
    policy=verify_delegation(encode(value['delegation']),owner,issued)
    require(policy.body['service_id']==service_id and policy.body['tenant']==dict(owner.tenant)
        and policy.body.get('execution_origin') is not None and policy.digest==value.get('delegationDigest'),
        'service_retention','Original signed service template required')
    return policy

def validate_tombstone(value,owner,service_id):
    require(type(value) is dict and set(value)==FIELDS and type(value['version']) is int and value['version']==2
        and value['state']=='retired' and type(value['retiredAt']) is int and 0<value['retiredAt']<=253402300799
        and value['cause'] in ('policyExpired','knownRevocation'),'service_retention','Unknown service retirement')
    identifier(value['revisionId']);policy=original_policy(value,owner,service_id);body=policy.body
    require(value['revisionDigest']==body['template_digest'] and value['inventoryDigest']==body['inventory_digest']
        and value['parametersDigest']==body['parameters_digest'],'service_retention','Retirement metadata changed')
    if value['cause']=='policyExpired':
        require(value['retiredAt']>=body['expires_at'] and value['revocationStatus'] is None,
            'service_retention','Retirement predates original expiry')
    else:
        status=value['revocationStatus']
        require(type(status) is dict and set(status)=={'observedAt','statement'} and type(status['observedAt']) is int
            and 0<status['observedAt']<=value['retiredAt'],'service_retention','Original observed revocation required')
        ended=retained_owner_status(encode(status['statement']),owner,service_id,policy.digest,status['observedAt'])
        require(ended['revoked'] is True,'service_retention','Local blocked metadata is not revocation')
    return value

def _identity(fd):
    m=os.fstat(fd)
    require(stat.S_ISDIR(m.st_mode) and m.st_uid==os.getuid() and stat.S_IMODE(m.st_mode)==0o700,
        'service_retention','Original private runtime required')
    return (m.st_dev,m.st_ino)

def _named(root,fd,identity):
    m=root.lstat()
    require(stat.S_ISDIR(m.st_mode) and (m.st_dev,m.st_ino)==identity and _identity(fd)==identity,
        'service_retention','Original runtime name changed')

def _read(fd,name,maximum):
    child=os.open(name,os.O_RDONLY|os.O_NOFOLLOW|os.O_CLOEXEC|os.O_NONBLOCK,dir_fd=fd)
    try:
        m=os.fstat(child)
        require(stat.S_ISREG(m.st_mode) and m.st_uid==os.getuid() and m.st_nlink==1
            and stat.S_IMODE(m.st_mode)==0o600 and m.st_size<=maximum,
            'service_retention','Unsupported private retirement artifact')
        with os.fdopen(child,'rb') as f:
            child=None;value=decode(f.read(maximum+1),max_bytes=maximum)
        return value,m
    finally:
        if child is not None:os.close(child)

def _same(fd,name,old):
    m=os.stat(name,dir_fd=fd,follow_symlinks=False)
    require(stat.S_ISREG(m.st_mode) and m.st_uid==os.getuid() and m.st_nlink==1 and stat.S_IMODE(m.st_mode)==0o600
        and (m.st_dev,m.st_ino,m.st_size)==(old.st_dev,old.st_ino,old.st_size),
        'service_retention','Original template file changed')

def _commit(fd,old,expected,value):
    actual,current=_read(fd,TEMPLATE,MAX_TEMPLATE)
    require(actual==expected,'service_retention','Template bytes changed before retirement')
    _same(fd,TEMPLATE,old)
    name='.retire-'+str(uuid.uuid4());child=None
    try:
        child=os.open(name,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW|os.O_CLOEXEC,0o600,dir_fd=fd)
        with os.fdopen(child,'wb') as out:
            child=None;out.write(encode(value));out.flush();os.fsync(out.fileno())
        _same(fd,TEMPLATE,old)
        os.rename(name,TEMPLATE,src_dir_fd=fd,dst_dir_fd=fd);os.fsync(fd)
    finally:
        if child is not None:os.close(child)
        try:os.unlink(name,dir_fd=fd)
        except FileNotFoundError:pass

def _retained_layout(fd):
    names=set(_children(fd,8))
    required={'service-version.json','service-template.json','state','scheduler','revisions'}
    require(required<=names,'service_retention','Incomplete original runtime')
    for name in names-required:
        require(name.startswith('uploads-'),'service_retention','Unknown retained runtime entry')
        identifier(name.removeprefix('uploads-'))
        child=os.open(name,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=fd)
        try:
            _identity(child);require(not _children(child,1),'service_retention','Interrupted upload requires original recovery')
        finally:os.close(child)

def _scheduler_floor(fd):
    """Fixed readonly view of the SAME held DELETE-journal database.
    /proc/self/fd references this process's held descriptor, never an ambient
    caller path. No recovery, database creation, reservation or row update.
    """
    directory=os.open('scheduler',os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=fd);database=None;connection=None
    try:
        _identity(directory);require(_children(directory,2)==['schedule.sqlite'],'service_retention','Unknown scheduler artifacts')
        database=os.open('schedule.sqlite',os.O_RDONLY|os.O_NOFOLLOW|os.O_CLOEXEC|os.O_NONBLOCK,dir_fd=directory)
        old=os.fstat(database)
        require(stat.S_ISREG(old.st_mode) and old.st_uid==os.getuid() and old.st_nlink==1 and stat.S_IMODE(old.st_mode)==0o600
            and 100<=old.st_size<=64*1024*1024 and os.pread(database,16,0)==b'SQLite format 3\0',
            'service_retention','Original scheduler file required')
        connection=sqlite3.connect('file:/proc/self/fd/'+str(database)+'?mode=ro&immutable=1',uri=True,timeout=1,isolation_level=None)
        connection.execute('PRAGMA trusted_schema=OFF');connection.execute('PRAGMA query_only=ON')
        require(connection.execute('PRAGMA user_version').fetchone()[0]==1
            and connection.execute('PRAGMA journal_mode').fetchone()[0]=='delete'
            and {r[0] for r in connection.execute("SELECT name FROM sqlite_master WHERE type='table' AND name NOT GLOB 'sqlite_*' LIMIT 4")}=={'clock_state','schedules','slots'},
            'service_retention','Unknown scheduler schema')
        rows=connection.execute('SELECT id,highwater FROM clock_state LIMIT 2').fetchall()
        require(len(rows)==1 and rows[0][0]==1 and type(rows[0][1]) is int and 0<=rows[0][1]<=253402300799,
            'service_retention','Unknown scheduler clock floor')
        _same_dir(fd,'scheduler',directory);_same(directory,'schedule.sqlite',old)
        return rows[0][1]
    finally:
        if connection is not None:connection.close()
        if database is not None:os.close(database)
        os.close(directory)

def _ended(scope,fd,value):
    policy=original_policy(value,scope.owner,scope.service_id);body=policy.body
    header,_=_read(fd,'service-version.json',8192);header=scope._validate_header(header)
    current=int(scope.clock())
    require(current>=header['clockFloor'],'service_retention','Clock predates retained floor')
    if current>=body['expires_at']:return policy,current,'policyExpired',None
    observed=header['ownerStatus']
    if not header['revoked'] or observed is None or observed['delegationDigest']!=policy.digest:return None
    ended=retained_owner_status(encode(observed['statement']),scope.owner,scope.service_id,policy.digest,observed['observedAt'])
    require(ended['revoked'] is True,'service_retention','Genuine original signed revocation required')
    return policy,current,'knownRevocation',{'observedAt':observed['observedAt'],'statement':observed['statement']}

def retire_joined(scope,expected_runtime,*,joined):
    """Private caller passes actual executor.close completion, never a UI flag.
    No job state/replay/slot update occurs. The same signed policy is checked at
    ORIGINAL issue for history, not reinterpreted as current authority.
    """
    require(joined is True,'service_retention','Actual worker join required')
    root=scope.root;fd=os.open(root,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC)
    try:
        identity=_identity(fd);require(identity==expected_runtime,'service_retention','Original runtime inode required');_named(root,fd,identity)
        value,m=_read(fd,TEMPLATE,MAX_TEMPLATE)
        if type(value) is dict and value.get('version')==2:
            validate_tombstone(value,scope.owner,scope.service_id);_named(root,fd,identity)
            return {'retired':True,'alreadyRetired':True}
        from .service_runtime import validate_template_value
        value=validate_template_value(value,scope.owner,scope.service_id)
        ended=_ended(scope,fd,value)
        if ended is None:return {'retired':False,'alreadyRetired':False}
        policy,current,cause,status=ended;body=policy.body
        _retained_layout(fd)
        require(current>=_scheduler_floor(fd),'service_retention','Clock predates original scheduler floor')
        state_fd=os.open('state',os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=fd)
        try:
            _identity(state_fd);jobs_fd=os.open('jobs',os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=state_fd)
            try:
                _identity(jobs_fd);_offline_jobs_joined(scope,jobs_fd,policy,max(current,math.ceil(scope.clock())),value['revisionId'])
                _same_dir(state_fd,'jobs',jobs_fd);_same_dir(fd,'state',state_fd)
            finally:os.close(jobs_fd)
        finally:os.close(state_fd)
        # No legacy plaintext input is orphaned by replacing its validation
        # template. Current anonymous inputs have no such disk artifact.
        for child in bounded_children(root/'state/jobs',MAX_JOBS,'service_retention'):
            identifier(child.name)
            require(not (child/'input.json').exists() and not (child/'input.json').is_symlink(),
                'service_retention','Legacy inputs require original recovery before retirement')
        retired={'version':2,'state':'retired','delegation':value['delegation'],'delegationDigest':policy.digest,
            'revisionId':value['revisionId'],'revisionDigest':body['template_digest'],
            'inventoryDigest':body['inventory_digest'],'parametersDigest':body['parameters_digest'],
            'retiredAt':current,'cause':cause,'revocationStatus':status}
        validate_tombstone(retired,scope.owner,scope.service_id)
        _named(root,fd,identity);_same(fd,TEMPLATE,m);_commit(fd,m,value,retired);_named(root,fd,identity)
        return {'retired':True,'alreadyRetired':False}
    finally:os.close(fd)


def _children(fd,maximum):
    names=[]
    with os.scandir(fd) as entries:
        for entry in entries:
            require(len(names)<maximum,'service_retention','Retained entries exceed bound')
            names.append(entry.name)
    return names

def _same_dir(parent,name,child):
    m=os.stat(name,dir_fd=parent,follow_symlinks=False)
    require(stat.S_ISDIR(m.st_mode) and (m.st_dev,m.st_ino)==_identity(child),
        'service_retention','Original retained directory changed')

def _local_worker_joined(state):
    """A local wait receipt is independent of remote success/uncertainty.
    This consumes ONLY exact fields emitted after run_bounded returned. It
    neither changes the result nor claims target join, rollback or authority.
    """
    from consolecrypt_runner import artifact_retention as history
    history.validate_state(state)
    require(state['state'] in history.TERMINAL,'service_retention_worker_active','Original worker is not terminal')
    result=state['result']
    if state['state']=='cancelled' and type(result) is dict and set(result)=={'started'} and result['started'] is False:
        return None # Positively cancelled BEFORE any worker, not an unknown wait.
    require(type(result) is dict and 'runner_execution' in result,
        'service_retention_join_missing','Original local wait observation is missing')
    ordinary={'exit_code','summary','timed_out','cancel_requested','remote_changes_may_be_partial','elapsed_ms','runner_execution'}
    after_wait={'reason','remote_changes_may_be_partial','cancel_requested','timed_out','runner_execution'}
    if set(result)==ordinary:
        require(state['state'] in ('succeeded','failed','unknown') and type(result['exit_code']) is int
            and -255<=result['exit_code']<=255 and (result['summary'] is None or type(result['summary']) is dict),
            'service_retention_join_invalid','Unknown local wait result')
        elapsed=result['elapsed_ms']
    else:
        require(set(result)==after_wait and state['state']=='unknown' and result['reason']=='executor_lost'
            and result['remote_changes_may_be_partial'] is True,
            'service_retention_join_invalid','Unknown post-wait result')
        elapsed=result['runner_execution'].get('elapsedMonotonicMs') if type(result['runner_execution']) is dict else None
    require(all(type(result[name]) is bool for name in ('timed_out','cancel_requested','remote_changes_may_be_partial')),
        'service_retention_join_invalid','Unknown local wait flags')
    interval=result['runner_execution']
    require(type(elapsed) is int and 0<=elapsed<=86400000 and type(interval) is dict
        and set(interval)=={'version','clockSource','scope','spawnObservedUtc','exitObservedUtc','elapsedMonotonicMs'}
        and type(interval['version']) is int and interval['version']==1 and interval['clockSource']=='runner-wall'
        and interval['scope']=='runner-process' and type(interval['elapsedMonotonicMs']) is int
        and interval['elapsedMonotonicMs']==elapsed,
        'service_retention_join_invalid','Unknown local process interval')
    accepted=history.timestamp(state['accepted_at']);spawn=history.timestamp(interval['spawnObservedUtc'])
    exit=history.timestamp(interval['exitObservedUtc']);activity=history.timestamp(state.get('history_activity_at',state['updated_at']))
    require(accepted<=spawn<=exit<=activity,'service_retention_join_invalid','Local wait chronology changed')
    return (accepted,spawn,exit,activity)


def _job_policy(scope,policy,manifest,state,binding,approval,current):
    """Historical signature/read validation ONLY. No current grant is returned.
    Reuse the real JobManifest verifier at the ORIGINAL admittedAt, never now.
    """
    from consolecrypt_runner.approval import TrustGrant,verify_envelope
    from consolecrypt_runner import artifact_retention as history
    body=policy.body;admitted=binding['admittedAt']
    grant=TrustGrant(bytes.fromhex(body['signer']['public_key']),body['tenant']['id'],
        frozenset(h['id'] for h in body['hosts']),frozenset([body['project_id']]),frozenset(['jobs.execute']),
        body['expires_at'],body['quotas']['max_job_seconds'])
    verified=verify_envelope(approval,{body['service_id']:grant},runner_id=body['service_id'],
        tenant_id=body['tenant']['id'],environment_digest=body['environment_digest'],now=admitted)
    require(verified.value==manifest and verified.digest==state['manifest_digest']
        and binding['tenant']==scope.tenant and binding['serviceId']==scope.service_id
        and binding['projectId']==body['project_id'] and binding['jobId']==manifest['job_id']
        and binding['manifestDigest']==verified.digest and binding['delegationDigest']==policy.digest
        and binding['scheduleId']==body['schedule_id'] and binding['scheduleRevision']==body['schedule_revision']
        and binding['definitionDigest']==body['definition_digest'] and binding['policyRevision']==body['policy_revision']
        and binding['revocationEpoch']==body['revocation_epoch']
        and body['issued_at']<=binding['dueUtc']<=admitted<body['expires_at']
        and manifest['issued_at']<=admitted<manifest['expires_at']<=min(body['expires_at'],manifest['issued_at']+300)
        and admitted<=history.timestamp(state['accepted_at'])<=current
        and manifest['project_id']==body['project_id'] and manifest['revision_digest']==body['template_digest']
        and manifest['inventory_digest']==body['inventory_digest'] and manifest['parameters_digest']==body['parameters_digest']
        and manifest['mode'] in body['modes'] and manifest['policy']['strict_isolation'] is True
        and manifest['policy']['cancel_on_lock'] is False and manifest['policy']['forks']<=body['quotas']['max_forks']
        and {t['host_id']:hashlib.sha256(encode(t)).hexdigest() for t in manifest['targets']}
            =={h['id']:h['target_digest'] for h in body['hosts']},
        'service_retention_binding','Original signed job/policy/custody chronology differs')


def _offline_jobs_joined(scope,jobs_fd,policy,current,revision_id,*,quiescent=False,template=None):
    """Strict original local wait receipts, never remote-outcome certainty.
    Missing/orphan/legacy/future receipts remain explicit refusals. No recovery,
    slot reconciliation, clock tick or original outcome/replay update occurs.
    """
    from consolecrypt_runner import artifact_retention as history
    from consolecrypt_runner.approval import DOMAIN,MANIFEST_FIELDS
    from .service_custody import _binding
    records=[]
    for job in _children(jobs_fd,MAX_JOBS):
        identifier(job)
        job_fd=os.open(job,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=jobs_fd)
        try:
            _identity(job_fd)
            names=set(_children(job_fd,10))
            require({'manifest.json','state.json','approval.json','service-custody.json'}<=names
                and names<= {'manifest.json','state.json','approval.json','revision.json',
                    'service-custody.json','service-start.json','events.jsonl','transport','admission.json'}|({'input.json'} if quiescent else set()),
                'service_retention_receipt_missing','Original signed custody records missing or unknown')
            manifest=_read(job_fd,'manifest.json',256*1024)[0];state=_read(job_fd,'state.json',256*1024)[0];history.validate_state(state)
            require(type(manifest) is dict and set(manifest)==MANIFEST_FIELDS and type(manifest.get('version')) is int and manifest['version']==1
                and manifest['job_id']==job and state['job_id']==job
                and manifest['runner_id']==scope.service_id and manifest['tenant_id']==scope.tenant['id']
                and hashlib.sha256(DOMAIN+encode(manifest)).hexdigest()==state['manifest_digest'],
                'service_retention','Foreign or changed retained job')
            joined=('observed_quiescent' if 'service-start.json' in names else None) if quiescent else _local_worker_joined(state)
            binding=_binding(_read(job_fd,'service-custody.json',8192)[0])
            approval=_read(job_fd,'approval.json',256*1024)[0]
            _job_policy(scope,policy,manifest,state,binding,approval,current)
            if 'admission.json' in names:
                from consolecrypt_runner.admission_audit import read as read_admission
                audit=read_admission(Path('/proc/self/fd/'+str(job_fd))/'admission.json',manifest,state['manifest_digest'])
                require(audit is not None and audit['signer_id']==approval['signer_id'],
                    'service_retention_binding','Original admission signer differs from actual signed job')
            ref=None
            if joined is None:
                require('service-start.json' not in names,'service_retention_join_invalid','Cancelled-before-start conflicts with actual start')
            else:
                require((quiescent or joined[2]<=current) and {'service-start.json','revision.json'}<=names,
                    'service_retention_receipt_missing','Original start/reference/local wait must be complete')
                start=_read(job_fd,'service-start.json',8192)[0]
                require(type(start) is dict and set(start)=={'version','jobId','manifestDigest','delegationDigest','revisionId','targetOutcome'}
                    and type(start['version']) is int and start['version']==1 and start['jobId']==job
                    and start['manifestDigest']==state['manifest_digest'] and start['delegationDigest']==policy.digest
                    and start['revisionId']==revision_id and start['targetOutcome'] in ('pending','known','uncertain'),
                    'service_retention_binding','Original started target metadata changed')
            if 'revision.json' in names:
                ref=_read(job_fd,'revision.json',256*1024)[0]
                require(type(ref) is dict and set(ref)=={'version','revision_id','revision_digest'}
                    and type(ref['version']) is int and ref['version']==1 and ref['revision_id']==revision_id
                    and ref['revision_digest']==manifest['revision_digest'],
                    'service_retention_binding','Original snapshot reference changed')
                identifier(ref['revision_id'])
            if 'input.json' in names:
                _legacy_input(_read(job_fd,'input.json',MAX_TEMPLATE)[0],manifest,template)
            # Return metadata needed by the existing explicit release guard;
            # no original state/result/custody/nonce record is changed here.
            records.append((state,manifest,ref,'events.jsonl' in names))
        finally:os.close(job_fd)
    return records


def ended_template_references(scope,expected_runtime,revision_id,digest):
    """Read-only service-specific eligibility for the existing release API.
    Ordinary UNKNOWN reference rules stay unchanged. Raw journal retirement and
    a full 30 days since ORIGINAL activity are required even for pinned summary.
    """
    from consolecrypt_runner import artifact_retention as history
    root=scope.root;fd=os.open(root,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC);state_fd=None;jobs_fd=None
    try:
        require(_identity(fd)==expected_runtime,'service_retention','Original runtime inode required');_named(root,fd,expected_runtime)
        value,_=_read(fd,TEMPLATE,MAX_TEMPLATE);validate_tombstone(value,scope.owner,scope.service_id)
        require(value['revisionId']==revision_id and value['revisionDigest']==digest,
            'service_retention_binding','Only exact original retired template may release')
        policy=original_policy(value,scope.owner,scope.service_id);current=int(scope.clock())
        header=scope._validate_header(_read(fd,'service-version.json',8192)[0])
        require(current>=max(value['retiredAt'],header['clockFloor'],_scheduler_floor(fd)),
            'service_retention','Clock predates original retained floor')
        state_fd=os.open('state',os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=fd);_identity(state_fd)
        jobs_fd=os.open('jobs',os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=state_fd);_identity(jobs_fd)
        records=_offline_jobs_joined(scope,jobs_fd,policy,max(current,math.ceil(scope.clock())),revision_id)
        for state,manifest,ref,raw_events in records:
            if ref is None or history.validate_state(state) is None or raw_events:return True
            if current<history.timestamp(state.get('history_activity_at',state['updated_at']))+history.SECONDS:return True
            if current<manifest['access_expires_at']:return True
        _same_dir(fd,'state',state_fd);_same_dir(state_fd,'jobs',jobs_fd);_named(root,fd,expected_runtime)
        return False
    finally:
        if jobs_fd is not None:os.close(jobs_fd)
        if state_fd is not None:os.close(state_fd)
        os.close(fd)


def retire_existing(scope,expected_runtime):
    """Fixed startup-only metadata maintenance, no executor created.
    Existing locks are acquired without creating anything. A live helper or an
    unresolved/orphan state is a hard refusal; no cleanup success is inferred.
    """
    root=scope.root;fd=os.open(root,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC);state_fd=None;jobs_fd=None;locks=[]
    try:
        require(_identity(fd)==expected_runtime,'service_retention','Original existing runtime required')
        _named(root,fd,expected_runtime)
        state_fd=os.open('state',os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=fd);_identity(state_fd)
        for name in ('.owner','.lock'):
            child=os.open(name,os.O_RDWR|os.O_NOFOLLOW|os.O_NONBLOCK|os.O_CLOEXEC,dir_fd=state_fd)
            locks.append(child);m=os.fstat(child)
            require(stat.S_ISREG(m.st_mode) and m.st_uid==os.getuid() and m.st_nlink==1 and stat.S_IMODE(m.st_mode)==0o600 and m.st_size==0,
                'service_retention','Original existing JobStore lock required')
            try:fcntl.flock(child,fcntl.LOCK_EX|fcntl.LOCK_NB)
            except BlockingIOError:require(False,'service_retention_join_required','Original helper still owns workers')
        # Existing tombstone is an idempotent denial, not reconstructed approval.
        value,_=_read(fd,TEMPLATE,MAX_TEMPLATE)
        if type(value) is dict and value.get('version')==2:
            validate_tombstone(value,scope.owner,scope.service_id)
            return {'retired':True,'alreadyRetired':True}
        from .service_runtime import validate_template_value
        validate_template_value(value,scope.owner,scope.service_id)
        if _ended(scope,fd,value) is None:return {'retired':False,'alreadyRetired':False}
        jobs_fd=os.open('jobs',os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=state_fd);_identity(jobs_fd)
        _same_dir(fd,'state',state_fd);_same_dir(state_fd,'jobs',jobs_fd)
        _offline_jobs_joined(scope,jobs_fd,original_policy(value,scope.owner,scope.service_id),math.ceil(scope.clock()),value['revisionId'])
        _same_dir(fd,'state',state_fd);_same_dir(state_fd,'jobs',jobs_fd)
        _named(root,fd,expected_runtime)
        return retire_joined(scope,expected_runtime,joined=True)
    finally:
        for child in reversed(locks):os.close(child)
        if jobs_fd is not None:os.close(jobs_fd)
        if state_fd is not None:os.close(state_fd)
        os.close(fd)


def bootstrap_retirement(root,body):
    """Only the inherited initial frame. It creates NO helper/executor/store.
    Identity/runtime are supplied by the held existing Native identity record,
    never a caller URL/path/cleanup flag or public dispatch method.
    """
    from .service_authority import PinnedOwner
    from .service_custody import ServiceCustody
    require(type(body) is dict and set(body)=={'version','owner','origin','delegation','revision_id','identity','runtime_identity'}
        and type(body['version']) is int and body['version']==1,'service_retention','Invalid private retirement frame')
    owner=body['owner']
    require(type(owner) is dict and set(owner)=={'owner_id','owner_epoch','tenant','public_key','not_after','minimum_policy_revision','minimum_revocation_epoch'},
        'service_retention','Original pinned owner required')
    pin=PinnedOwner(owner['owner_id'],owner['owner_epoch'],owner['tenant'],bytes.fromhex(owner['public_key']),
        owner['not_after'],owner['minimum_policy_revision'],owner['minimum_revocation_epoch'])
    ident=body['identity'];runtime=body['runtime_identity']
    require(type(ident) is dict and set(ident)=={'identity_id','key_epoch','signing','recipient'} and type(ident['key_epoch']) is int and ident['key_epoch']==1,
        'service_retention','Existing persistent identity required')
    identifier(ident['identity_id'])
    for field in ('signing','recipient'):
        require(type(ident[field]) is str and len(ident[field])==64 and all(c in '0123456789abcdef' for c in ident[field]),
            'service_retention','Original public identity facts required')
    require(type(runtime) is dict and set(runtime)=={'device','inode'} and type(runtime['device']) is int and 0<=runtime['device']<=2**64-1
        and type(runtime['inode']) is int and 0<runtime['inode']<=2**64-1,'service_retention','Original runtime inode required')
    signed=body['delegation'];issued=signed.get('body',{}).get('issued_at') if type(signed) is dict else None
    require(type(issued) is int,'service_retention','Original signed policy required')
    policy=verify_delegation(encode(signed),pin,issued);original=policy.body
    require(original['execution_origin']==body['origin'] and original['signer']['public_key']==ident['signing']
        and original['recipient']['public_key']==ident['recipient'],'service_retention','Original identity and policy differ')
    identifier(body['revision_id'])
    scope=ServiceCustody._restore_scope(root,pin,original['service_id'])
    # Same strict signed template/revision match before any metadata write.
    expected=(runtime['device'],runtime['inode'])
    root=Path(root);fd=os.open(root,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC)
    try:
        require(_identity(fd)==expected,'service_retention','Original runtime inode required')
        _named(root,fd,expected);value,_=_read(fd,TEMPLATE,MAX_TEMPLATE)
        require(type(value) is dict and value.get('delegation')==signed and value.get('delegationDigest')==policy.digest
            and value.get('revisionId')==body['revision_id'],'service_retention','Original enrolled template required')
        _named(root,fd,expected)
    finally:os.close(fd)
    return retire_existing(scope,expected)


def _legacy_input(value,manifest,template):
    from consolecrypt_runner.inventory import StaticInventory
    from consolecrypt_runner.parameters import validate_parameters,parameters_digest
    require(type(template) is dict and template['version']==1 and type(value) is dict
        and set(value)=={'playbook','mode','forks','inventory','parameters'}
        and value['playbook']==manifest['playbook'] and value['mode']==manifest['mode']
        and type(value['forks']) is int and value['forks']==manifest['policy']['forks'],
        'service_retention_legacy','Legacy raw inputs cannot be bound to original template')
    parameters=validate_parameters(value['parameters']);inventory=StaticInventory.from_snapshot(template['inventory'])
    require('vault' not in parameters and parameters_digest(parameters)==manifest['parameters_digest']
        and inventory.digest==manifest['inventory_digest'] and inventory.runtime_inventory(manifest)==value['inventory'],
        'service_retention_legacy','Legacy raw inputs differ from original signed job')


def _raw_end_time(scope,fd,policy):
    end=policy.body['expires_at'];header=scope._validate_header(_read(fd,'service-version.json',8192)[0])
    status=header['ownerStatus']
    if header['revoked'] and status is not None and status['delegationDigest']==policy.digest:
        original=retained_owner_status(encode(status['statement']),scope.owner,scope.service_id,policy.digest,status['observedAt'])
        require(original['revoked'] is True,'service_retention','Original signed revocation required')
        end=min(end,status['observedAt'])
    return end


def _raw_context(scope,expected_runtime,expected_policy):
    """Read-only whole supported-layout preflight. Returned FDs stay original.
    Caller closes all FDs. This is not a new Store/recovery/queue constructor.
    """
    from consolecrypt_runner import artifact_retention as history
    original=(expected_runtime.st_dev,expected_runtime.st_ino) if hasattr(expected_runtime,'st_dev') else expected_runtime
    fds=[]
    try:
        fd=os.open(scope.root,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC);fds.append(fd)
        require(_identity(fd)==original,'service_retention','Original runtime inode required');_named(scope.root,fd,original)
        value,m=_read(fd,TEMPLATE,MAX_TEMPLATE)
        from .service_runtime import validate_template_value
        value=validate_template_value(value,scope.owner,scope.service_id);policy=original_policy(value,scope.owner,scope.service_id)
        require(policy.digest==expected_policy,'service_retention_binding','Original requested policy changed')
        ended=_ended(scope,fd,value)
        require(ended is not None,'service_retention_policy_live','Original policy is neither expired nor genuinely revoked')
        policy,current,cause,status=ended;_retained_layout(fd)
        require(current>=_scheduler_floor(fd),'service_retention','Clock predates original scheduler floor')
        require(current>=_raw_end_time(scope,fd,policy)+history.SECONDS,
            'service_retention_ttl','Original raw template retention interval has not elapsed')
        state=os.open('state',os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=fd);fds.append(state);_identity(state)
        require(set(_children(state,5))=={'.owner','.lock','jobs','nonces'},'service_retention','Unknown original state layout')
        jobs=os.open('jobs',os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=state);fds.append(jobs);_identity(jobs)
        records=_offline_jobs_joined(scope,jobs,policy,max(current,math.ceil(scope.clock())),value['revisionId'],quiescent=True,template=value)
        for job_state,_,_,_ in records:
            if 'local_quiescence' in job_state:
                observation=job_state['local_quiescence']
                require(observation['policy_digest']==policy.digest
                    and (observation['runtime_device'],observation['runtime_inode'])==original,
                    'service_retention_binding','Original quiescence record belongs to another runtime/policy')
            require(current>=history.timestamp(job_state.get('history_activity_at',job_state['updated_at']))+history.SECONDS,
                'service_retention_ttl','Original job raw-output retention interval has not elapsed')
        nonces=os.open('nonces',os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=state);fds.append(nonces);_identity(nonces)
        known={v['job_id']:v for _,v,_,_ in records}
        for name in _children(nonces,MAX_JOBS):
            identifier(name);nonce=_read(nonces,name,8192)[0]
            require(type(nonce) is dict and set(nonce)=={'job_id','manifest_digest','expires_at'}
                and type(nonce['expires_at']) is int and 0<nonce['expires_at']<=min(current,policy.body['expires_at'])
                and type(nonce['manifest_digest']) is str and len(nonce['manifest_digest'])==64
                and all(c in '0123456789abcdef' for c in nonce['manifest_digest']),
                'service_retention_future_reference','Unknown or future nonce must remain pinned')
            identifier(nonce['job_id'])
            if nonce['job_id'] in known:
                manifest=known[nonce['job_id']]
                require(name==manifest['nonce'] and nonce['expires_at']==manifest['expires_at']
                    and nonce['manifest_digest']==hashlib.sha256(__import__('consolecrypt_runner.approval',fromlist=['DOMAIN']).DOMAIN+encode(manifest)).hexdigest(),
                    'service_retention_binding','Original nonce binding changed')
        _same_dir(fd,'state',state);_same_dir(state,'jobs',jobs);_same_dir(state,'nonces',nonces);_named(scope.root,fd,original)
        return fds,(fd,state,jobs,value,m,policy,current,cause,status,records)
    except BaseException:
        for item in reversed(fds):os.close(item)
        raise


def quiescent_preflight(scope,expected_runtime,expected_policy):
    fds,context=_raw_context(scope,expected_runtime,expected_policy)
    try:return (expected_policy,hashlib.sha256(encode(context[3])).hexdigest())
    finally:
        for fd in reversed(fds):os.close(fd)



def _revision_caches(child,snapshot):
    """Closed current-format raw caches; future/foreign children are preserved."""
    from consolecrypt_runner.validation_reports import FIELDS as REPORT_FIELDS,report_key,MAX_REPORTS,MAX_BYTES
    from consolecrypt_runner.inventory_sources import MAX_CAPTURES,MAX_REVISION_BYTES
    from consolecrypt_runner.inventory import StaticInventory,MAX_INVENTORY_BYTES
    metadata,_=_read(child,'snapshot.json',32*1024*1024)
    require(metadata==snapshot.to_json(),'service_retention','Unknown original snapshot metadata')
    names=set(_children(child,5))
    for folder,limit,budget in (('validations',MAX_REPORTS,MAX_BYTES),('inventory-previews',MAX_CAPTURES,MAX_REVISION_BYTES)):
        if folder not in names:continue
        directory=os.open(folder,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=child)
        try:
            _identity(directory);used=0
            for name in _children(directory,limit):
                stem=name.removesuffix('.json')
                require(name==stem+'.json' and len(stem)==64 and all(c in '0123456789abcdef' for c in stem),
                    'service_retention','Unknown original raw cache entry')
                value,info=_read(directory,name,MAX_INVENTORY_BYTES if folder=='inventory-previews' else 256*1024)
                used+=info.st_size;require(used<=budget,'service_retention','Original raw cache exceeds bound')
                if folder=='validations':
                    require(set(REPORT_FIELDS)|{'valid'}<=set(value)<=set(REPORT_FIELDS)|{'valid','checks','declared_privileges'}
                        and type(value['valid']) is bool and value['revision_digest']==snapshot.digest and report_key(value)==stem,
                        'service_retention','Unknown original validation cache')
                    if 'checks' in value:
                        require(type(value['checks']) is list and len(value['checks'])<=2,
                            'service_retention','Unknown validation check list')
                        for check in value['checks']:
                            require(type(check) is dict and set(check)=={'name','exit_code','output','truncated','cancelled','timed_out','elapsed_ms'}
                                and check['name'] in ('syntax','lint') and type(check['output']) is str
                                and type(check['exit_code']) is int and type(check['elapsed_ms']) is int
                                and all(type(check[k]) is bool for k in ('truncated','cancelled','timed_out')),
                                'service_retention','Unknown validation check schema')
                    if 'declared_privileges' in value and value['declared_privileges'] is not None:
                        require(type(value['declared_privileges']) is dict and type(value['declared_privileges'].get('version')) is int
                            and value['declared_privileges']['version']==1,'service_retention','Future declared metadata is preserved')
                else:
                    parsed=StaticInventory.from_snapshot(value)
                    require(hashlib.sha256(encode(value)).hexdigest()==stem and encode(parsed.to_snapshot())==encode(value),
                        'service_retention','Original inventory cache changed')
                _same(directory,name,info)
            _same_dir(child,folder,directory)
        finally:os.close(directory)

def _release_original_revision(scope,fd,value,policy,proof):
    """Existing bounded repository release, exact original UUID only.
    No user source path or nonce/ref/journal metadata is deleted.
    """
    from consolecrypt_runner.revisions import RevisionRepository
    from consolecrypt_runner.snapshot import verify_snapshot
    revisions=os.open('revisions',os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=fd)
    try:
        _identity(revisions);names=_children(revisions,64)
        for name in names:identifier(name)
        if value['revisionId'] not in names:
            require(value['version']==2,'service_retention','Missing original raw revision before tombstone')
            return # Idempotent completion after an earlier exact release.
        child=os.open(value['revisionId'],os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=revisions)
        try:
            _identity(child);known=set(_children(child,5))
            require({'project','snapshot.json'}<=known and known<={'project','snapshot.json','validations','inventory-previews'},
                'service_retention','Unknown original revision layout')
            # The same repository/snapshot validators used by start/release.
            repository=RevisionRepository(scope.root/'revisions')
            project,snapshot=repository.read(value['revisionId'])
            require(snapshot.digest==policy.body['template_digest'],'service_retention_binding','Original revision digest differs')
            _revision_caches(child,snapshot)
            verify_snapshot(project,snapshot);proof.check();_same_dir(revisions,value['revisionId'],child)
            def referenced(revision_id,digest):
                require(revision_id==value['revisionId'] and digest==snapshot.digest,
                    'service_retention_binding','Only the exact original revision may release')
                proof.check()
                return False # Whole original signed reference set was checked under original held locks.
            repository.release(value['revisionId'],referenced=referenced)
            os.fsync(revisions);_same_dir(fd,'revisions',revisions)
        finally:os.close(child)
    finally:os.close(revisions)


def retire_quiescent(scope,expected_runtime,proof,captured):
    from consolecrypt_runner import artifact_retention as history
    from .service_quiescence import _ObservedQuiescence
    require(type(proof) is _ObservedQuiescence,'service_retention_quiescence','Actual original OS custody required')
    proof.check();fds,context=_raw_context(scope,expected_runtime,captured[0])
    try:
        fd,state_fd,jobs_fd,value,old,policy,current,cause,status,records=context
        require(hashlib.sha256(encode(value)).hexdigest()==captured[1],
            'service_retention_binding','Original template changed while waiting for OS quiescence')
        metadata=proof.metadata(policy.digest)
        observed_now=int(scope.clock())
        require(observed_now>=max(current,metadata['observed_at']),'service_retention','Clock moved behind original quiescence')
        current=observed_now
        require((metadata['runtime_device'],metadata['runtime_inode'])==_identity(fd),
            'service_retention_quiescence','Original OS/runtime binding differs')
        for state,_,_,_ in records:
            if 'local_quiescence' in state:
                observation=state['local_quiescence']
                require(observation['identity']==proof.identity
                    and (observation['identity_device'],observation['identity_inode'])==(metadata['identity_device'],metadata['identity_inode']),
                    'service_retention_binding','Original service identity differs from retained quiescence')
        # All records/policy/TTL/future references passed BEFORE the first write.
        for state,manifest,_,_ in records:
            proof.check();directory=scope.root/'state/jobs'/state['job_id']
            state=history.recover_quiescent(directory,state,metadata,current)
            state,_=history.retire(directory,state,current,cause='service_quiescent')
            require(history.validate_state(state) is not None,'service_retention','Raw journal retirement incomplete')
            job_fd=os.open(state['job_id'],os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=jobs_fd)
            try:
                _identity(job_fd)
                try:_,info=_read(job_fd,'input.json',MAX_TEMPLATE)
                except FileNotFoundError:continue
                # Already validated against the original signed template in preflight.
                proof.check();_same(job_fd,'input.json',info);os.unlink('input.json',dir_fd=job_fd);os.fsync(job_fd)
            finally:os.close(job_fd)
        proof.check()
        if value['version']==1:
            body=policy.body
            retired={'version':2,'state':'retired','delegation':value['delegation'],'delegationDigest':policy.digest,
                'revisionId':value['revisionId'],'revisionDigest':body['template_digest'],'inventoryDigest':body['inventory_digest'],
                'parametersDigest':body['parameters_digest'],'retiredAt':current,'cause':cause,'revocationStatus':status}
            validate_tombstone(retired,scope.owner,scope.service_id);_commit(fd,old,value,retired);value=retired
        proof.check();_release_original_revision(scope,fd,value,policy,proof)
        _same_dir(fd,'state',state_fd);_same_dir(state_fd,'jobs',jobs_fd);_named(scope.root,fd,_identity(fd))
        return {'rawRetired':True,'originalOutcomesPreserved':True,'localQuiescence':metadata['source']}
    finally:
        for fd in reversed(fds):os.close(fd)
