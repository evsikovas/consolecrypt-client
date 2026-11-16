"""Standalone signed service fixtures; no TestCase discovery or dispatch authority."""
import base64,hashlib,tempfile,uuid
from pathlib import Path
from datetime import datetime,timezone
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.asymmetric.x25519 import X25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding,PublicFormat
from consolecrypt_runner.service.scheduler import Store,Definition,Clock,Status
from consolecrypt_runner.service.service_authority_v4 import PinnedOwner,DELEGATION_DOMAIN,HEAD_DOMAIN,verify_delegation
from consolecrypt_runner.service.service_custody import ServiceCustody
from consolecrypt_runner.service.service_runtime import ServiceExecutionRuntime
from consolecrypt_runner.__main__ import Helper
from consolecrypt_runner.inventory import StaticInventory
from consolecrypt_runner.parameters import parameters_digest
from consolecrypt_runner.probe import probe
from consolecrypt_runner.protocol import decode,encode
from consolecrypt_runner.target_channel import TargetChannel
from consolecrypt_runner.validation_reports import write_report

NOW=int(datetime(2026,11,12,12,0,tzinfo=timezone.utc).timestamp())
def uid():return str(uuid.uuid4())
def public(key):return key.public_key().public_bytes(Encoding.Raw,PublicFormat.Raw)
def signed(key,domain,body):return encode({'body':body,'signature':base64.b64encode(key.sign(domain+encode(body))).decode()})

class Fixture:
    def __init__(self,path):
        self.owner=Ed25519PrivateKey.generate();self.service=Ed25519PrivateKey.generate()
        self.tenant={'kind':'localProfile','id':uid()};self.service_id=uid();self.project=uid();self.host=uid();self.credential=uid();self.schedule=uid()
        self.pin=PinnedOwner(uid(),1,self.tenant,public(self.owner),NOW+2*86400)
        self.future=NOW+360;stamp=datetime.fromtimestamp(self.future,timezone.utc)
        definition=Definition('Etc/UTC',stamp.hour,stamp.minute)
        self.store=Store(path);self.store.save(self.schedule,0,definition,Clock(NOW,1000000000,uid()))
        self.target={'host_id':self.host,'alias':'owned','address':'192.0.2.10','port':22,'username':'owned','route_digest':'d'*64,'credential_refs':[self.credential]}
        self.body={'version':4,'id':uid(),'owner_id':self.pin.owner_id,'owner_epoch':1,'tenant':self.tenant,
            'service_id':self.service_id,'signer':{'kind':'ed25519','public_key':public(self.service).hex(),'key_epoch':1,'certificate_epoch':1},
            'recipient':{'kind':'x25519','public_key':public(X25519PrivateKey.generate()).hex(),'key_epoch':1,'certificate_epoch':1},
            'project_id':self.project,'project_revision':4,'template_digest':'a'*64,'playbook':'playbook.yml',
            'schedule_id':self.schedule,'schedule_revision':1,'definition_digest':hashlib.sha256(definition.encoded().encode()).hexdigest(),
            'inventory_digest':'b'*64,'parameters_digest':'c'*64,'environment_digest':'e'*64,
            'hosts':[{'id':self.host,'revision':7,'target_digest':hashlib.sha256(encode(self.target)).hexdigest()}],
            'credentials':[{'id':self.credential,'revision':3}],
            'route_inputs':{'dependencies':sorted([{'id':self.host,'revision':7,'kind':'host'},{'id':self.credential,'revision':3,'kind':'credential'}],key=lambda value:value['id']),
                'key_trust':[{'id':uid(),'revision':6,'kind':'known_host'}]},'modes':['check'],
            'quotas':{'max_jobs':16,'max_per_utc_day':4,'max_concurrent':1,'max_job_seconds':120,'max_forks':1},
            'policy_revision':1,'revocation_epoch':1,'issued_at':NOW-30,'expires_at':NOW+3600,
            'desktop_lock_policy':'independentDelegatedService','access_policy':{'mode':'boundedOffline','offline_not_after':NOW+3600}}
        self.head={'version':1,'owner_id':self.pin.owner_id,'owner_epoch':1,'tenant':self.tenant,'service_id':self.service_id,
            'delegation_digest':'0'*64,'policy_revision':1,'revocation_epoch':1,'signer_epoch':1,
            'recipient_epoch':1,'certificate_epoch':1,'recipient_certificate_epoch':1,'revoked':False,'issued_at':NOW-5,'expires_at':NOW+180}
    def reserve(self):
        previous=self.store.db.execute('SELECT highwater,epoch,monotonic_ns FROM clock_state WHERE id=1').fetchone()
        tick=self.store.tick(self.schedule,1,Clock(self.future,previous[2]+(self.future-previous[0])*1000000000,previous[1]))
        assert tick.status is Status.RESERVED

class CapturedWorker:
    def __init__(self):self.calls=[]
    def submit(self,*args):self.calls.append(args)
    def shutdown(self,**kwargs):pass

class ServiceExecutionFixture:
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory(prefix='owned-service-exec-');self.root=Path(self.temp.name)
        self.f=Fixture(self.root/'schedule');self.clock=NOW
        self.q=ServiceCustody(self.root/'custody',self.f.pin,self.f.service_id,clock=lambda:self.clock)
        self.f.store.close();(self.root/'schedule').rename(self.q.root/'scheduler')
        self.f.store=Store.open_existing(self.q.root/'scheduler')
        self.helper=Helper(self.q.root,_service_store=self.q.store)
        source=self.root/'source';source.mkdir();(source/'playbook.yml').write_text('---\n- name: Scoped service\n  hosts: all\n  gather_facts: false\n  tasks: []\n')
        captured=self.helper.revisions.capture(source);self.revision=captured['revision_id']
        self.target=self.f.target
        self.inventory={'version':1,'hosts':[{'alias':self.target['alias'],'host_id':self.f.host,
            'address':self.target['address'],'port':22,'username':self.target['username'],
            'variables':{},'sources':['fixture'],'credential_refs':self.target['credential_refs']}],
            'groups':{},'source_digests':{'fixture':'b'*64}}
        self.parameters={'extra_vars':{},'tags':[],'skip_tags':[],'diff':False}
        environment=probe();body=self.f.body
        body.update(playbook='playbook.yml',template_digest=captured['digest'],environment_digest=environment['environment_digest'],
            inventory_digest=StaticInventory.from_snapshot(self.inventory).digest,parameters_digest=parameters_digest(self.parameters))
        origin={'version':1,'service_id':self.f.service_id,'runner_revision':1,'runner_configuration_digest':'1'*64,
            'signer_key_sha256':hashlib.sha256(bytes.fromhex(body['signer']['public_key'])).hexdigest(),
            'recipient_key_sha256':hashlib.sha256(bytes.fromhex(body['recipient']['public_key'])).hexdigest(),
            'environment_digest':environment['environment_digest'],'helper_digest':environment['helper_digest'],
            'known_hosts_digest':'2'*64,'backend':'linuxNativeSsh','network':'serviceLocal'}
        body['execution_origin']=origin
        self.raw=signed(self.f.owner,DELEGATION_DOMAIN,body);self.signed=decode(self.raw)
        self.f.delegation=verify_delegation(self.raw,self.f.pin,NOW);self.f.head['delegation_digest']=self.f.delegation.digest
        self.runtime=ServiceExecutionRuntime(self.q,self.helper,TargetChannel(lambda _:None),origin)
        self.runtime.executor.worker.shutdown(wait=True);self.worker=CapturedWorker();self.runtime.executor.worker=self.worker
        inputs={'revision_digest':body['template_digest'],'environment_digest':body['environment_digest'],
            'playbook':body['playbook'],'inventory_digest':body['inventory_digest'],'parameters_digest':body['parameters_digest'],
            'targets_digest':hashlib.sha256(encode([self.target])).hexdigest(),'valid':True}
        self.report=self.helper.root/'revisions'/self.revision
        write_report(self.report,inputs)
        self.q.record_owner_status(self.f.delegation,signed(self.f.owner,HEAD_DOMAIN,self.f.head),self.runtime.executor)
        self.runtime.enroll(self.signed,self.revision,self.inventory,self.parameters,[self.target])
        self.f.reserve();self.clock=self.f.future

    def tearDown(self):
        self.runtime.close();self.helper.close();self.q.close();self.f.store.close();self.temp.cleanup()

