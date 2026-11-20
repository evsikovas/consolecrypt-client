"""Private durable signed queue. No service delegation, credentials or dispatch."""
from __future__ import annotations
import base64
from dataclasses import dataclass
import hashlib
import os
from pathlib import Path
import re
import stat
import time
from consolecrypt_runner.approval import DOMAIN, MANIFEST_FIELDS, TrustGrant, verify_envelope
from consolecrypt_runner.errors import RunnerError, require
from consolecrypt_runner.protocol import decode, encode, uuid_field, MAX_FRAME_BYTES
from consolecrypt_runner.store import JobStore, atomic_json, _private_directory
from consolecrypt_runner import artifact_retention as history

MAX_JOBS = 128
MAX_MESSAGE = 256 * 1024
HEADER = {'version': 1, 'mode': 'signed-queue-only'}
STATES = frozenset({'queued','running','cancelling','succeeded','failed','cancelled','interrupted','unknown'})
STATE_FIELDS = frozenset({'version','job_id','manifest_digest','state','sequence','output_bytes',
                         'output_truncated','accepted_at','updated_at','result'})
DIGEST = re.compile(r'^[0-9a-f]{64}$')

def identifier(value):
    result = uuid_field(value)
    require(result != '00000000-0000-0000-0000-000000000000', 'invalid_id', 'Nonzero ID required')
    return result

def record(path, maximum=MAX_MESSAGE):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC)
    with os.fdopen(fd, 'rb') as stream:
        info = os.fstat(stream.fileno())
        require(stat.S_ISREG(info.st_mode) and info.st_nlink == 1 and info.st_uid == os.getuid()
                and info.st_mode & 0o077 == 0 and info.st_size <= maximum,
                'service_storage', 'Unsupported private queue record')
        return decode(stream.read(maximum + 1), max_bytes=maximum)

@dataclass(frozen=True, repr=False)
class NativeQueueEnrollment:
    """Original private Native control DATA, never persistent service delegation."""
    runner_id: str
    tenant_id: str
    signer_id: str
    grant: TrustGrant
    environment_digest: str

    @classmethod
    def from_native(cls, value):
        fields = {'version','transport','runner_id','tenant_id','signer_id','public_key',
                  'hosts','projects','expires_at','max_job_seconds','environment_digest'}
        require(type(value) is dict and set(value) == fields and type(value['version']) is int
                and value['version'] == 1 and value['transport'] == 'native_scoped_rpc_v1',
                'service_enrollment', 'Original Native queue scope required')
        for name in ('runner_id','tenant_id','signer_id'): identifier(value[name])
        for name in ('hosts','projects'):
            require(type(value[name]) is list and len(value[name]) <= 128
                    and all(type(v) is str for v in value[name])
                    and len(set(value[name])) == len(value[name]), 'service_enrollment','Invalid scope')
            for item in value[name]: identifier(item)
        try: key = base64.b64decode(value['public_key'], validate=True)
        except (TypeError, ValueError):
            raise RunnerError('service_enrollment','Invalid queue key') from None
        now = int(time.time())
        require(len(key) == 32 and type(value['expires_at']) is int
                and now < value['expires_at'] <= now + 3600
                and type(value['max_job_seconds']) is int and 1 <= value['max_job_seconds'] <= 3600
                and type(value['environment_digest']) is str and DIGEST.fullmatch(value['environment_digest']),
                'service_enrollment','Invalid enrollment bounds')
        grant = TrustGrant(key,value['tenant_id'],frozenset(value['hosts']),frozenset(value['projects']),
                           frozenset({'jobs.execute'}),value['expires_at'],value['max_job_seconds'])
        return cls(value['runner_id'],value['tenant_id'],value['signer_id'],grant,value['environment_digest'])

def check_header(path):
    value = record(path,1024)
    require(type(value) is dict and set(value) == set(HEADER)
            and type(value['version']) is int and value == HEADER,
            'service_schema','Unsupported queue schema')

def bounded_children(path, maximum, code='job_quota'):
    children = []
    with os.scandir(path) as entries:
        for entry in entries:
            require(len(children) < maximum, code, 'Retained directory entry quota reached')
            children.append(Path(entry.path))
    return children

class QueueStore(JobStore):
    """Same JobStore. Validate unknown versions before its recovery writes."""
    EXTRA_FILES = frozenset()
    EXECUTION_JOURNAL = False

    def _preflight_extra(self, directory, names):
        pass # Default signed-queue v1 has no extension records.

    def _preflight_execution_journal(self,directory,names):
        if not self.EXECUTION_JOURNAL or 'events.jsonl' not in names:return 0
        fd=os.open(directory/'events.jsonl',os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK|os.O_CLOEXEC)
        with os.fdopen(fd,'rb') as stream:
            info=os.fstat(stream.fileno())
            require(stat.S_ISREG(info.st_mode) and info.st_nlink==1 and info.st_uid==os.getuid()
                and info.st_mode&0o077==0 and info.st_size<=self.output_limit,'service_schema','Unsupported execution journal')
            sequence=0
            while raw:=stream.readline(MAX_FRAME_BYTES+1):
                require(len(raw)<=MAX_FRAME_BYTES and raw.endswith(b'\n'),'service_schema','Incomplete execution journal')
                event=decode(raw)
                require(type(event) is dict and set(event)=={'version','job_id','sequence','timestamp','type','payload'}
                    and type(event['version']) is int and event['version']==1 and event['job_id']==directory.name
                    and type(event['sequence']) is int and event['sequence']==sequence+1
                    and type(event['timestamp']) is str and len(event['timestamp'])<=128
                    and type(event['type']) is str and 0<len(event['type'])<=64,'service_schema','Unsupported execution journal')
                sequence+=1
            return info.st_size

    def _recover_journals(self):
        with self.locked():
            children = sorted(bounded_children(self.root/'jobs', self.max_jobs),key=lambda p:p.name)
            require(len(children) <= self.max_jobs,'job_quota','Retained queue exceeds quota')
            execution_bytes=0
            for child in children:
                directory = self.job_directory(identifier(child.name))
                names = {p.name for p in bounded_children(directory, 5 + len(self.EXTRA_FILES) + int(self.EXECUTION_JOURNAL), 'service_schema')}
                require(names <= {'manifest.json','state.json','approval.json','revision.json','admission.json'} | self.EXTRA_FILES | ({'events.jsonl'} if self.EXECUTION_JOURNAL else set()),
                        'service_schema','Unsupported queue files')
                self._preflight_extra(directory, names)
                execution_bytes+=self._preflight_execution_journal(directory,names)
                require(execution_bytes<=self.total_output_limit,'service_schema','Execution journal aggregate quota exceeded')
                if not names: continue
                require('manifest.json' in names,'service_schema','Incomplete queue journal')
                manifest = record(directory/'manifest.json')
                require(type(manifest) is dict and set(manifest) == MANIFEST_FIELDS
                        and type(manifest['version']) is int and manifest['version'] == 1
                        and manifest['job_id'] == child.name,'service_schema','Unsupported retained manifest')
                for key in ('tenant_id','runner_id','project_id','job_id','nonce'): identifier(manifest[key])
                digest = hashlib.sha256(DOMAIN+encode(manifest)).hexdigest()
                if 'admission.json' in names:
                    from ...admission_audit import read as read_admission_audit
                    require(read_admission_audit(directory/'admission.json',manifest,digest) is not None,
                            'service_schema','Retained admission audit changed')
                if 'state.json' in names:
                    state = record(directory/'state.json')
                    require(type(state) is dict and STATE_FIELDS <= set(state)
                            and set(state) <= STATE_FIELDS|history.EXTRA
                            and type(state['version']) is int and state['version'] == 1
                            and state['job_id'] == child.name and state['manifest_digest'] == digest
                            and type(state['state']) is str and state['state'] in STATES and type(state['sequence']) is int
                            and 0 <= state['sequence'] <= 1_000_000_000 and type(state['output_bytes']) is int
                            and 0 <= state['output_bytes'] <= self.output_limit
                            and (self.EXECUTION_JOURNAL or (state['sequence']==0 and state['output_bytes']==0)),'service_schema','Unsupported retained queue state')
                if 'state.json' in names:history.validate_state(state)
                if 'approval.json' in names:
                    approval = record(directory/'approval.json')
                    require(type(approval) is dict and set(approval) == {'signer_id','manifest','signature'}
                            and approval['manifest'] == manifest,'service_schema','Unsupported retained signature record')
                    if 'admission.json' in names:
                        require(read_admission_audit(directory/'admission.json',manifest,digest)['signer_id']==approval['signer_id'],
                                'service_schema','Original audit signer differs from retained signature')
                if 'revision.json' in names:
                    ref = record(directory/'revision.json')
                    require(type(ref) is dict and set(ref) == {'version','revision_id','revision_digest'}
                            and type(ref['version']) is int and ref['version'] == 1
                            and ref['revision_digest'] == manifest['revision_digest'],
                            'service_schema','Unsupported revision reference')
                    identifier(ref['revision_id'])
            nonces = bounded_children(self.root/'nonces', MAX_JOBS)
            require(len(nonces) <= MAX_JOBS,'job_quota','Retained nonce quota exceeded')
            for path in nonces:
                identifier(path.name)
                nonce = record(path)
                require(type(nonce) is dict and set(nonce) == {'job_id','manifest_digest','expires_at'}
                        and type(nonce['manifest_digest']) is str and DIGEST.fullmatch(nonce['manifest_digest'])
                        and type(nonce['expires_at']) is int,'service_schema','Unsupported retained nonce')
                identifier(nonce['job_id'])
        super()._recover_journals()

class SignedQueue:
    """Queue custody only. No executor, target channel or scheduler dispatch."""
    def __init__(self, root, enrollment, *, max_jobs=MAX_JOBS):
        require(type(enrollment) is NativeQueueEnrollment,'service_enrollment','Original queue scope required')
        require(type(max_jobs) is int and 0 < max_jobs <= MAX_JOBS,'store_limits','Invalid queue quota')
        self.scope = enrollment
        self.root = Path(root).absolute()
        _private_directory(self.root)
        header = self.root/'service-version.json'
        if header.exists() or header.is_symlink():
            check_header(header)
        else:
            require(not any(self.root.iterdir()),'service_schema','Unidentified existing service root')
            atomic_json(header,HEADER,max_bytes=1024)
        self.store = QueueStore(self.root/'state',max_jobs=max_jobs)
        try:
            self.recovered = len(self.store.recover_interactive())
            _private_directory(self.root/'projects')
        except BaseException:
            self.store.close()
            raise

    def close(self): self.store.close()

    def _scope(self):
        require(int(time.time()) < self.scope.grant.expires_at,
                'service_enrollment_expired','Original queue scope expired')
        check_header(self.root/'service-version.json')

    def _view(self,state):
        require(type(state) is dict and STATE_FIELDS <= set(state)
                and set(state) <= STATE_FIELDS|history.EXTRA
                and type(state['version']) is int and state['version'] == 1
                and type(state['state']) is str and state['state'] in STATES,
                'service_schema','Unsupported retained queue state')
        history.validate_state(state)
        manifest = record(self.store.job_directory(state['job_id'])/'manifest.json')
        require(manifest['tenant_id'] == self.scope.tenant_id and manifest['runner_id'] == self.scope.runner_id
                and manifest['project_id'] in self.scope.grant.allowed_projects,
                'job_identity','Job outside original queue scope')
        return {'version':1,'jobId':state['job_id'],'projectId':manifest['project_id'],
                'state':state['state'],'manifestDigest':state['manifest_digest'],
                'serviceAuthorized':False,'dispatch':'blockedServiceDelegation','inputs':'notVerifiedForExecution'}

    def accept(self,body):
        self._scope()
        require(type(body) is dict and set(body) == {'approval'},'job_fields','Unsupported queue submission')
        require(len(encode(body)) <= MAX_MESSAGE,'message_size','Queue envelope exceeds limit')
        verified = verify_envelope(body['approval'],{self.scope.signer_id:self.scope.grant},
                    runner_id=self.scope.runner_id,tenant_id=self.scope.tenant_id,
                    environment_digest=self.scope.environment_digest)
        # The existing owner lock excludes other processes; its RLock serializes
        # this exact journal+signature+workspace bundle without a second writer.
        with self.store._mutex:
            if not self.store.job_directory(verified.value['job_id']).exists():
                require(len(bounded_children(self.store.root/'nonces', MAX_JOBS)) < MAX_JOBS,
                        'job_quota','Replay tombstone capacity reached')
            self._scope()
            state,created = self.store.accept(verified)
            if created:
                atomic_json(self.store.job_directory(state['job_id'])/'approval.json',body['approval'])
            project = self.root/'projects'/identifier(verified.value['project_id'])
            _private_directory(project)
            _private_directory(project/identifier(verified.value['job_id']))
            return {'created':created,'durable':True,'job':self._view(state)}

    def request(self,operation,body):
        require(type(body) is dict,'invalid_request','Object required')
        if operation == 'probe':
            require(not body,'request_fields','Probe takes no fields')
            return {'version':1,'durableQueue':True,'persistentServiceIdentity':False,
                    'dispatch':False,'scheduledDispatch':False,'recovered':self.recovered}
        self._scope()
        if operation == 'accept': return self.accept(body)
        if operation == 'get':
            require(set(body) == {'job_id'},'job_fields','Exact job ID required')
            return self._view(self.store.get(identifier(body['job_id'])))
        if operation == 'dispatch':
            require(set(body) == {'job_id'},'job_fields','Exact job ID required')
            identifier(body['job_id'])
            raise RunnerError('service_delegation_required','Independent service delegation and signed slot binding required')
        raise RunnerError('unsupported_operation','Unsupported private queue operation')
