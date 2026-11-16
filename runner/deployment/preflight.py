#!/usr/bin/env python3
"""Read-only host/private-runtime observation before software/state changes."""
import base64,hashlib,io,json,os,platform,re,stat,subprocess,sys,zipfile
from pathlib import Path

# Closed stage identifiers only: never print exception text, file names, input,
# dependency metadata or process output when the read-only observation fails.
_phase=0

def entrypoint():
 try:main();return 0
 except Exception:
  code=_phase if type(_phase)is int and 0<=_phase<=20 else 0
  sys.stderr.write('service_host_or_runtime_unsupported:%02d\n'%code)
  return 78
def require(ok):
 if not ok:raise ValueError('service_host_or_runtime_unsupported')
def version(raw):
 require(type(raw)is str and re.fullmatch(r'[0-9]{1,3}\.[0-9]{1,3}(?:\.[0-9]{1,3})?',raw))
 values=tuple(map(int,raw.split('.')));return values+(0,)*(3-len(values))
def host(expected,observed):
 target=expected['target'];runtime=expected['runtime']
 require(observed['system']==target['system']=='Linux' and observed['arch']==target['arch'] in ('x86_64','aarch64'))
 require(observed['libc']==target['libc']=='glibc' and version(observed['libcVersion'])>=version(target['minGlibc']))
 require(observed['distribution']==target['distribution']=='ubuntu' and observed['distributionVersion']==target['distributionVersion']=='24.04')
 require(type(observed['systemd'])is int and observed['systemd']>=target['minSystemd']==255)
 require(observed['prefix']==runtime['prefix']=='/opt/cc-runtime' and observed['python']==runtime['python'])
def checked_sources(raw,expected_hash,expected_manifest):
 # Read-only integrity projection, NOT a host/owner/runtime authorization.
 require(type(raw)is bytes and len(raw)<=7*1024*1024 and re.fullmatch('[0-9a-f]{64}',expected_hash)is not None)
 require(hashlib.sha256(raw).hexdigest()==expected_hash and re.fullmatch('[0-9a-f]{64}',expected_manifest)is not None)
 def unique(pairs):
  result={}
  for key,value in pairs:require(key not in result);result[key]=value
  return result
 value=json.loads(raw,object_pairs_hook=unique)
 require(type(value)is dict and set(value)=={'version','manifestSha256','files','archiveSha256','archiveBase64'}
     and type(value['version'])is int and value['version']==1 and value['manifestSha256']==expected_manifest)
 records=value['files'];require(type(records)is dict and 1<=len(records)<=128)
 require('consolecrypt_runner/__init__.py'in records and 'consolecrypt_runner/probe.py'in records)
 require(type(value['archiveBase64'])is str and len(value['archiveBase64'])<=7*1024*1024)
 archive=base64.b64decode(value['archiveBase64'],validate=True)
 require(len(archive)<=5*1024*1024 and hashlib.sha256(archive).hexdigest()==value['archiveSha256'])
 seen=set();total=0;files={}
 with zipfile.ZipFile(io.BytesIO(archive))as source:
  items=source.infolist();require(len(items)==len(records))
  for item in items:
   name=item.filename;require(type(name)is str and name.isascii()and len(name)<=240 and '\\'not in name)
   parts=name.split('/')
   require(parts[0]=='consolecrypt_runner'and len(parts)>=2 and all(re.fullmatch('[A-Za-z0-9_.+-]+',p)and p not in('.','..')for p in parts)
       and Path(name).suffix in('.py','.json')and name in records and name not in seen)
   record=records[name];require(type(record)is dict and set(record)=={'sha256','bytes'}
       and type(record['bytes'])is int and 0<=record['bytes']<=256*1024 and type(record['sha256'])is str
       and re.fullmatch('[0-9a-f]{64}',record['sha256'])is not None)
   require(not item.is_dir()and stat.S_ISREG(item.external_attr>>16)and (item.external_attr>>16)&0o777==0o444
       and item.compress_type==zipfile.ZIP_STORED and not item.flag_bits&1
       and item.file_size==item.compress_size==record['bytes'])
   total+=item.file_size;require(total<=4*1024*1024)
   body=source.read(item);require(len(body)==record['bytes']and hashlib.sha256(body).hexdigest()==record['sha256'])
   seen.add(name);files[name]=body
 require(seen==records.keys())
 return files
def materialize_sources(root,files):
 # Only called after main's original /tmp/uid/0700/noNewPrivs checks. A fresh
 # absent subtree is never merged/adopted and is covered by existing cleanup.
 destination=root/'runner';destination.mkdir(mode=0o700)
 for name,body in sorted(files.items()):
  parent=destination
  for part in name.split('/')[:-1]:
   parent=parent/part
   if not parent.exists():parent.mkdir(mode=0o700)
   info=parent.lstat();require(stat.S_ISDIR(info.st_mode)and info.st_uid==os.geteuid()and stat.S_IMODE(info.st_mode)==0o700)
  path=destination/name
  fd=os.open(path,os.O_WRONLY|os.O_CREAT|os.O_EXCL|getattr(os,'O_NOFOLLOW',0)|getattr(os,'O_CLOEXEC',0),0o444)
  try:
   view=memoryview(body)
   while view:
    count=os.write(fd,view);require(count>0);view=view[count:]
   os.fchmod(fd,0o444)
  finally:os.close(fd)
def main():
 global _phase
 _phase=1
 require(os.geteuid()!=0 and os.getuid()==os.geteuid());os.umask(0o077)
 _phase=2
 raw=sys.stdin.buffer.read(262145);require(len(raw)<=262144)
 def unique(pairs):
  result={}
  for key,value in pairs:require(key not in result);result[key]=value
  return result
 expected=json.loads(raw,object_pairs_hook=unique);require(type(expected)is dict and set(expected)=={'target','runtime','sourceSha256','sourceManifestSha256'})
 _phase=3
 flags=dict(line.split(':',1) for line in Path('/proc/self/status').read_text().splitlines() if ':' in line)
 require(int(flags['CapEff'].strip(),16)==0 and int(flags['CapAmb'].strip(),16)==0 and flags['NoNewPrivs'].strip()=='1')
 _phase=4
 root=Path(__file__).resolve().parent;info=root.lstat()
 require(root.parent==Path('/tmp') and root.name.startswith('cc-service-preflight-') and stat.S_ISDIR(info.st_mode) and info.st_uid==os.geteuid() and stat.S_IMODE(info.st_mode)==0o700)
 _phase=5
 result=subprocess.run(['/usr/bin/systemctl','--version'],capture_output=True,text=True,timeout=5,check=True)
 require(len(result.stdout)+len(result.stderr)<=4096);matched=re.match(r'systemd ([0-9]{1,3})\b',result.stdout);require(matched)
 release=platform.freedesktop_os_release();libc,libc_version=platform.libc_ver()
 observed={'system':platform.system(),'arch':platform.machine(),'libc':libc,'libcVersion':libc_version,
  'distribution':release.get('ID'),'distributionVersion':release.get('VERSION_ID'),'systemd':int(matched[1]),
  'prefix':sys.prefix,'python':platform.python_version()}
 _phase=6;host(expected,observed)
 _phase=7
 prefix=Path('/opt/cc-runtime');state=prefix.lstat()
 require(stat.S_ISDIR(state.st_mode) and not prefix.is_symlink() and state.st_uid in (0,os.geteuid()) and state.st_mode&0o022==0 and Path(sys.prefix).resolve()==prefix)
 _phase=8
 payload=root/'preflight-sources.json';info=payload.lstat()
 require(stat.S_ISREG(info.st_mode)and info.st_uid==os.geteuid()and info.st_nlink==1 and stat.S_IMODE(info.st_mode)==0o444 and info.st_size<=7*1024*1024)
 fd=os.open(payload,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK|os.O_CLOEXEC)
 try:
  held=os.fstat(fd);identity=lambda st:(st.st_dev,st.st_ino,st.st_mode,st.st_uid,st.st_nlink,st.st_size,st.st_mtime_ns,st.st_ctime_ns)
  require(identity(info)==identity(held))
  with os.fdopen(os.dup(fd),'rb')as source:payload_raw=source.read(7*1024*1024+1)
  require(len(payload_raw)==info.st_size and identity(os.fstat(fd))==identity(info)==identity(payload.lstat()))
 finally:os.close(fd)
 _phase=9
 materialize_sources(root,checked_sources(payload_raw,expected['sourceSha256'],expected['sourceManifestSha256']))
 # SAME shipped helper probe/dependency fingerprint, isolated source copy;
 # never an ambient package, second verifier or a service authorization grant.
 _phase=10
 sys.path.insert(0,str(root/'runner'));from consolecrypt_runner.probe import probe
 actual=probe();runtime=expected['runtime']
 for stage,field,key in [(11,'helper_digest','helperDigest'),(12,'dependency_files_digest','dependencyFilesDigest'),(13,'dependency_lock','dependencyLock'),(14,'environment_digest','environmentDigest')]:
  _phase=stage;require(actual[field]==runtime[key])
 _phase=15;require(actual['dependencies_ready'])
 _phase=16;require(actual['non_root'])
 for stage,key in [(17,'filesystem'),(18,'pid'),(19,'network')]:
  _phase=stage;require(actual['sandbox'][key] is True)
 _phase=20
 print(json.dumps({'version':1,**observed,'environmentDigest':actual['environment_digest'],'helperDigest':actual['helper_digest']}))
# Fixed readonly maintenance projection. No path/UID/command authority in input.
_MAINT_PARENTS=('/opt/consolecrypt','/opt/consolecrypt/bin','/opt/consolecrypt/deployment',
 '/opt/consolecrypt/service-bundles','/etc/consolecrypt-service')
_MAINT_BOOTSTRAP=(('service_daemon.py','/opt/consolecrypt/deployment/service_daemon.py'),
 ('service_control.py','/opt/consolecrypt/deployment/service_control.py'),
 ('bundle.py','/opt/consolecrypt/deployment/bundle.py'),('pointer.py','/opt/consolecrypt/deployment/pointer.py'),
 ('consolecrypt-service@.service','/etc/systemd/system/consolecrypt-service@.service'),
 ('consolecrypt-service-retention@.service','/etc/systemd/system/consolecrypt-service-retention@.service'),
 ('consolecrypt-service-retention@.timer','/etc/systemd/system/consolecrypt-service-retention@.timer'))

def _maintenance_request(raw):
 require(type(raw)is bytes and len(raw)<=8192)
 def unique(pairs):
  out={}
  for key,value in pairs:require(key not in out);out[key]=value
  return out
 def invalid(_):raise ValueError('service_maintenance_metadata_unsupported')
 value=json.loads(raw,object_pairs_hook=unique,parse_float=invalid,parse_constant=invalid)
 require(type(value)is dict and set(value)=={'version','phase','operation','identity','bootstrap'})
 require(type(value['version'])is int and value['version']==1 and value['phase']in('accountState','software')
  and value['operation']in('install','update','start','remove'))
 import uuid
 identity=value['identity'];require(type(identity)is str)
 parsed=uuid.UUID(identity);require(parsed.int!=0 and str(parsed)==identity)
 records=value['bootstrap'];require(type(records)is dict)
 if value['phase']=='accountState':require(not records)
 else:
  require(value['operation']in('install','update','start') and set(records)=={n for n,_ in _MAINT_BOOTSTRAP})
  total=0
  for r in records.values():
   require(type(r)is dict and set(r)=={'sha256','bytes'} and type(r['bytes'])is int
    and 0<=r['bytes']<=64*1024*1024 and type(r['sha256'])is str and re.fullmatch('[0-9a-f]{64}',r['sha256']))
   total+=r['bytes'];require(total<=96*1024*1024) # SAME existing bundle bounds.
 return value

def _maintenance_account_values(pw,gr,groups):
 require(type(groups)is list and 1<=len(groups)<=64 and all(type(g)is int for g in groups))
 groups=sorted(set(groups)) # id -G reports unique memberships too.
 require(pw.pw_name==gr.gr_name=='consolecrypt-service' and type(pw.pw_uid)is int and pw.pw_uid>0
  and type(pw.pw_gid)is int and pw.pw_gid>0 and gr.gr_gid==pw.pw_gid
  and type(groups)is list and groups==[pw.pw_gid])
 return {'uid':pw.pw_uid,'gid':pw.pw_gid,'namedGroupGid':gr.gr_gid,'groups':groups}

def _maintenance_account():
 # Existing NSS account only; no user/group creation, repair or adoption.
 import pwd,grp
 pw=pwd.getpwnam('consolecrypt-service');gr=grp.getgrnam('consolecrypt-service')
 groups=os.getgrouplist('consolecrypt-service',pw.pw_gid)
 result=_maintenance_account_values(pw,gr,groups)
 require(pwd.getpwnam('consolecrypt-service')==pw and grp.getgrnam('consolecrypt-service')==gr)
 return result

def _maintenance_identity(s):
 return(s.st_dev,s.st_ino,s.st_mode,s.st_uid,s.st_gid,s.st_nlink,s.st_size,s.st_mtime_ns,s.st_ctime_ns)

def _maintenance_directory(s):
 return(s.st_dev,s.st_ino,s.st_mode,s.st_uid,s.st_gid,s.st_nlink)

class _MaintenanceTree:
 # Private held root FD. Production opens only '/'; no root/path selector in JSON.
 def __init__(self,fd,account_uid):
  self.fd=os.dup(fd);self.uid=account_uid
  try:
   state=os.fstat(self.fd)
   require(stat.S_ISDIR(state.st_mode) and state.st_uid==0 and state.st_mode&0o022==0)
  except BaseException:os.close(self.fd);self.fd=None;raise
 def close(self):
  if self.fd is not None:os.close(self.fd);self.fd=None
 def scan(self,path,expected=None):
  require(path.startswith('/') and all(p not in('','.','..') for p in path.split('/')[1:]))
  parts=path.split('/')[1:];current=self.fd;held=[]
  flags=os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_NONBLOCK|os.O_CLOEXEC
  try:
   for at,part in enumerate(parts[:-1]):
    try:before=os.stat(part,dir_fd=current,follow_symlinks=False)
    except FileNotFoundError:return {'exists':False}
    private=parts[:at+1]==['var','lib','consolecrypt-service']
    require(stat.S_ISDIR(before.st_mode) and before.st_uid==(self.uid if private else 0)
     and before.st_mode&0o022==0 and (not private or stat.S_IMODE(before.st_mode)==0o700))
    child=os.open(part,flags,dir_fd=current)
    try:require(_maintenance_directory(before)==_maintenance_directory(os.fstat(child)))
    except BaseException:os.close(child);raise
    held.append((current,part,child,before));current=child
   try:before=os.stat(parts[-1],dir_fd=current,follow_symlinks=False)
   except FileNotFoundError:return {'exists':False}
   require(not stat.S_ISLNK(before.st_mode))
   result={'exists':True,'path':path,'uid':before.st_uid,'gid':before.st_gid,'nlink':before.st_nlink,
    'mode':format(stat.S_IMODE(before.st_mode),'04o'),'size':before.st_size,'inode':before.st_ino,'dev':before.st_dev,
    'isdir':stat.S_ISDIR(before.st_mode),'isreg':stat.S_ISREG(before.st_mode),'islnk':False}
   if expected is not None:
    require(result['isreg'] and before.st_uid==0 and before.st_nlink==1 and result['mode']=='0444'
     and before.st_size==expected['bytes'] and before.st_size<=64*1024*1024)
    fd=os.open(parts[-1],os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK|os.O_CLOEXEC,dir_fd=current)
    try:
     require(_maintenance_identity(before)==_maintenance_identity(os.fstat(fd)))
     digest=hashlib.sha256();count=0
     while block:=os.read(fd,65536):
      count+=len(block);require(count<=expected['bytes']);digest.update(block)
     require(count==expected['bytes'] and digest.hexdigest()==expected['sha256']
      and _maintenance_identity(before)==_maintenance_identity(os.fstat(fd))
      and _maintenance_identity(before)==_maintenance_identity(os.stat(parts[-1],dir_fd=current,follow_symlinks=False)))
     result['checksum']=digest.hexdigest()
    finally:os.close(fd)
   elif result['isdir']:
    fd=os.open(parts[-1],flags,dir_fd=current)
    try:require(_maintenance_directory(before)==_maintenance_directory(os.fstat(fd)))
    finally:os.close(fd)
   identity=_maintenance_directory if result['isdir'] else _maintenance_identity
   require(identity(before)==identity(os.stat(parts[-1],dir_fd=current,follow_symlinks=False)))
   return result
  finally:
   valid=True
   for parent,part,fd,original in reversed(held):
    try:
     try:
      valid=(_maintenance_directory(original)==_maintenance_directory(os.fstat(fd))
       ==_maintenance_directory(os.stat(part,dir_fd=parent,follow_symlinks=False))) and valid
     except OSError:valid=False
    finally:os.close(fd)
   require(valid)

def _maintenance_observe(request,tree,account):
 phase=request['phase'];operation=request['operation'];identity=request['identity']
 result={'version':1,'phase':phase,'operation':operation,'identity':identity,'account':account}
 if phase=='accountState':
  home=tree.scan('/var/lib/consolecrypt-service');state=tree.scan('/var/lib/consolecrypt-service/'+identity)
  require(home.get('isdir') and home['uid']==account['uid'] and home['mode']=='0700')
  if operation=='install':require(not state['exists'])
  else:require(state.get('isdir') and state['uid']==account['uid'] and state['mode']=='0700')
  result['privateDirectories']={'results':[{'stat':home},{'stat':state}]}
 else:
  parents=[]
  if operation in('install','update'):
   for path in _MAINT_PARENTS:
    value=tree.scan(path);require(not value['exists']or(value.get('isdir')and value['uid']==0 and value['mode']in('0755','0555')))
    parents.append({'item':path,'stat':value})
  bootstrap=[]
  for name,path in _MAINT_BOOTSTRAP:
   value=tree.scan(path,request['bootstrap'][name]);require(operation=='install'or value['exists'])
   bootstrap.append({'item':{'source':name,'destination':path},'stat':value})
  result.update(softwareParents={'results':parents},bootstrap={'results':bootstrap})
 return result

def _maintenance_entrypoint():
 try:
  require(sys.platform=='linux' and os.getuid()==os.geteuid()==0)
  request=_maintenance_request(sys.stdin.buffer.read(8193));account=_maintenance_account()
  root=os.open('/',os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC)
  try:tree=_MaintenanceTree(root,account['uid'])
  finally:os.close(root)
  try:result=_maintenance_observe(request,tree,account)
  finally:tree.close()
  raw=json.dumps(result,sort_keys=True,separators=(',',':'),allow_nan=False)
  require(len(raw.encode('ascii'))<=32768);print(raw);return 0
 except Exception:
  sys.stderr.write('service_maintenance_metadata_unsupported\n');return 78


if __name__=='__main__':
 sys.exit(_maintenance_entrypoint() if sys.argv[1:]==['--maintenance-observe'] else entrypoint())
