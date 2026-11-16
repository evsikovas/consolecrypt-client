"""Same durable JobStore: slot/replay/quota custody, never job authority."""
from __future__ import annotations
from datetime import datetime, timezone
from pathlib import Path
import hashlib,time,uuid,os,stat
from dataclasses import replace
from .cc_service_queue.queue import QueueStore,record,identifier,MAX_JOBS,DIGEST,bounded_children
from consolecrypt_runner.approval import TrustGrant,verify_envelope
from consolecrypt_runner.errors import RunnerError,require
from consolecrypt_runner.protocol import encode
from consolecrypt_runner.store import atomic_json,_private_directory
from .service_authority_v4 import PinnedOwner,capture_original_slot,verify_job_evidence,observe_owner_status,retained_owner_status
from .service_authority import integer
from consolecrypt_runner.protocol import decode

HEADER={'version':4,'mode':'service-slot-custody-only'}
FILE='service-custody.json'
DOMAIN=b'ConsoleCrypt ServiceLogicalSlotJobId v1\0'

def logical_job_id(tenant,service_id,project_id,schedule_id,local_slot):
    """Stable across policy/schedule edits: new content must conflict, not retry.

    Revisions, UTC due time, digests and signatures stay in the signed binding;
    omitting them from this identity prevents edits/renewals resetting replay.
    """
    require(type(tenant) is dict and set(tenant)=={'kind','id'}
            and tenant['kind'] in ('localProfile','personalAccount'),'service_slot','Typed tenant required')
    for value in [tenant['id'],service_id,project_id,schedule_id]:identifier(value)
    require(type(local_slot) is str and len(local_slot)==16,'service_slot','Invalid original local slot')
    try: parsed=datetime.strptime(local_slot,'%Y-%m-%dT%H:%M')
    except ValueError:raise RunnerError('service_slot','Invalid original local slot') from None
    require(parsed.strftime('%Y-%m-%dT%H:%M')==local_slot,'service_slot','Noncanonical local slot')
    raw=bytearray(hashlib.sha256(DOMAIN+encode({'tenant':tenant,'serviceId':service_id,'projectId':project_id,
        'scheduleId':schedule_id,'localSlot':local_slot})).digest()[:16])
    raw[6]=(raw[6]&15)|128;raw[8]=(raw[8]&63)|128
    return str(uuid.UUID(bytes=bytes(raw))) # RFC9562 custom version8, fixed domain above.

def _binding(value):
    fields={'version','tenant','serviceId','projectId','scheduleId','scheduleRevision','definitionDigest',
        'localSlot','dueUtc','jobId','manifestDigest','delegationDigest','admittedAt','policyRevision','revocationEpoch'}
    require(type(value) is dict and set(value)==fields and type(value['version']) is int and value['version']==1,
            'service_schema','Unsupported original service custody record')
    for name in ['jobId','serviceId','projectId','scheduleId']:identifier(value[name])
    for name in ['manifestDigest','delegationDigest','definitionDigest']:
        require(type(value[name]) is str and DIGEST.fullmatch(value[name]),'service_schema','Invalid custody digest')
    for name in ['scheduleRevision','dueUtc','admittedAt','policyRevision','revocationEpoch']:
        require(type(value[name]) is int and 0<value[name]<=253402300799,'service_schema','Invalid custody bounds')
    require(logical_job_id(value['tenant'],value['serviceId'],value['projectId'],value['scheduleId'],value['localSlot'])==value['jobId'],
            'service_schema','Custody identity differs from original slot')
    return value

class CustodyStore(QueueStore):
    EXTRA_FILES=frozenset({FILE,'service-start.json','input.json','transport'})
    EXECUTION_JOURNAL=True
    def _preflight_extra(self,directory,names):
        if 'transport' in names:
            path=directory/'transport';info=path.lstat()
            require(stat.S_ISDIR(info.st_mode) and info.st_uid==os.getuid() and info.st_mode&0o077==0,
                'service_schema','Unsupported retained transport')
            children=bounded_children(path,1,'service_schema')
            for child in children:
                info=child.lstat()
                require(child.name=='broker.sock' and stat.S_ISSOCK(info.st_mode) and info.st_nlink==1
                    and info.st_uid==os.getuid() and info.st_mode&0o077==0,'service_schema','Unsupported retained transport')
        if 'input.json' in names:
            from consolecrypt_runner.parameters import validate_parameters,parameters_digest
            from consolecrypt_runner.inventory import StaticInventory
            value=record(directory/'input.json',6*1024*1024);manifest=record(directory/'manifest.json')
            template=record(self.root.parent/'service-template.json',6*1024*1024)
            require(type(value) is dict and set(value)=={'playbook','mode','forks','inventory','parameters'}
                and value['playbook']==manifest['playbook'] and value['mode']==manifest['mode']
                and type(value['forks']) is int and value['forks']==manifest['policy']['forks'],
                'service_schema','Unsupported retained service inputs')
            checked=validate_parameters(value['parameters'])
            inventory=StaticInventory.from_snapshot(template['inventory'])
            require('vault' not in checked and parameters_digest(checked)==manifest['parameters_digest']
                and inventory.digest==manifest['inventory_digest'] and inventory.runtime_inventory(manifest)==value['inventory'],
                'service_schema','Retained service inputs differ from signed manifest')

        if 'service-start.json' in names:
            value=record(directory/'service-start.json',8192)
            require(type(value) is dict and set(value)=={'version','jobId','manifestDigest','delegationDigest','revisionId','targetOutcome'}
                and type(value['version']) is int and value['version']==1
                and value['jobId']==directory.name and value['targetOutcome'] in ('pending','known','uncertain'),'service_schema','Unsupported service start record')
            identifier(value['revisionId'])
            require(all(type(value[n]) is str and DIGEST.fullmatch(value[n]) for n in ['manifestDigest','delegationDigest']),
                'service_schema','Unsupported service start record')
            require(FILE in names and 'revision.json' in names,'service_schema','Incomplete service start reference')
            binding=_binding(record(directory/FILE,8192));ref=record(directory/'revision.json')
            require(value['delegationDigest']==binding['delegationDigest'] and value['manifestDigest']==binding['manifestDigest']
                and value['revisionId']==ref['revision_id'],'service_schema','Service start differs from original custody')

        if FILE in names:
            value=_binding(record(directory/FILE,8192))
            require(value['jobId']==directory.name,'service_schema','Foreign custody record')
            if 'state.json' in names:
                require(value['manifestDigest']==record(directory/'state.json')['manifest_digest'],
                        'service_schema','Custody differs from retained job')

class ServiceCustody:
    """Single existing owner/RLock/JobStore, no second DB/outbox/nonce cleanup.

    Acceptance is custody only. Scheduler reservation is read, never resolved;
    dispatch must repeat its original state and fresh authority after custody.
    """
    def _set_scope(self,root,pinned_owner,service_id,clock):
        require(type(pinned_owner) is PinnedOwner,'service_owner','Original Native owner pin required')
        self.root=Path(root).absolute()
        self.owner=pinned_owner;self.tenant=dict(pinned_owner.tenant);self.service_id=identifier(service_id);self.clock=clock
        self.header={**HEADER,'tenant':self.tenant,'serviceId':self.service_id,'ownerId':self.owner.owner_id,
            'ownerEpoch':self.owner.owner_epoch,'ownerKeySha256':hashlib.sha256(self.owner.public_key).hexdigest()}

    @classmethod
    def _restore_scope(cls,root,pinned_owner,service_id):
        scope=cls.__new__(cls)
        scope._set_scope(root,pinned_owner,service_id,time.time)
        return scope # observation only: no JobStore/accept/dispatch exists.

    def __init__(self,root,pinned_owner,service_id,*,clock=time.time,_existing_only=False):
        self._set_scope(root,pinned_owner,service_id,clock)
        if _existing_only:
            from .service_resume import directory,regular
            for relative in ('','state','state/jobs','state/nonces','scheduler','revisions'):directory(self.root/relative)
            for name in ('.lock','.owner'):regular(self.root/'state'/name,0)
            regular(self.root/'service-version.json',8192);regular(self.root/'service-template.json',6*1024*1024)
        else:_private_directory(self.root)
        header=self.root/'service-version.json'
        if header.exists() or header.is_symlink():
            self._validate_header(record(header,8192))
        else:
            require(not _existing_only,'service_restore','Original custody header missing')
            require(not any(self.root.iterdir()),'service_schema','Fresh explicit custody root required')
            atomic_json(header,{**self.header,'clockFloor':0,'policyFloor':self.owner.minimum_policy_revision,'revocationFloor':self.owner.minimum_revocation_epoch,'ownerStatus':None,'revoked':False},max_bytes=8192)
        if (self.root/'service-template.json').exists() or (self.root/'service-template.json').is_symlink():
            from .service_runtime import validate_template_record
            validate_template_record(self.root,self.owner,self.service_id)
        self.store=CustodyStore(self.root/'state',max_jobs=MAX_JOBS)
        try:self.recovered=self.store.recover_interactive()
        except BaseException:self.store.close();raise

    def close(self):self.store.close()

    def _validate_header(self,value):
        require(type(value) is dict and set(value)==set(self.header)|{'clockFloor','policyFloor','revocationFloor','ownerStatus','revoked'}
            and all(value[k]==v for k,v in self.header.items()) and type(value['version']) is int and type(value['ownerEpoch']) is int,
            'service_schema','Unsupported custody root')
        integer(value['clockFloor'],0,253402300799);integer(value['policyFloor']);integer(value['revocationFloor'])
        require(type(value['revoked']) is bool and value['policyFloor']>=self.owner.minimum_policy_revision
            and value['revocationFloor']>=self.owner.minimum_revocation_epoch,'service_policy_rollback','Retained owner floor changed')
        status=value['ownerStatus']
        if status is not None:
            require(type(status) is dict and set(status)=={'delegationDigest','observedAt','statement'},'service_schema','Unsupported owner status')
            integer(status['observedAt'],1,253402300799)
            require(type(status['delegationDigest']) is str and DIGEST.fullmatch(status['delegationDigest'])
                and status['observedAt']<=value['clockFloor'],'service_schema','Unsupported owner status')
            observed=retained_owner_status(encode(status['statement']),self.owner,self.service_id,status['delegationDigest'],status['observedAt'])
            require(observed['policy_revision']<=value['policyFloor'] and observed['revocation_epoch']<=value['revocationFloor']
                and (not observed['revoked'] or value['revoked']),'service_schema','Retained owner facts changed')
        return value

    def _scheduler_floor(self,scheduler,current):
        # SAME scheduler transaction; no clock tick/reset/reservation mutation.
        with scheduler.transaction():
            row=scheduler.db.execute('SELECT highwater FROM clock_state WHERE id=1').fetchone()
            require(row is not None and type(row[0]) is int and current>=row[0],
                    'service_clock_rollback','Current time predates persisted scheduler floor')
        return row[0]

    def record_owner_status(self,delegation,raw_head,executor):
        from consolecrypt_runner.execution import InteractiveExecutor
        require(type(executor) is InteractiveExecutor and executor.store is self.store,
                'service_scope','Original service executor required')
        require(delegation.owner.owner_id==self.owner.owner_id and delegation.owner.owner_epoch==self.owner.owner_epoch
            and delegation.owner.public_key==self.owner.public_key and delegation.owner.tenant==self.owner.tenant,
            'service_owner','Original enrolled owner differs')
        current=int(self.clock());body=observe_owner_status(raw_head,delegation,current)
        require(body['tenant']==self.tenant and body['service_id']==self.service_id,
                'service_scope','Original service differs')
        with self.store._mutex:
            header=self._validate_header(record(self.root/'service-version.json',8192))
            require(current>=header['clockFloor'] and body['policy_revision']>=header['policyFloor']
                and body['revocation_epoch']>=header['revocationFloor'],'service_policy_rollback','Owner status rollback')
            # Known service revocation is sticky in this enrollment. A later
            # head cannot silently re-enroll it or revive an expired capsule.
            header.update(clockFloor=current,policyFloor=body['policy_revision'],revocationFloor=body['revocation_epoch'],
                revoked=header['revoked'] or body['revoked'],ownerStatus={'delegationDigest':delegation.digest,'observedAt':current,'statement':decode(raw_head,max_bytes=8192) if isinstance(raw_head,bytes) else raw_head})
            atomic_json(self.root/'service-version.json',header,max_bytes=8192)
            jobs,_=self._records()
            cancel_ids=[job for job,(state,binding) in jobs.items() if state['state'] in ('queued','running','cancelling') and binding is not None
                and (header['revoked'] or binding['policyRevision']<header['policyFloor'] or binding['revocationEpoch']<header['revocationFloor'])]
        # Existing executor owns the tokens; never callbacks under store lock.
        for job in cancel_ids:executor.cancel(job)
        return {'recorded':True,'cancelRequested':len(cancel_ids),'dispatch':False}


    def _records(self):
        jobs={}
        children=bounded_children(self.store.root/'jobs',MAX_JOBS)
        nonces=bounded_children(self.store.root/'nonces',MAX_JOBS)
        require(len(children)<=MAX_JOBS and len(nonces)<=MAX_JOBS,'job_quota','Retained custody bound reached')
        for directory in children:
            directory=self.store.job_directory(identifier(directory.name))
            manifest=record(directory/'manifest.json');state=record(directory/'state.json')
            require(manifest['tenant_id']==self.tenant['id'] and manifest['runner_id']==self.service_id,
                    'service_scope','Foreign retained service job')
            path=directory/FILE
            binding=_binding(record(path,8192)) if path.exists() or path.is_symlink() else None
            if binding is not None:
                require(binding['tenant']==self.tenant and binding['serviceId']==self.service_id
                        and binding['jobId']==directory.name and binding['manifestDigest']==state['manifest_digest'],
                        'service_scope','Foreign retained custody')
            jobs[directory.name]=(state,binding)
        nonce_rows=[]
        for path in nonces:
            identifier(path.name);nonce_rows.append(record(path))
        return jobs,nonce_rows

    def accept(self,delegation,trust_head,slot_grant,approval,scheduler,current_refs,current_owner):
        current=int(self.clock())
        require(current>0,'service_clock','Invalid service clock')
        body=delegation.body
        require(body['tenant']==self.tenant and body['service_id']==self.service_id,'service_scope','Original service differs')
        require(delegation.owner.owner_id==self.owner.owner_id and delegation.owner.owner_epoch==self.owner.owner_epoch
                and delegation.owner.public_key==self.owner.public_key,'service_owner','Original approved owner differs')
        original=capture_original_slot(scheduler,body['schedule_id'],body['schedule_revision'])
        self._scheduler_floor(scheduler,current)
        with self.store._mutex:
            header=self._validate_header(record(self.root/'service-version.json',8192))
            require(not header['revoked'] and current>=header['clockFloor'],'service_revoked','Service revoked or clock rolled back')
            require(current_owner.owner_id==self.owner.owner_id and current_owner.owner_epoch==self.owner.owner_epoch
                and current_owner.public_key==self.owner.public_key and current_owner.tenant==self.owner.tenant,
                'service_owner','Original owner differs')
            current_owner=replace(current_owner,tenant=dict(current_owner.tenant),minimum_policy_revision=max(current_owner.minimum_policy_revision,header['policyFloor']),
                minimum_revocation_epoch=max(current_owner.minimum_revocation_epoch,header['revocationFloor']))
            if body['access_policy']['mode']=='boundedOffline':
                status=header['ownerStatus'];require(status is not None and status['delegationDigest']==delegation.digest,
                    'service_owner_status_required','Initial authenticated owner status required')
                observed=observe_owner_status(encode(status['statement']),delegation,status['observedAt'])
                require(not observed['revoked'],'service_revoked','Service revoked')
        proof=verify_job_evidence(delegation,trust_head,slot_grant,approval,original,current_refs,current_owner,current)
        expected=logical_job_id(self.tenant,self.service_id,body['project_id'],original.schedule_id,original.local_slot)
        require(proof.job_id==expected,'service_slot','Native-derived original-slot job ID required')
        grant=TrustGrant(bytes.fromhex(body['signer']['public_key']),self.tenant['id'],
            frozenset(h['id'] for h in body['hosts']),frozenset([body['project_id']]),frozenset(['jobs.execute']),
            body['expires_at'],body['quotas']['max_job_seconds'])
        verified=verify_envelope(approval,{self.service_id:grant},runner_id=self.service_id,tenant_id=self.tenant['id'],
                                 environment_digest=body['environment_digest'],now=current)
        with self.store._mutex:
            header=self._validate_header(record(self.root/'service-version.json',8192))
            require(not header['revoked'] and current>=header['clockFloor']
                and body['policy_revision']>=header['policyFloor'] and body['revocation_epoch']>=header['revocationFloor'],
                'service_policy_rollback','Owner changed before custody')
            self._scheduler_floor(scheduler,current)
            # Re-capture on admission; this never resolves the separate clock
            # reservation. Cross-store dispatch needs a fresh recheck again.
            require(capture_original_slot(scheduler,body['schedule_id'],body['schedule_revision'])==original,
                    'service_schedule_changed','Original schedule changed before custody')
            jobs,nonces=self._records()
            old=jobs.get(expected)
            if old:
                require(old[0]['manifest_digest']==proof.manifest_digest,'job_conflict','Original slot already has another job')
                require(old[1] is not None,'service_reconciliation_required','Incomplete custody requires reconciliation')
                require(old[1]['delegationDigest']==delegation.digest,'job_conflict','Original custody delegation differs')
                return {'created':False,'durable':True,'jobId':expected,'state':old[0]['state'],'dispatch':False}
            # A crash after nonce fsync but before manifest fsync is unknown,
            # never permission to replace/replay the original logical slot.
            require(not any(n['job_id']==expected for n in nonces),'service_reconciliation_required','Original slot has partial durable acceptance')
            orphan={n['job_id'] for n in nonces}-jobs.keys()
            total=len(jobs)+len(orphan)
            require(total<min(MAX_JOBS,body['quotas']['max_jobs']) and len(nonces)<MAX_JOBS,'job_quota','Retained service quota reached')
            known=[b['admittedAt'] for _,b in jobs.values() if b is not None]
            require(all(b is None or (body['policy_revision']>=b['policyRevision'] and body['revocation_epoch']>=b['revocationEpoch'])
                        for _,b in jobs.values()),'service_policy_rollback','Retained owner policy highwater cannot decrease')
            require(not known or current>=max(known),'service_clock_rollback','Clock moved behind retained admission')
            day=datetime.fromtimestamp(current,timezone.utc).date()
            daily=len(orphan)+sum(1 for _,b in jobs.values() if b is None or datetime.fromtimestamp(b['admittedAt'],timezone.utc).date()==day)
            require(daily<body['quotas']['max_per_utc_day'],'service_daily_quota','UTC daily service quota reached')
            header.update(clockFloor=current,policyFloor=max(header['policyFloor'],body['policy_revision']),revocationFloor=max(header['revocationFloor'],body['revocation_epoch']))
            atomic_json(self.root/'service-version.json',header,max_bytes=8192)
            state,created=self.store.accept(verified)
            require(created,'service_reconciliation_required','Unexpected incomplete acceptance')
            binding={'version':1,'tenant':self.tenant,'serviceId':self.service_id,'projectId':body['project_id'],
                'scheduleId':original.schedule_id,'scheduleRevision':original.schedule_revision,'definitionDigest':original.definition_digest,
                'localSlot':original.local_slot,'dueUtc':original.due_utc,'jobId':expected,'manifestDigest':proof.manifest_digest,
                'delegationDigest':delegation.digest,'admittedAt':current,
                'policyRevision':body['policy_revision'],'revocationEpoch':body['revocation_epoch']}
            atomic_json(self.store.job_directory(expected)/FILE,_binding(binding),max_bytes=8192)
            atomic_json(self.store.job_directory(expected)/'approval.json',approval)
            return {'created':True,'durable':True,'jobId':expected,'state':state['state'],'dispatch':False}

    def dispatch(self,*_):
        raise RunnerError('service_provisioning_required','Fresh authority, scoped E2EE material and execution adapter required')
