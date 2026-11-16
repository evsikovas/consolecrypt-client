"""Actual signed JobStore metadata/read-only FD tests, not execution acceptance.

The bundle loader is stubbed ONLY at this component boundary; the original
parent()/no-follow owner reads are real. Installed loader/SSH/Native/UI remain
separate actual integration gates, never inferred from these tests.
"""
import base64,copy,hashlib,importlib.util,io,json,os,runpy,tempfile,time,unittest,uuid
import multiprocessing,selectors,sys
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch
DEPLOY=Path(__file__).resolve().parent
RUNNER=DEPLOY.parent
sys.path[:0]=[str(RUNNER/'src'),str(RUNNER/'tests')]
from service_execution_fixtures import Fixture,NOW,signed,public
from consolecrypt_runner.protocol import encode,decode
from consolecrypt_runner.service.service_custody import ServiceCustody,logical_job_id
from consolecrypt_runner.service.service_authority_v4 import DELEGATION_DOMAIN
from consolecrypt_runner.approval import DOMAIN,TrustGrant,verify_envelope
from consolecrypt_runner.store import atomic_json

spec=importlib.util.spec_from_file_location('results_candidate',str(DEPLOY/'service_control.py'))
control=importlib.util.module_from_spec(spec);spec.loader.exec_module(control)
daemon=runpy.run_path(str(DEPLOY/'service_daemon.py'))
CANARY='SYNTHETIC_RESULTS_MUST_NEVER_APPEAR_9f8c'

class ResultsTests(unittest.TestCase):
 def setUp(self):
  self.temp=tempfile.TemporaryDirectory(prefix='owned-service-results-');self.base=Path(self.temp.name)
  self.f=Fixture(self.base/'scheduler-fixture');self.identity=str(uuid.uuid4())
  self.home=self.base/self.identity;self.home.mkdir(mode=0o700);self.runtime=self.home/'runtime'
  self.q=ServiceCustody(self.runtime,self.f.pin,self.f.service_id,clock=lambda:NOW)
  self.ids=self.home/'identities';self.ids.mkdir(mode=0o700)
  self.keydir=self.ids/('service-identity-'+self.identity);self.keydir.mkdir(mode=0o700)
  body=self.f.body;self.policy=decode(signed(self.f.owner,DELEGATION_DOMAIN,body))
  self.digest=hashlib.sha256(DELEGATION_DOMAIN+encode(body)).hexdigest()
  signing=bytes.fromhex(body['signer']['public_key']);recipient=bytes.fromhex(body['recipient']['public_key'])
  # Public-layout component fixture only. Never a claimed loadable key/Node.
  key=b'CCSI\0\0\0\1'+uuid.UUID(self.identity).bytes+(1).to_bytes(8,'little')+(CANARY.encode()*3)[:69]+signing+recipient
  (self.keydir/'identity.v1').write_bytes(key+b'0'*32);(self.keydir/'identity.v1').chmod(0o600)
  namespace='cc-service-local-v1-'+hashlib.sha256(b'ConsoleCrypt ServiceLocalNamespace v1\0'+uuid.UUID(self.identity).bytes+(1).to_bytes(8,'big')+signing+recipient).hexdigest()
  self.expected={'namespace':namespace,'service_id':self.f.service_id,'tenant':self.f.tenant,'owner_id':self.f.pin.owner_id,
    'owner_epoch':1,'owner_key_sha256':hashlib.sha256(public(self.f.owner)).hexdigest(),'signer_sha256':hashlib.sha256(signing).hexdigest(),
    'recipient_sha256':hashlib.sha256(recipient).hexdigest(),'policy_digest':self.digest}
  inode=self.runtime.stat()
  record={'version':1,'identity_id':self.identity,'key_epoch':1,'signing':signing.hex(),'recipient':recipient.hex(),
    'runtime':{'device':inode.st_dev,'inode':inode.st_ino},'bootstrap':{'owner':{'signing':public(self.f.owner).hex(),
    'owner_epoch':1,'not_after':self.f.pin.not_after}},'delegation':self.policy,
    'helper_ack':{'enrolled':True,'delegationDigest':self.digest,'revisionId':str(uuid.uuid4())},'targets':[], 'plans':[], 'known_hosts':[], 'capsules':[]}
  raw=encode(record);prefix=b'CCSE\0\0\0\1'+len(raw).to_bytes(4,'little')+raw
  (self.keydir/'enrollment.v1').write_bytes(prefix+hashlib.sha256(b'ConsoleCrypt Original Service Enrollment Commit v1\0'+prefix).digest());(self.keydir/'enrollment.v1').chmod(0o600)
  local='2026-11-12T12:06';job=logical_job_id(self.f.tenant,self.f.service_id,self.f.project,self.f.schedule,local)
  manifest={'version':1,'job_id':job,'tenant_id':self.f.tenant['id'],'runner_id':self.f.service_id,'project_id':self.f.project,
    'revision_digest':'a'*64,'inventory_digest':'b'*64,'environment_digest':'e'*64,'parameters_digest':'c'*64,'playbook':'playbook.yml',
    'policy':{'strict_isolation':True,'cancel_on_lock':True,'timeout_seconds':60,'forks':1,'production':False},'mode':'check',
    'targets':[self.f.target],'issued_at':NOW,'expires_at':NOW+60,'access_expires_at':NOW+60,'nonce':str(uuid.uuid4())}
  self.envelope={'signer_id':self.f.service_id,'manifest':manifest,'signature':base64.b64encode(self.f.service.sign(DOMAIN+encode(manifest))).decode()}
  grant=TrustGrant(public(self.f.service),self.f.tenant['id'],frozenset({self.f.host}),frozenset({self.f.project}),frozenset({'jobs.execute'}),NOW+60)
  verified=verify_envelope(self.envelope,{self.f.service_id:grant},runner_id=self.f.service_id,tenant_id=self.f.tenant['id'],environment_digest='e'*64,now=NOW)
  self.q.store.accept(verified);self.q.store.transition(job,'running');self.q.store.transition(job,'unknown',result={'cancel_requested':True,'timed_out':False,'exit_code':-15,'remote_changes_may_be_partial':True,'raw_output':CANARY})
  self.job=self.q.store.job_directory(job)
  binding={'version':1,'tenant':self.f.tenant,'serviceId':self.f.service_id,'projectId':self.f.project,'scheduleId':self.f.schedule,
    'scheduleRevision':1,'definitionDigest':body['definition_digest'],'localSlot':local,'dueUtc':NOW+360,'jobId':job,'manifestDigest':verified.digest,
    'delegationDigest':self.digest,'admittedAt':NOW,'policyRevision':1,'revocationEpoch':1}
  atomic_json(self.job/'service-custody.json',binding)
  atomic_json(self.job/'approval.json',self.envelope)
  atomic_json(self.job/'service-start.json',{'version':1,'jobId':job,'manifestDigest':verified.digest,'delegationDigest':self.digest,
    'revisionId':str(uuid.uuid4()),'targetOutcome':'pending'})
  self.q.close()  # Actual same JobStore closed before this one-shot reader.
 def tearDown(self):
  self.f.store.close();self.temp.cleanup()
 def snapshot(self):
  return {str(p.relative_to(self.home)):hashlib.sha256(p.read_bytes()).hexdigest() for p in self.home.rglob('*')if p.is_file()and not p.is_symlink()}
 def read(self,scope=None):
  frame={'version':1,'request_id':str(uuid.uuid4()),'operation':'service_results','body':scope or self.expected}
  output=io.BytesIO()
  with patch.object(control,'_RESULT_STATE',self.base),patch.object(control,'_result_software',return_value=(daemon,RUNNER/'src')),\
       patch.object(control.sys,'stdin',SimpleNamespace(buffer=io.BytesIO(encode(frame)+b'\n'))),patch.object(control.sys,'stdout',SimpleNamespace(buffer=output)):
   control.results(self.identity)
  return output.getvalue()
 def test_closed_real_store_original_signed_unknown_has_no_mutation_or_raw_payload(self):
  before=self.snapshot();actual=os.pread;ranges=[]
  def observed(fd,size,offset):
   ranges.append((size,offset));self.assertTrue(offset+size<=32 or offset>=101);return actual(fd,size,offset)
  with patch.object(control.os,'pread',side_effect=observed):raw=self.read()
  self.assertEqual(ranges,[(32,0),(64,101)]*2)
  after=self.snapshot();self.assertEqual(before,after)
  value=decode(raw)['result'];self.assertEqual(len(value['jobs']),1);job=value['jobs'][0]
  self.assertEqual(job['state'],'unknown');self.assertTrue(job['cancelRequested']);self.assertFalse(job['timedOut'])
  self.assertTrue(job['remoteChangesMayBePartial']);self.assertEqual(job['targetOutcome'],'pending')
  self.assertNotIn(CANARY.encode(),raw);self.assertNotIn(b'credential_refs',raw)
 def test_actual_pipe_reply_and_process_exit_do_not_wait_for_stdin_eof(self):
  incoming,writer=os.pipe();reader,outgoing=os.pipe();before=self.snapshot()
  frame={'version':1,'request_id':str(uuid.uuid4()),'operation':'service_results','body':self.expected}
  def query():
   os.close(writer);os.close(reader)
   with os.fdopen(incoming,'rb')as source,os.fdopen(outgoing,'wb')as output,\
        patch.object(control,'_RESULT_STATE',self.base),patch.object(control,'_result_software',return_value=(daemon,RUNNER/'src')),\
        patch.object(control.sys,'stdin',SimpleNamespace(buffer=source)),patch.object(control.sys,'stdout',SimpleNamespace(buffer=output)):
    control.results(self.identity)
  child=multiprocessing.get_context('fork').Process(target=query);child.start();os.close(incoming);os.close(outgoing)
  try:
   os.write(writer,encode(frame)+b'\n')
   output=bytearray();deadline=time.monotonic()+3
   with selectors.DefaultSelector()as selector:
    selector.register(reader,selectors.EVENT_READ)
    while not output.endswith(b'\n'):
     self.assertLess(time.monotonic(),deadline)
     if selector.select(.05):
      block=os.read(reader,65536);self.assertTrue(block);output.extend(block);self.assertLessEqual(len(output),65536)
   self.assertEqual(decode(bytes(output))['request_id'],frame['request_id'])
   child.join(timeout=2);self.assertEqual(child.exitcode,0) # writer STILL OPEN.
   self.assertEqual(before,self.snapshot())
  finally:
   os.close(writer);os.close(reader)
   if child.is_alive():child.kill();child.join(timeout=2)
 def test_cross_service_and_policy_filters_deny_without_writes(self):
  for delta in ({'service_id':str(uuid.uuid4())},{'policy_digest':'0'*64},{'namespace':'cc-service-local-v1-'+'0'*64}):
   before=self.snapshot()
   with self.assertRaises(Exception):self.read(dict(self.expected,**delta))
   self.assertEqual(before,self.snapshot())
 def test_corrupt_completion_future_state_and_signature_deny_preserve(self):
  for path,change in [(self.keydir/'enrollment.v1',lambda raw:raw[:-1]+bytes([raw[-1]^1])),
      (self.job/'state.json',lambda raw:encode(dict(decode(raw),version=2))),
      (self.job/'approval.json',lambda raw:encode(dict(decode(raw),signature=base64.b64encode(b'0'*64).decode())))]:
   old=path.read_bytes();path.write_bytes(change(old));before=self.snapshot()
   with self.assertRaises(Exception):self.read()
   self.assertEqual(before,self.snapshot());path.write_bytes(old)
 def test_fifo_state_denied_without_blocking_or_writing(self):
  path=self.job/'state.json';old=path.read_bytes();path.unlink();os.mkfifo(path,0o600);started=time.monotonic()
  try:
   with self.assertRaises(Exception):self.read()
   self.assertLess(time.monotonic()-started,1);self.assertTrue(path.is_fifo())
  finally:path.unlink();path.write_bytes(old);path.chmod(0o600)
 def test_future_or_corrupt_header_preserved(self):
  path=self.runtime/'service-version.json';old=path.read_bytes();value=decode(old);value['version']=5;path.write_bytes(encode(value));before=self.snapshot()
  with self.assertRaises(Exception):self.read()
  self.assertEqual(before,self.snapshot())
 def test_metadata_quota_counts_junk_max_plus_one(self):
  jobs=self.runtime/'state/jobs'
  for i in range(128):(jobs/('junk-'+str(i))).mkdir(mode=0o700)
  before=self.snapshot()
  with self.assertRaises(Exception):self.read()
  self.assertEqual(before,self.snapshot())

if __name__=='__main__':unittest.main()
