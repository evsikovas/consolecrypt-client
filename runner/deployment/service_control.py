#!/opt/cc-runtime/bin/python
"""Fixed same-UID attach/stop for existing service frames. No new RPC dialect."""
import argparse,json,os,selectors,socket,stat,struct,sys,time,uuid,hashlib,runpy,fcntl,re,errno
from pathlib import Path
MAX=1024*1024
def unique(pairs):
 result={}
 for key,value in pairs:
  if key in result:raise ValueError("private service unavailable")
  result[key]=value
 return result
BASE=Path('/run/consolecrypt-service')

def same(a,b):return a.st_dev==b.st_dev and a.st_ino!=0 and a.st_ino==b.st_ino
class Endpoint:
 def __init__(self,identity):
  self.uid=os.geteuid()
  if self.uid==0:raise ValueError('private service unavailable')
  self.path=BASE/identity;self.socket=self.path/'control.sock';self.fd=None
  # Every component is opened relative to a retained no-follow directory.
  fd=os.open('/',os.O_RDONLY|os.O_DIRECTORY|os.O_CLOEXEC)
  try:
   for part in ('run','consolecrypt-service',identity):
    nextfd=os.open(part,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=fd);os.close(fd);fd=nextfd
   self.fd=fd;fd=None;self.parent=os.fstat(self.fd);self.name=os.stat('control.sock',dir_fd=self.fd,follow_symlinks=False);self.check()
  except BaseException:
   self.close();raise
  finally:
   if fd is not None:os.close(fd)
 def check(self):
  fd=os.open('/',os.O_RDONLY|os.O_DIRECTORY|os.O_CLOEXEC)
  try:
   for part in ('run','consolecrypt-service',self.path.name):
    nextfd=os.open(part,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=fd);os.close(fd);fd=nextfd
   current=os.fstat(fd)
  finally:os.close(fd)
  named=os.stat('control.sock',dir_fd=self.fd,follow_symlinks=False)
  if (not stat.S_ISDIR(current.st_mode) or current.st_uid!=self.uid or stat.S_IMODE(current.st_mode)!=0o700
   or not same(current,self.parent) or not same(os.fstat(self.fd),self.parent)
   or not stat.S_ISSOCK(named.st_mode) or named.st_uid!=self.uid or named.st_nlink!=1
   or stat.S_IMODE(named.st_mode)!=0o600 or not same(named,self.name)):raise ValueError('private service unavailable')
 def connect(self):
  stream=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM)
  try:
   stream.settimeout(5);stream.connect(str(self.socket));pid,uid,gid=struct.unpack('3i',stream.getsockopt(socket.SOL_SOCKET,socket.SO_PEERCRED,struct.calcsize('3i')))
   if uid!=self.uid:raise ValueError('private service unavailable')
   self.check();return stream
  except BaseException:stream.close();raise
 def close(self):
  if self.fd is not None:os.close(self.fd);self.fd=None

def stopped(stream):
 request=str(uuid.uuid4());deadline=time.monotonic()+185;stream.settimeout(185)
 frame=json.dumps({'version':1,'request_id':request,'operation':'service_close','body':{}},separators=(',',':')).encode()+b'\n';stream.sendall(frame);stream.shutdown(socket.SHUT_WR)
 raw=bytearray()
 while not raw.endswith(b'\n'):
  remaining=deadline-time.monotonic()
  if remaining<=0:raise TimeoutError('private service unavailable')
  stream.settimeout(remaining);part=stream.recv(1)
  if not part or len(raw)>=MAX:raise ValueError('private service unavailable')
  raw.extend(part)
 value=json.loads(raw,object_pairs_hook=unique)
 if (type(value) is not dict or set(value)!={'version','type','request_id','result','error'} or type(value['version']) is not int
  or value['version']!=1 or value['type']!='response' or value['request_id']!=request or value['error'] is not None
  or type(value['result']) is not dict or set(value['result'])!={'cancellation_requested'} or value['result']['cancellation_requested'] is not True):raise ValueError('private service unavailable')
 remaining=deadline-time.monotonic()
 if remaining<=0:raise TimeoutError('private service unavailable')
 stream.settimeout(remaining)
 if stream.recv(1)!=b'':raise ValueError('private service unavailable')
 # This waits for original transport EOF. systemd ALSO waits original main PID;
 # neither acknowledgement claims a remote partial/unknown task was rolled back.

def attach(stream,endpoint):
 stream.setblocking(False);os.set_blocking(0,False);os.set_blocking(1,False)
 selector=selectors.DefaultSelector();pending_to=bytearray();pending_out=bytearray();incoming=bytearray();outgoing=bytearray();stdin=True;closed=False;write_closed=False;last=time.monotonic();started={}
 def framed(buffer,data):
  if not buffer:started[id(buffer)]=time.monotonic()
  buffer.extend(data)
  while b'\n' in buffer:
   line,_,tail=buffer.partition(b'\n');buffer[:]=tail
   if len(line)+1>MAX:raise ValueError('private service unavailable')
   started[id(buffer)]=time.monotonic()
   yield line+b'\n'
  if len(buffer)>MAX:raise ValueError('private service unavailable')
 try:
  while not closed or pending_out:
   if any(buffer and time.monotonic()-started[id(buffer)]>30 for buffer in (incoming,outgoing)):raise TimeoutError('private service unavailable')
   if time.monotonic()-last>300:raise TimeoutError('private service unavailable')
   # Rebuild only a fixed three-descriptor interest set; no listeners or threads.
   for key in list(selector.get_map().values()):selector.unregister(key.fd)
   if stdin and len(pending_to)<MAX:selector.register(0,selectors.EVENT_READ,'stdin')
   interests=(selectors.EVENT_READ if not closed and len(pending_out)<MAX else 0)|(selectors.EVENT_WRITE if pending_to else 0)
   if interests:selector.register(stream,interests,'socket')
   if pending_out:selector.register(1,selectors.EVENT_WRITE,'stdout')
   if not stdin and not pending_to and not write_closed:stream.shutdown(socket.SHUT_WR);write_closed=True
   for key,mask in selector.select(1):
    last=time.monotonic();endpoint.check()
    if key.data=='stdin':
     data=os.read(0,65536)
     if not data:
      if incoming:raise ValueError('private service unavailable')
      stdin=False
     else:
      for raw in framed(incoming,data):pending_to.extend(raw)
    elif key.data=='stdout':
     count=os.write(1,pending_out);del pending_out[:count]
    else:
     if mask&selectors.EVENT_WRITE:
      count=stream.send(pending_to);del pending_to[:count]
     if mask&selectors.EVENT_READ:
      data=stream.recv(65536)
      if not data:
       if outgoing:raise ValueError('private service unavailable')
       closed=True;stdin=False;pending_to.clear()
      else:
       for raw in framed(outgoing,data):pending_out.extend(raw)
    if len(pending_to)>2*MAX or len(pending_out)>2*MAX:raise ValueError('private service unavailable')
 finally:selector.close()

_RESULT_STATE=Path('/var/lib/consolecrypt-service')
def _result_software(identity):
 library=Path('/opt/consolecrypt/deployment/service_daemon.py')
 info=library.lstat()
 if not stat.S_ISREG(info.st_mode) or info.st_uid!=0 or info.st_nlink!=1 or info.st_mode&0o222:raise ValueError('private service results unavailable')
 daemon=runpy.run_path(str(library));return daemon,daemon['software'](identity)

def _completed_stop(identity):
 # systemd invokes ExecStop even after a successful original peer-close. A
 # missing listener alone proves nothing: only the actual current ExecStop
 # process in this immutable instance's kernel cgroup may acknowledge it.
 uid=os.geteuid();pid=os.getpid()
 if uid==0 or uid!=os.getuid():raise ValueError('private service unavailable')
 _,selected=_result_software(identity)
 sys.path.insert(0,str(selected/'runner'))
 from consolecrypt_runner.service import service_quiescence as guard
 unit='consolecrypt-service@'+identity+'.service';deadline=time.monotonic()+5
 properties=guard.PROPERTIES+('ControlPID','ExecMainPID','ExecMainCode','ExecMainStatus','Result')
 def observe():
  value=guard._show(unit,selected,deadline,properties=properties)
  if (value['ActiveState']!='deactivating' or value['SubState']!='stop' or value['MainPID']!='0'
      or value['ControlPID']!=str(pid) or not value['ExecMainPID'].isascii()
      or not value['ExecMainPID'].isdigit() or int(value['ExecMainPID'])<=0 or value['ExecMainPID']==str(pid)
      or value['ExecMainCode']!='1' or value['ExecMainStatus']!='0' or value['Result']!='success'
      or value['ControlGroup']!=guard.TEMPLATE_CGROUP+'/'+unit):raise ValueError('private service unavailable')
  with open('/proc/self/cgroup','rb') as source:membership=source.read(4097)
  if membership!=('0::'+value['ControlGroup']+'\n').encode():raise ValueError('private service unavailable')
  return value
 original=observe();parent=guard._cgroup_parent();group=None;processes=None
 try:
  parent_info=os.fstat(parent)
  group=os.open(unit,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=parent)
  info=os.fstat(group)
  if not stat.S_ISDIR(info.st_mode) or info.st_uid!=0 or info.st_mode&0o022:raise ValueError('private service unavailable')
  # No remaining original helper, worker, or delegated child cgroup is hidden
  # behind MainPID=0. This is service shutdown only, never a job outcome.
  with os.scandir(group) as entries:
   if any(entry.is_dir(follow_symlinks=False) for entry in entries):raise ValueError('private service unavailable')
  processes=os.open('cgroup.procs',os.O_RDONLY|os.O_NONBLOCK|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=group)
  metadata=os.fstat(processes)
  if not stat.S_ISREG(metadata.st_mode) or metadata.st_uid!=0 or metadata.st_mode&0o022:raise ValueError('private service unavailable')
  if os.read(processes,4097)!=f'{pid}\n'.encode():raise ValueError('private service unavailable')
  if observe()!=original:raise ValueError('private service unavailable')
  current=guard._cgroup_parent()
  try:
   if not same(parent_info,os.fstat(current)) or not same(info,os.stat(unit,dir_fd=current,follow_symlinks=False)):
    raise ValueError('private service unavailable')
  finally:os.close(current)
 finally:
  if processes is not None:os.close(processes)
  if group is not None:os.close(group)
  os.close(parent)

def _automatic_expected(enrollment,namespace):
 # Original held registration/bootstrap metadata, NOT a policy's self-key or
 # caller approval JSON. The SAME signature/header/runtime checks below and
 # actual inactive-only OS+locks proof remain mandatory before any write.
 owner=enrollment['bootstrap']['owner'];origin=enrollment['bootstrap']['origin']
 from consolecrypt_runner.service.service_authority_v4 import DELEGATION_DOMAIN
 from consolecrypt_runner.protocol import encode
 return {'namespace':namespace,'service_id':origin['service_id'],'tenant':owner['tenant'],
  'owner_id':owner['owner_id'],'owner_epoch':owner['owner_epoch'],
  'owner_key_sha256':hashlib.sha256(bytes.fromhex(owner['signing'])).hexdigest(),
  'signer_sha256':hashlib.sha256(bytes.fromhex(enrollment['signing'])).hexdigest(),
  'recipient_sha256':hashlib.sha256(bytes.fromhex(enrollment['recipient'])).hexdigest(),
  'policy_digest':hashlib.sha256(DELEGATION_DOMAIN+encode(enrollment['delegation']['body'])).hexdigest()}

def _expected_fields(expected):
 fields={'namespace','service_id','tenant','owner_id','owner_epoch','owner_key_sha256','signer_sha256','recipient_sha256','policy_digest'}
 if type(expected)is not dict or set(expected)!=fields:raise ValueError('private service results unavailable')
 from consolecrypt_runner.service.service_authority import integer
 for name in fields-{'namespace','service_id','tenant','owner_id','owner_epoch'}:
  if type(expected[name])is not str or re.fullmatch('[0-9a-f]{64}',expected[name])is None:raise ValueError('private service results unavailable')
 integer(expected['owner_epoch'])
 if (type(expected['namespace'])is not str or re.fullmatch('cc-service-local-v1-[0-9a-f]{64}',expected['namespace'])is None
  or str(uuid.UUID(expected['service_id']))!=expected['service_id'] or str(uuid.UUID(expected['owner_id']))!=expected['owner_id']
  or type(expected['tenant'])is not dict or set(expected['tenant'])!={'kind','id'} or expected['tenant']['kind']not in('localProfile','personalAccount')
  or str(uuid.UUID(expected['tenant']['id']))!=expected['tenant']['id']):raise ValueError('private service results unavailable')
 return expected

def results(identity,*,retire_raw=False,inactive_only=False):
 # Observation-only mode. NEVER construct JobStore/Helper/Node: their normal
 # constructors recover/retire records. No key file is opened by this reader.
 uid=os.geteuid()
 if uid==0 or uid!=os.getuid():raise ValueError('private service results unavailable')
 daemon,selected=_result_software(identity)
 sys.path.insert(0,str(selected/'runner'))
 from consolecrypt_runner.protocol import decode,encode
 from consolecrypt_runner.service.service_custody import _binding,ServiceCustody
 from consolecrypt_runner.service.service_authority_v4 import delegation_shape,DELEGATION_DOMAIN,PinnedOwner
 from consolecrypt_runner.service.service_authority import statement,integer
 from consolecrypt_runner.service.cc_service_queue.queue import MAX_JOBS,STATES
 from consolecrypt_runner import artifact_retention as history
 from consolecrypt_runner.approval import DOMAIN,MANIFEST_FIELDS
 if inactive_only:
  if not retire_raw:raise ValueError('private service retirement unavailable')
  from consolecrypt_runner.service.service_quiescence import require_inactive
  require_inactive(identity,selected) # No attach/stop/key/store construction.
  frame={'version':1,'request_id':str(uuid.uuid4()),'operation':'service_retire_raw','body':None}
  expected=None
 else:
  raw=_retirement_input() if retire_raw else sys.stdin.buffer.readline(8193)
  if not raw.endswith(b'\n') or len(raw)>8192:raise ValueError('private service results unavailable')
  frame=decode(raw,max_bytes=8192)
  if (type(frame)is not dict or set(frame)!={'version','request_id','operation','body'} or type(frame['version'])is not int
      or frame['version']!=1 or frame['operation']!=('service_retire_raw' if retire_raw else 'service_results') or str(uuid.UUID(frame['request_id']))!=frame['request_id']):raise ValueError('private service results unavailable')
  expected=_expected_fields(frame['body'])
 def need(value):
  if not value:raise ValueError('private service results unavailable')
 class Directory:
  def __init__(self,path,fd=None):
   self.path=path;self.fd=daemon['parent'](path) if fd is None else fd;self.original=os.fstat(self.fd)
   try:need(stat.S_ISDIR(self.original.st_mode) and self.original.st_uid==uid and self.original.st_mode&0o7777==0o700)
   except BaseException:os.close(self.fd);raise
  def child(self,name):
   need(type(name)is str and '/' not in name and name not in ('','.','..'))
   return Directory(self.path/name,os.open(name,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=self.fd))
  def close(self):os.close(self.fd)
  def check(self):
   fd=daemon['parent'](self.path)
   try:
    current=os.fstat(fd);held=os.fstat(self.fd)
    need(same(self.original,current) and same(self.original,held)
      and current.st_uid==held.st_uid==uid and current.st_mode&0o7777==held.st_mode&0o7777==0o700)
   finally:os.close(fd)
  def read(self,name,maximum,binary=False):
   fd=os.open(name,os.O_RDONLY|os.O_NONBLOCK|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=self.fd)
   with os.fdopen(fd,'rb')as stream:
    before=os.fstat(stream.fileno());need(stat.S_ISREG(before.st_mode) and before.st_uid==uid and before.st_nlink==1
      and before.st_mode&0o7777==0o600 and 0<=before.st_size<=maximum)
    raw=stream.read(maximum+1);after=os.fstat(stream.fileno());named=os.stat(name,dir_fd=self.fd,follow_symlinks=False)
    need(len(raw)==before.st_size and same(before,after) and same(before,named)
      and (before.st_size,before.st_mtime_ns,before.st_ctime_ns)==(after.st_size,after.st_mtime_ns,after.st_ctime_ns))
   self.check();return raw if binary else decode(raw,max_bytes=maximum)
  def names(self,maximum):
   names=[]
   with os.scandir(self.fd)as entries:
    for entry in entries:need(len(names)<maximum);names.append(entry.name)
   self.check();return names
  def public_identity(self):
   # Existing CCSI v1 PUBLIC offsets only. Never read/decode the secret69-byte
   # region32..101 or create a key/holder. This observes identity metadata for a
   # history read, not proof that damaged secret material could start a Node.
   fd=os.open('identity.v1',os.O_RDONLY|os.O_NONBLOCK|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=self.fd)
   try:
    before=os.fstat(fd);need(stat.S_ISREG(before.st_mode) and before.st_uid==uid and before.st_nlink==1
      and before.st_mode&0o7777==0o600 and before.st_size==197)
    prefix=os.pread(fd,32,0);public=os.pread(fd,64,101)
    after=os.fstat(fd);named=os.stat('identity.v1',dir_fd=self.fd,follow_symlinks=False)
    need(same(before,after) and same(before,named) and len(prefix)==32 and len(public)==64
      and prefix==b'CCSI\0\0\0\1'+uuid.UUID(identity).bytes+(1).to_bytes(8,'little')
      and (before.st_size,before.st_mtime_ns,before.st_ctime_ns)==(after.st_size,after.st_mtime_ns,after.st_ctime_ns))
    self.check();return before.st_dev,before.st_ino,before.st_mtime_ns,before.st_ctime_ns,public
   finally:os.close(fd)
 held=[]
 def keep(value):held.append(value);return value
 root=keep(Directory(_RESULT_STATE/identity))
 try:
  runtime=keep(root.child('runtime'));identities=keep(root.child('identities'));keydir=keep(identities.child('service-identity-'+identity))
  need(set(keydir.names(2))=={'identity.v1','enrollment.v1'})
  original_identity=keydir.public_identity()
  original=keydir.read('enrollment.v1',2*1024*1024+44,True)
  need(original[:8]==b'CCSE\0\0\0\1' and len(original)>=44)
  size=int.from_bytes(original[8:12],'little');need(0<size<=2*1024*1024 and len(original)==size+44)
  need(hashlib.sha256(b'ConsoleCrypt Original Service Enrollment Commit v1\0'+original[:12+size]).digest()==original[12+size:])
  enrollment=decode(original[12:12+size],max_bytes=2*1024*1024)
  need(type(enrollment)is dict and set(enrollment)=={'version','identity_id','key_epoch','signing','recipient','runtime','bootstrap','delegation','helper_ack','targets','plans','known_hosts','capsules'}
    and type(enrollment['version'])is int and enrollment['version']==1 and enrollment['identity_id']==identity
    and type(enrollment['key_epoch'])is int and enrollment['key_epoch']==1)
  signing=bytes.fromhex(enrollment['signing']);recipient=bytes.fromhex(enrollment['recipient']);need(len(signing)==len(recipient)==32)
  need(original_identity[-1]==signing+recipient)
  namespace='cc-service-local-v1-'+hashlib.sha256(b'ConsoleCrypt ServiceLocalNamespace v1\0'+uuid.UUID(identity).bytes+(1).to_bytes(8,'big')+signing+recipient).hexdigest()
  if inactive_only:expected=_expected_fields(_automatic_expected(enrollment,namespace))
  need(namespace==expected['namespace'] and hashlib.sha256(signing).hexdigest()==expected['signer_sha256']
    and hashlib.sha256(recipient).hexdigest()==expected['recipient_sha256'])
  need(enrollment['runtime']=={'device':runtime.original.st_dev,'inode':runtime.original.st_ino})
  signed=enrollment['delegation'];need(type(signed)is dict and set(signed)=={'body','signature'})
  policy=signed['body'];delegation_shape(policy)
  need(hashlib.sha256(DELEGATION_DOMAIN+encode(policy)).hexdigest()==expected['policy_digest']
    and policy['service_id']==expected['service_id'] and policy['tenant']==expected['tenant'] and policy['owner_id']==expected['owner_id'])
  ack=enrollment['helper_ack']
  need(type(ack)is dict and set(ack)=={'enrolled','delegationDigest','revisionId'} and ack['enrolled']is True
    and ack['delegationDigest']==expected['policy_digest'] and type(ack['revisionId'])is str
    and str(uuid.UUID(ack['revisionId']))==ack['revisionId'] and uuid.UUID(ack['revisionId']).int!=0)
  owner=enrollment['bootstrap']['owner']
  need(hashlib.sha256(bytes.fromhex(owner['signing'])).hexdigest()==expected['owner_key_sha256']
    and policy['owner_epoch']==owner['owner_epoch']==expected['owner_epoch'])
  header=runtime.read('service-version.json',8192)
  # SAME existing observation-only scope/strict header/status verifier. This
  # object has no Store/accept/dispatch. Public owner key is independently bound
  # to Native's original Frame/ledger hash, never policy-self-key authority.
  pin=PinnedOwner(expected['owner_id'],expected['owner_epoch'],expected['tenant'],bytes.fromhex(owner['signing']),
    owner['not_after'],policy['policy_revision'],policy['revocation_epoch'])
  scope=ServiceCustody._restore_scope(runtime.path,pin,expected['service_id']);scope._validate_header(header)
  if retire_raw:
   from consolecrypt_runner.service.service_quiescence import retire_fixed
   observed=retire_fixed(scope,identity,runtime,keydir,selected,Endpoint,stopped,expected['policy_digest'],inactive_only=inactive_only)
   for directory in held:directory.check()
   need(keydir.read('enrollment.v1',2*1024*1024+44,True)==original and keydir.public_identity()==original_identity)
   reply={'version':1,'type':'response','request_id':frame['request_id'],'result':{'version':1,'namespace':namespace,
     'policyDigest':expected['policy_digest'],**observed},'error':None}
   output=encode(reply)+b'\n';need(len(output)<=8192);sys.stdout.buffer.write(output);sys.stdout.buffer.flush();return
  state=keep(runtime.child('state'));jobs=keep(state.child('jobs'))
  # A shared lock on the EXISTING metadata lock is read-only and does not take
  # the active owner's lock. No create, recovery, clock/quota or retention write.
  lock=os.open('.lock',os.O_RDONLY|os.O_NONBLOCK|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=state.fd)
  try:
   info=os.fstat(lock);need(stat.S_ISREG(info.st_mode) and info.st_uid==uid and info.st_nlink==1 and info.st_mode&0o7777==0o600 and info.st_size==0)
   fcntl.flock(lock,fcntl.LOCK_SH|fcntl.LOCK_NB)
   result=[]
   for name in jobs.names(MAX_JOBS):
    need(str(uuid.UUID(name))==name)
    job=jobs.child(name)
    try:
     binding=_binding(job.read('service-custody.json',8192))
     need(binding['jobId']==name and binding['tenant']==expected['tenant'] and binding['serviceId']==expected['service_id'])
     if binding['delegationDigest']!=expected['policy_digest']:continue
     need(binding['scheduleId']==policy['schedule_id'] and binding['scheduleRevision']==policy['schedule_revision']
       and binding['definitionDigest']==policy['definition_digest'] and binding['projectId']==policy['project_id'])
     raw_state=job.read('state.json',1048576,True);value=decode(raw_state,max_bytes=1048576)
     history.validate_state(value)
     need(type(value)is dict and type(value.get('version'))is int and value['version']==1 and value['job_id']==name
       and value['manifest_digest']==binding['manifestDigest'] and value['state'] in STATES)
     if value['state'] in ('queued','running','cancelling'):continue
     manifest=job.read('manifest.json',262144)
     need(type(manifest)is dict and set(manifest)==MANIFEST_FIELDS and type(manifest['version'])is int and manifest['version']==1
       and manifest['job_id']==name and manifest['tenant_id']==policy['tenant']['id'] and manifest['runner_id']==policy['service_id']
       and manifest['project_id']==policy['project_id'] and hashlib.sha256(DOMAIN+encode(manifest)).hexdigest()==binding['manifestDigest'])
     approval=job.read('approval.json',262144)
     need(type(approval)is dict and set(approval)=={'signer_id','manifest','signature'} and approval['manifest']==manifest)
     # Existing job signature/domain, observation only. No TrustGrant or
     # VerifiedManifest is constructed; expiry is NOT renewed for a history read.
     statement(encode({'body':manifest,'signature':approval['signature']}),DOMAIN,bytes.fromhex(policy['signer']['public_key']))
     start=job.read('service-start.json',8192)
     need(set(start)=={'version','jobId','manifestDigest','delegationDigest','revisionId','targetOutcome'} and type(start['version'])is int
       and start['version']==1 and start['jobId']==name and start['manifestDigest']==binding['manifestDigest']
       and start['delegationDigest']==binding['delegationDigest'] and start['targetOutcome'] in ('known','pending','uncertain'))
     result_value=value['result'];need(result_value is None or type(result_value)is dict)
     result_value=result_value or {}
     def flag(key):
      v=result_value.get(key);need(v is None or type(v)is bool);return v
     cancel=flag('cancel_requested');timeout=flag('timed_out');partial=flag('remote_changes_may_be_partial')
     code=result_value.get('exit_code');need(code is None or type(code)is int and -2147483648<=code<=2147483647)
     for key in ('accepted_at','updated_at'):need(type(value[key])is str and len(value[key])<=64 and not any(ord(c)<32 for c in value[key]))
     uncertain=partial is not False or start['targetOutcome']!='known' or value['state']=='unknown'
     observed='unknown' if uncertain else value['state']
     result.append({'jobId':name,'manifestDigest':binding['manifestDigest'],'scheduleId':binding['scheduleId'],
       'localSlot':binding['localSlot'],'dueUtc':binding['dueUtc'],'state':observed,'runnerState':value['state'],
       'targetOutcome':start['targetOutcome'],'remoteChangesMayBePartial':uncertain,
       'cancelRequested':cancel,'timedOut':timeout,'exitCode':code,'acceptedUtc':value['accepted_at'],'updatedUtc':value['updated_at']})
     need(job.read('state.json',1048576,True)==raw_state)
    finally:job.close()
   result.sort(key=lambda v:(v['updatedUtc'],v['jobId']),reverse=True)
   for directory in held:directory.check()
   need(keydir.read('enrollment.v1',2*1024*1024+44,True)==original)
   need(keydir.public_identity()==original_identity)
   reply={'version':1,'type':'response','request_id':frame['request_id'],'result':{'version':1,'namespace':namespace,
     'signing':enrollment['signing'],'recipient':enrollment['recipient'],'delegation':signed,'jobs':result[:32]},'error':None}
   output=encode(reply)+b'\n';need(len(output)<=65536);sys.stdout.buffer.write(output);sys.stdout.buffer.flush()
  finally:os.close(lock)
 finally:
  for directory in reversed(held):directory.close()

def _retirement_input():
 # Fixed bounded initial metadata frame. No caller paths/PIDs/stop/join booleans.
 deadline=time.monotonic()+5;raw=bytearray()
 with selectors.DefaultSelector() as selector:
  selector.register(0,selectors.EVENT_READ)
  while not raw.endswith(b'\n'):
   remaining=deadline-time.monotonic()
   if remaining<=0 or not selector.select(remaining):raise ValueError('private service retirement unavailable')
   part=os.read(0,1)
   if not part or len(raw)>=8192:raise ValueError('private service retirement unavailable')
   raw.extend(part)
 return bytes(raw)

def main():
 parser=argparse.ArgumentParser();mode=parser.add_mutually_exclusive_group();mode.add_argument('--stop',action='store_true');mode.add_argument('--results',action='store_true');mode.add_argument('--retire-raw',action='store_true');mode.add_argument('--retire-raw-inactive',action='store_true');parser.add_argument('identity');args=parser.parse_args()
 if str(uuid.UUID(args.identity))!=args.identity or uuid.UUID(args.identity).int==0:raise ValueError('private service unavailable')
 if args.results:results(args.identity);return
 if args.retire_raw_inactive:results(args.identity,retire_raw=True,inactive_only=True);return
 if args.retire_raw:results(args.identity,retire_raw=True);return
 endpoint=None;stream=None
 try:
  try:endpoint=Endpoint(args.identity);stream=endpoint.connect()
  except OSError as error:
   if not args.stop or error.errno not in (errno.ENOENT,errno.ECONNREFUSED):raise
   _completed_stop(args.identity);return
  if args.stop:stopped(stream)
  else:attach(stream,endpoint)
 finally:
  if stream is not None:stream.close()
  if endpoint is not None:endpoint.close()
if __name__=='__main__':
 try:main()
 except Exception:
  sys.stderr.write('private service control unavailable\n');raise SystemExit(65)
