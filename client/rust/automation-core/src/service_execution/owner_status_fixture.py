"""TEST ONLY actual signature/ServiceCustody peer; never a job/grant producer."""
import base64,hashlib,json,os,sys,time,uuid
from pathlib import Path
sys.path.insert(0,sys.argv[2])
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.asymmetric.x25519 import X25519PrivateKey
from consolecrypt_runner.protocol import encode,decode,read_request,write_frame
from consolecrypt_runner.service.service_authority_v4 import PinnedOwner,verify_delegation,DELEGATION_DOMAIN,HEAD_DOMAIN
from consolecrypt_runner.service.service_custody import ServiceCustody
from consolecrypt_runner.execution import InteractiveExecutor
from consolecrypt_runner.errors import RunnerError
assert os.getuid()!=0;os.umask(0o077)
root=Path(sys.argv[1]);assert root.is_absolute() and root.is_dir() and not root.is_symlink()
uid=lambda:str(uuid.uuid4())
owner=Ed25519PrivateKey.generate();service=Ed25519PrivateKey.generate();recipient=X25519PrivateKey.generate()
public=lambda k:k.public_key().public_bytes_raw()
signed=lambda domain,body:{'body':body,'signature':base64.b64encode(owner.sign(domain+encode(body))).decode()}
now=int(time.time());host=uid();owner_id=uid();tenant={'kind':'localProfile','id':uid()}
policy={'version':4,'id':uid(),'owner_id':owner_id,'owner_epoch':1,'tenant':tenant,'service_id':uid(),
 'signer':{'kind':'ed25519','public_key':public(service).hex(),'key_epoch':1,'certificate_epoch':1},
 'recipient':{'kind':'x25519','public_key':public(recipient).hex(),'key_epoch':1,'certificate_epoch':1},
 'project_id':uid(),'project_revision':1,'template_digest':'a'*64,'playbook':'playbook.yml','schedule_id':uid(),
 'schedule_revision':1,'definition_digest':'b'*64,'inventory_digest':'c'*64,'parameters_digest':'d'*64,'environment_digest':'e'*64,
 'hosts':[{'id':host,'revision':1,'target_digest':'f'*64}],'credentials':[],
 'route_inputs':{'dependencies':[{'id':host,'revision':1,'kind':'host'}],'key_trust':[{'id':uid(),'revision':1,'kind':'known_host'}]},
 'modes':['apply'],'quotas':{'max_jobs':2,'max_per_utc_day':2,'max_concurrent':1,'max_job_seconds':30,'max_forks':1},
 'policy_revision':1,'revocation_epoch':1,'issued_at':now-1,'expires_at':now+180,
 'desktop_lock_policy':'independentDelegatedService','access_policy':{'mode':'boundedOffline','offline_not_after':now+180}}
packet=signed(DELEGATION_DOMAIN,policy);digest=hashlib.sha256(DELEGATION_DOMAIN+encode(policy)).hexdigest()
status={'version':1,'owner_id':owner_id,'owner_epoch':1,'tenant':tenant,'service_id':policy['service_id'],
 'delegation_digest':digest,'policy_revision':1,'revocation_epoch':1,'signer_epoch':1,'recipient_epoch':1,
 'certificate_epoch':1,'recipient_certificate_epoch':1,'revoked':False,'issued_at':now-1,'expires_at':now+120}
initial=signed(HEAD_DOMAIN,status);revoked=signed(HEAD_DOMAIN,dict(status,revoked=True,revocation_epoch=2))
forged=decode(encode(initial));forged['body']['revoked']=True
pin=PinnedOwner(owner_id,1,tenant,public(owner),now+3600)
verified=verify_delegation(encode(packet),pin,now)
custody=ServiceCustody(root/'custody',pin,policy['service_id']);executor=InteractiveExecutor(None,None,custody.store)
setup={'ownerSigning':public(owner).hex(),'policy':packet,'initial':initial,'revoked':revoked,'forged':forged}
sys.stdout.buffer.write(encode(setup)+b'\n');sys.stdout.buffer.flush();count=0
try:
 while request:=read_request(sys.stdin.buffer):
  try:
   body=request['body']
   if request['operation']=='service_owner_status':
    assert set(body)=={'delegation','statement'}
    original=verify_delegation(encode(body['delegation']),pin,int(time.time()))
    assert original.digest==verified.digest
    result=custody.record_owner_status(original,encode(body['statement']),executor);count+=1
    if body['statement']['body']['revoked']:
     (root/'status-entered').write_text('actual durable original-owner revocation')
     end=time.monotonic()+10
     while not (root/'release-status').is_file():
      assert time.monotonic()<end;time.sleep(.005)
   elif request['operation']=='fixture_status':
    assert body=={}
    header=decode((root/'custody/service-version.json').read_bytes())
    result={'recorded':count,'revoked':header['revoked'],'floor':header['revocationFloor']}
   else:raise RunnerError('unsupported_operation','Owned test operation unavailable')
   write_frame(sys.stdout.buffer,{'version':1,'type':'response','request_id':request['request_id'],'result':result,'error':None})
  except RunnerError as error:
   write_frame(sys.stdout.buffer,{'version':1,'type':'response','request_id':request['request_id'],'result':None,'error':{'code':error.code,'message':'Owned test request denied'}})
finally:executor.close();custody.close()
