#!/usr/bin/env python3
"""Final Linux service build/strict package observation; never installs/starts."""
import argparse,base64,hashlib,io,json,os,platform,re,shutil,stat,subprocess,sys,tomllib,zipfile
from pathlib import Path
KIND='consolecrypt-service-linux-v1'
FIXED={'cc-service-node','service_daemon.py','service_control.py','consolecrypt-service@.service','consolecrypt-service-retention@.service','consolecrypt-service-retention@.timer','LICENSE','dependency-licenses.json','bundle.py','pointer.py'}
def require(ok):
 if not ok:raise ValueError('unsupported or changed service bundle')
def sha(path):
 with path.open('rb') as source:return hashlib.file_digest(source,'sha256').hexdigest()
def unique(pairs):
 result={}
 for key,value in pairs:require(key not in result);result[key]=value
 return result
def relative(name):
 require(type(name)is str and 0<len(name)<=240 and name.isascii() and not name.startswith('/') and '\\' not in name)
 require(all(p not in ('','.','..') and re.fullmatch('[A-Za-z0-9_.@+-]+',p) for p in name.split('/')))
 require(name in FIXED or name.startswith('runner/consolecrypt_runner/') and Path(name).suffix in ('.py','.json') or name.startswith('notices/'))
 return name
def read_metadata(raw):
 require(type(raw)is bytes and len(raw)<=256*1024)
 value=json.loads(raw,object_pairs_hook=unique)
 require(type(value)is dict and set(value)=={'version','kind','nativeVersion','sourceRevision','cargoLockSha256','runtimePinSha256','target','runtime','files'})
 require(type(value['version'])is int and value['version']==1 and value['kind']==KIND)
 require(type(value['nativeVersion'])is str and re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+',value['nativeVersion']) is not None)
 require(type(value['sourceRevision'])is str and re.fullmatch('(?:[0-9a-f]{40}|[0-9a-f]{64})',value['sourceRevision']) is not None)
 require(all(type(value[k])is str and re.fullmatch('[0-9a-f]{64}',value[k]) for k in ('cargoLockSha256','runtimePinSha256')))
 target=value['target'];runtime=value['runtime']
 require(type(target)is dict and set(target)=={'system','arch','libc','minGlibc','distribution','distributionVersion','minSystemd'})
 require(target['system']=='Linux' and target['arch'] in ('x86_64','aarch64') and target['libc']=='glibc' and target['distribution']=='ubuntu' and target['distributionVersion']=='24.04')
 require(type(target['minGlibc'])is str and re.fullmatch(r'[0-9]{1,3}\.[0-9]{1,3}(?:\.[0-9]{1,3})?',target['minGlibc']) and type(target['minSystemd'])is int and target['minSystemd']==255)
 require(type(runtime)is dict and set(runtime)=={'prefix','python','environmentDigest','helperDigest','dependencyFilesDigest','dependencyLock'})
 require(runtime['prefix']=='/opt/cc-runtime' and type(runtime['python'])is str and re.fullmatch(r'3\.12\.[0-9]{1,3}',runtime['python']))
 require(all(type(runtime[k])is str and re.fullmatch('[0-9a-f]{64}',runtime[k]) for k in ('environmentDigest','helperDigest','dependencyFilesDigest','dependencyLock')))
 files=value['files'];require(type(files)is dict and 6<=len(files)<=1024 and FIXED<=files.keys())
 for name,record in files.items():
  relative(name);require(type(record)is dict and set(record)=={'sha256','bytes'})
  require(type(record['bytes'])is int and 0<=record['bytes']<=64*1024*1024 and type(record['sha256'])is str and re.fullmatch('[0-9a-f]{64}',record['sha256']))
 return value
def verify(folder,expected):
 require(re.fullmatch('[0-9a-f]{64}',expected) is not None)
 require(folder.is_dir() and not folder.is_symlink());manifest=folder/'bundle.json'
 require(manifest.is_file() and not manifest.is_symlink() and manifest.stat().st_size<=256*1024 and sha(manifest)==expected)
 value=read_metadata(manifest.read_bytes());files=value['files']
 found=set();total=0
 # Count unknown entries too, without an unbounded list/sorted enumeration.
 stack=[folder];entries=0
 while stack:
  with os.scandir(stack.pop()) as scan:
   for entry in scan:
    entries+=1;require(entries<=2048 and not entry.is_symlink())
    if entry.is_dir(follow_symlinks=False):stack.append(Path(entry.path));continue
    require(entry.is_file(follow_symlinks=False));info=Path(entry.path).lstat()
    require(stat.S_ISREG(info.st_mode) and info.st_nlink==1);name=Path(entry.path).relative_to(folder).as_posix()
    if name=='bundle.json':continue
    relative(name);require(name in files);record=files[name]
    require(type(record)is dict and set(record)=={'sha256','bytes'} and type(record['bytes'])is int and 0<=record['bytes']<=64*1024*1024)
    require(type(record['sha256'])is str and re.fullmatch('[0-9a-f]{64}',record['sha256']) is not None)
    total+=info.st_size;require(total<=96*1024*1024 and info.st_size==record['bytes'] and sha(Path(entry.path))==record['sha256']);found.add(name)
 require(found==files.keys());return value

def preflight_sources(folder,expected):
 # SAME original verified manifest; a bulk courier never grants execution or
 # changes the actual private-runtime/helper compatibility check.
 value=verify(folder,expected);records={n:r for n,r in value['files'].items()if n.startswith('runner/')}
 require(1<=len(records)<=128 and sum(r['bytes']for r in records.values())<=4*1024*1024)
 require('runner/consolecrypt_runner/__init__.py'in records and 'runner/consolecrypt_runner/probe.py'in records)
 output=io.BytesIO()
 with zipfile.ZipFile(output,'w',compression=zipfile.ZIP_STORED,allowZip64=False)as archive:
  for name,record in sorted(records.items()):
   relative(name);require(record['bytes']<=256*1024)
   path=folder/name;info=path.lstat();raw=path.read_bytes();after=path.lstat()
   identity=lambda st:(st.st_dev,st.st_ino,st.st_mode,st.st_nlink,st.st_size,st.st_mtime_ns,st.st_ctime_ns)
   require(stat.S_ISREG(info.st_mode)and info.st_nlink==1 and identity(info)==identity(after)
       and len(raw)==record['bytes']and hashlib.sha256(raw).hexdigest()==record['sha256'])
   entry=zipfile.ZipInfo(name.removeprefix('runner/'),(1980,1,1,0,0,0));entry.create_system=3
   entry.external_attr=(stat.S_IFREG|0o444)<<16
   archive.writestr(entry,raw)
 require(len(output.getvalue())<=5*1024*1024)
 # A mutation while packaging cannot become the asserted original source.
 require(verify(folder,expected)==value)
 raw=output.getvalue()
 return{'version':1,'manifestSha256':expected,'files':{n.removeprefix('runner/'):r for n,r in sorted(records.items())},
        'archiveSha256':hashlib.sha256(raw).hexdigest(),'archiveBase64':base64.b64encode(raw).decode('ascii')}

COURIER_MAX=100*1024*1024
def write_courier(folder,expected,destination):
 value=verify(folder,expected);records=dict(value['files'])
 manifest=(folder/'bundle.json').read_bytes()
 require(len(manifest)<=256*1024 and hashlib.sha256(manifest).hexdigest()==expected)
 with zipfile.ZipFile(destination,'w',compression=zipfile.ZIP_STORED,allowZip64=False)as archive:
  for name in sorted(set(records)|{'bundle.json'}):
   if name!='bundle.json':relative(name)
   entry=zipfile.ZipInfo(name,(1980,1,1,0,0,0));entry.create_system=3;entry.external_attr=(stat.S_IFREG|0o444)<<16
   with archive.open(entry,'w')as output:
    if name=='bundle.json':output.write(manifest)
    else:
     fd=os.open(folder/name,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK|os.O_CLOEXEC)
     try:
      before=os.fstat(fd);require(stat.S_ISREG(before.st_mode)and before.st_nlink==1 and before.st_size==records[name]['bytes'])
      digest=hashlib.sha256();count=0
      while block:=os.read(fd,65536):
       count+=len(block);require(count<=records[name]['bytes']);digest.update(block);output.write(block)
      ident=lambda st:(st.st_dev,st.st_ino,st.st_mode,st.st_nlink,st.st_size,st.st_mtime_ns,st.st_ctime_ns)
      require(count==records[name]['bytes']and digest.hexdigest()==records[name]['sha256']
          and ident(before)==ident(os.fstat(fd))==ident((folder/name).lstat()))
     finally:os.close(fd)
   require(destination.tell()<=COURIER_MAX)
 require(destination.tell()<=COURIER_MAX and verify(folder,expected)==value)

def check_courier(archive,expected):
 require(re.fullmatch('[0-9a-f]{64}',expected)is not None)
 items=archive.infolist();require(1<=len(items)<=1025)
 names=set();manifest=None
 for item in items:
  name=item.filename;require(name not in names);names.add(name)
  require(not item.is_dir()and item.external_attr>>16==stat.S_IFREG|0o444
      and item.compress_type==zipfile.ZIP_STORED
      and not item.flag_bits&1 and item.file_size==item.compress_size and not item.extra)
  if name=='bundle.json':
   require(item.file_size<=256*1024);manifest=archive.read(item)
  else:relative(name)
 require(manifest is not None and hashlib.sha256(manifest).hexdigest()==expected)
 value=read_metadata(manifest);require(names==set(value['files'])|{'bundle.json'})
 total=0
 for item in items:
  if item.filename=='bundle.json':continue
  record=value['files'][item.filename];require(item.file_size==record['bytes'])
  total+=item.file_size;require(total<=96*1024*1024)
  digest=hashlib.sha256();count=0
  with archive.open(item)as stream:
   while block:=stream.read(65536):
    count+=len(block);require(count<=record['bytes']);digest.update(block)
  require(count==record['bytes']and digest.hexdigest()==record['sha256'])
 return value

def courier_directory(path,device,inode,root):
 require(sys.platform=='linux'and path.is_absolute()and path.parent==Path('/var/tmp'if root else'/tmp')
     and re.fullmatch('cc-service-bundle-[A-Za-z0-9_]{8,64}',path.name)is not None)
 fd=os.open(path,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC)
 try:
  st=os.fstat(fd);named=path.lstat()
  require((st.st_dev,st.st_ino)==(device,inode)==(named.st_dev,named.st_ino)
      and st.st_uid==os.geteuid()and stat.S_IMODE(st.st_mode)==0o700 and (not root or os.geteuid()==0))
  return fd
 except BaseException:os.close(fd);raise

def build_courier(directory,device,inode,expected):
 parent=courier_directory(directory,device,inode,False)
 try:
  fd=os.open('bundle.zip',os.O_RDWR|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW|os.O_CLOEXEC,0o600,dir_fd=parent)
  try:
   with os.fdopen(os.dup(fd),'w+b')as output:
    write_courier(Path(__file__).resolve().parent/'bundle',expected,output);output.flush()
   os.fsync(fd);st=os.fstat(fd);require(st.st_nlink==1 and stat.S_ISREG(st.st_mode)and st.st_size<=COURIER_MAX)
   os.lseek(fd,0,os.SEEK_SET);digest=hashlib.sha256();count=0
   while block:=os.read(fd,65536):
    count+=len(block);require(count<=st.st_size);digest.update(block)
   require((directory.lstat().st_dev,directory.lstat().st_ino)==(device,inode))
   return{'version':1,'archiveSha256':digest.hexdigest(),'archiveBytes':st.st_size}
  finally:os.close(fd)
 finally:os.close(parent)

def stage_courier(directory,device,inode,expected,archive_hash):
 parent=courier_directory(directory,device,inode,True)
 try:
  require(re.fullmatch('[0-9a-f]{64}',archive_hash)is not None)
  fd=os.open('bundle.zip',os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK|os.O_CLOEXEC,dir_fd=parent)
  try:
   before=os.fstat(fd);ident=lambda st:(st.st_dev,st.st_ino,st.st_mode,st.st_uid,st.st_nlink,st.st_size,st.st_mtime_ns,st.st_ctime_ns)
   require(stat.S_ISREG(before.st_mode)and before.st_uid==0 and before.st_nlink==1
       and stat.S_IMODE(before.st_mode)==0o400 and 0<before.st_size<=COURIER_MAX)
   digest=hashlib.sha256();count=0
   while block:=os.read(fd,65536):
    count+=len(block);require(count<=before.st_size);digest.update(block)
   require(count==before.st_size and digest.hexdigest()==archive_hash and ident(os.fstat(fd))==ident(before))
   os.lseek(fd,0,os.SEEK_SET)
   with os.fdopen(os.dup(fd),'rb')as stream,zipfile.ZipFile(stream)as archive:
    value=check_courier(archive,expected) # all hashes/members BEFORE destination writes
    target=Path('/opt/consolecrypt/service-bundles')/expected
    for p in target.parents:
     st=p.lstat();require(stat.S_ISDIR(st.st_mode)and st.st_uid==0 and not st.st_mode&0o022)
    require((directory.lstat().st_dev,directory.lstat().st_ino)==(device,inode)
        and ident(os.fstat(fd))==ident(before)==ident(os.stat('bundle.zip',dir_fd=parent,follow_symlinks=False)))
    target.mkdir(mode=0o700) # exclusive absent immutable version; never merge/adopt
    created={target}
    for item in archive.infolist():
     destination=target/item.filename;p=target
     for part in Path(item.filename).parts[:-1]:
      p=p/part
      if p not in created:p.mkdir(mode=0o700);created.add(p)
      st=p.lstat();require(stat.S_ISDIR(st.st_mode)and st.st_uid==0 and stat.S_IMODE(st.st_mode)==0o700)
     output=os.open(destination,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW|os.O_CLOEXEC,0o600)
     try:
      digest=hashlib.sha256();count=0
      with archive.open(item)as contents:
       while block:=contents.read(65536):
        count+=len(block);require(count<=item.file_size);digest.update(block);pending=memoryview(block)
        while pending:
         n=os.write(output,pending);require(n>0);pending=pending[n:]
      wanted=expected if item.filename=='bundle.json'else value['files'][item.filename]['sha256']
      require(count==item.file_size and digest.hexdigest()==wanted)
      os.fchmod(output,0o555 if item.filename=='cc-service-node'else 0o444);os.fsync(output)
     finally:os.close(output)
    for p in sorted(created,key=lambda p:len(p.parts),reverse=True):p.chmod(0o555)
    require(ident(os.fstat(fd))==ident(before)==ident(os.stat('bundle.zip',dir_fd=parent,follow_symlinks=False)))
    verify_installed(target,expected)
    return{'version':1,'bundle':expected,'verifiedImmutable':True}
  finally:os.close(fd)
 finally:os.close(parent)

def read_pointer(raw):
 require(type(raw)is bytes and len(raw)<=4096);value=json.loads(raw,object_pairs_hook=unique)
 require(type(value)is dict and set(value)=={'version','bundle'} and type(value['version'])is int and value['version']==1)
 require(type(value['bundle'])is str and re.fullmatch('[0-9a-f]{64}',value['bundle']))
 return value

def verify_installed(folder,expected):
 value=verify(folder,expected)
 # Root-owned immutable software only, never private identity/JobStore data.
 paths=[folder];count=0
 while paths:
  path=paths.pop();info=path.lstat();count+=1;require(count<=2048 and info.st_uid==0 and info.st_mode&0o222==0 and not path.is_symlink())
  if stat.S_ISDIR(info.st_mode):
   with os.scandir(path) as scan:paths.extend(Path(v.path) for v in scan)
  else:require(stat.S_ISREG(info.st_mode) and info.st_nlink==1)
 require((folder/'cc-service-node').lstat().st_mode & 0o111 == 0o111)
 return value

def elf_identity(path):
 with path.open('rb') as executable:header=executable.read(20)
 require(len(header)==20 and header[:6]==b'\x7fELF\x02\x01')
 machine=int.from_bytes(header[18:20],'little');require(machine in (62,183))
 versions=subprocess.check_output(['readelf','-W','--version-info',str(path)],text=True);require(len(versions)<=1024*1024)
 glibc=set(re.findall(r'Name: GLIBC_([0-9]+\.[0-9]+(?:\.[0-9]+)?)\b',versions));require(glibc)
 minimum=max(glibc,key=lambda v:tuple(map(int,v.split('.')))+(0,)*(3-len(v.split('.'))))
 return {'arch':{62:'x86_64',183:'aarch64'}[machine],'minGlibc':minimum}
def build(output):
 require(not any(os.environ.get(k) for k in ('RUSTFLAGS','CARGO_ENCODED_RUSTFLAGS','RUSTC_WRAPPER','RUSTC_WORKSPACE_WRAPPER')))
 machine=platform.machine()
 require(sys.platform=='linux' and machine in ('x86_64','aarch64') and platform.libc_ver()[0]=='glibc')
 release=platform.freedesktop_os_release();require(release.get('ID')=='ubuntu' and release.get('VERSION_ID')=='24.04')
 root=Path(__file__).resolve().parents[2]
 pin=json.loads((root/'client/rust/automation-core/src/vault_validation/runtime-pin-v1.json').read_bytes(),object_pairs_hook=unique)
 # A new architecture needs its own reviewed actual runtime pin. Never stamp
 # the existing AMD observation onto an ARM service build.
 require(pin['system']=='Linux' and pin['machine']==machine)
 require(not output.exists() and not output.is_symlink())
 # Existing release source/audit tooling remains the policy owner. Do not
 # substitute package metadata for a service consent, signature or runtime pin.
 subprocess.run([sys.executable,str(root/'client/scripts/dependency-audit.py'),'check'],cwd=root,check=True)
 require(not subprocess.check_output(['git','status','--porcelain','--untracked-files=normal'],cwd=root).strip())
 revision=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()
 output.mkdir(parents=True);source=output/'corresponding-source'
 subprocess.run([sys.executable,str(root/'client/scripts/export-source.py'),'--root',str(root),'--output',str(source)],cwd=root,check=True)
 # Existing Cargo vendor handles locked registry/Git sources; no hand-copied
 # cache, repository history, credentials or new dependency resolver.
 vendor=subprocess.check_output(['cargo','vendor','--locked','--versioned-dirs','--manifest-path',str(root/'client/rust/Cargo.toml'),'vendor'],cwd=source,text=True)
 configuration=source/'.cargo';configuration.mkdir(exist_ok=True)
 require(not (configuration/'config.toml').exists());(configuration/'config.toml').write_text(vendor)
 subprocess.run(['cargo','build','--locked','--release','--manifest-path',str(root/'client/rust/Cargo.toml'),'-p','cc-automation-core','--bin','cc-service-node'],cwd=root,check=True)
 target=Path(os.environ.get('CARGO_TARGET_DIR',str(root/'client/rust/target')))
 if not target.is_absolute():target=root/target
 node=target/'release/cc-service-node';require(node.is_file() and node.stat().st_size<=64*1024*1024)
 actual_elf=elf_identity(node);require(actual_elf['arch']==machine);minimum=actual_elf['minGlibc']
 target_identity={'system':'Linux','arch':machine,'libc':'glibc','minGlibc':minimum,'distribution':'ubuntu','distributionVersion':'24.04','minSystemd':255}
 runtime_identity={'prefix':'/opt/cc-runtime','python':pin['python'],'environmentDigest':pin['environment_digest'],'helperDigest':pin['helper_digest'],'dependencyFilesDigest':pin['dependency_files_digest'],'dependencyLock':pin['dependency_lock']}
 install=output/'install';install.mkdir();bundle=install/'bundle';bundle.mkdir()
 for name in ('service_daemon.py','service_control.py','consolecrypt-service@.service','consolecrypt-service-retention@.service','consolecrypt-service-retention@.timer','bundle.py','pointer.py'):shutil.copy2(root/'runner/deployment'/name,bundle/name)
 shutil.copy2(node,bundle/'cc-service-node');shutil.copy2(root/'LICENSE',bundle/'LICENSE')
 metadata=json.loads(subprocess.check_output(['cargo','metadata','--locked','--format-version','1','--filter-platform',machine+'-unknown-linux-gnu','--manifest-path',str(root/'client/rust/Cargo.toml')],cwd=root))
 packages={p['id']:p for p in metadata['packages']};nodes={p['id']:p for p in metadata['resolve']['nodes']}
 roots=[p['id'] for p in packages.values() if p['name']=='cc-automation-core'];require(len(roots)==1)
 pending=roots[:];selected=set()
 while pending:
  name=pending.pop()
  if name in selected:continue
  selected.add(name);require(len(selected)<=512);pending.extend(nodes[name]['dependencies'])
 licenses=[]
 for name in sorted(selected):
  p=packages[name];require(p.get('license') or p.get('license_file'))
  package=Path(p['manifest_path']).resolve().parent;notices=[]
  candidates=[]
  if p.get('license_file'):candidates.append(Path(p['license_file']))
  for item in package.iterdir():
   if item.name.upper().startswith(('LICENSE','COPYING','NOTICE')):candidates.append(item)
  require(len(candidates)<=32)
  key=p['name']+'-'+p['version'];require(re.fullmatch('[A-Za-z0-9_.+-]{1,128}',key))
  for item in sorted(set(candidates)):
   if not item.is_absolute():item=package/item
   require(item.resolve().is_relative_to(package) and not item.is_symlink())
   if not item.is_file():continue
   require(item.stat().st_size<=256*1024 and re.fullmatch('[A-Za-z0-9_.+-]{1,96}',item.name))
   dest=bundle/'notices'/key/item.name;dest.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(item,dest)
   notices.append(dest.relative_to(bundle).as_posix())
  require(notices or package.is_relative_to(root))
  licenses.append({'name':p['name'],'version':p['version'],'license':p.get('license'),'noticeFiles':notices})
 (bundle/'dependency-licenses.json').write_text(json.dumps(licenses,indent=2)+'\n')
 for p in (root/'runner/src/consolecrypt_runner').rglob('*'):
  if p.is_file() and p.suffix in ('.py','.json'):
   require(not p.is_symlink());dest=bundle/'runner'/p.relative_to(root/'runner/src');dest.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(p,dest)
 for name in ('bundle.py','preflight.py','service.yml','README.md'):shutil.copy2(root/'runner/deployment'/name,install/name)
 files={p.relative_to(bundle).as_posix():{'sha256':sha(p),'bytes':p.stat().st_size} for p in bundle.rglob('*') if p.is_file()}
 data={'version':1,'kind':KIND,'nativeVersion':tomllib.loads((root/'client/rust/Cargo.toml').read_text())['workspace']['package']['version'],
  'sourceRevision':revision,'cargoLockSha256':sha(root/'client/rust/Cargo.lock'),'runtimePinSha256':sha(root/'client/rust/automation-core/src/vault_validation/runtime-pin-v1.json'),'target':target_identity,'runtime':runtime_identity,'files':files}
 manifest=bundle/'bundle.json';manifest.write_text(json.dumps(data,sort_keys=True,indent=2)+'\n');verify(bundle,sha(manifest))
 print(json.dumps({'version':1,'bundleManifestSha256':sha(manifest),'sourceRevision':revision,'nativeVersion':data['nativeVersion'],'sourceDirectory':'corresponding-source','installDirectory':'install'}))
def main():
 parser=argparse.ArgumentParser(description=__doc__);sub=parser.add_subparsers(dest='action',required=True)
 b=sub.add_parser('build');b.add_argument('--output',type=Path,required=True)
 v=sub.add_parser('verify');v.add_argument('--expected-manifest-sha256',required=True)
 v=sub.add_parser('preflight-sources');v.add_argument('--expected-manifest-sha256',required=True)
 for action in('build-courier','stage-courier'):
  v=sub.add_parser(action);v.add_argument('--directory',type=Path,required=True)
  v.add_argument('--directory-device',type=int,required=True);v.add_argument('--directory-inode',type=int,required=True)
  v.add_argument('--expected-manifest-sha256',required=True)
  if action=='stage-courier':v.add_argument('--archive-sha256',required=True)
 sub.add_parser('inspect-record')
 sub.add_parser('inspect-pointer')
 v=sub.add_parser('verify-installed');v.add_argument('--bundle-sha256',required=True)
 args=parser.parse_args()
 if args.action=='build':build(args.output.resolve())
 elif args.action=='preflight-sources':
  print(json.dumps(preflight_sources(Path(__file__).resolve().parent/'bundle',args.expected_manifest_sha256),sort_keys=True,separators=(',',':')))
 elif args.action=='build-courier':print(json.dumps(build_courier(args.directory,args.directory_device,args.directory_inode,args.expected_manifest_sha256)))
 elif args.action=='stage-courier':print(json.dumps(stage_courier(args.directory,args.directory_device,args.directory_inode,args.expected_manifest_sha256,args.archive_sha256)))
 elif args.action=='inspect-pointer':print(json.dumps(read_pointer(sys.stdin.buffer.read(4097))))
 elif args.action=='verify-installed':
  require(re.fullmatch('[0-9a-f]{64}',args.bundle_sha256));value=verify_installed(Path('/opt/consolecrypt/service-bundles')/args.bundle_sha256,args.bundle_sha256)
  print(json.dumps({'version':1,'nativeVersion':value['nativeVersion'],'sourceRevision':value['sourceRevision'],'bundle':args.bundle_sha256}))
 else:
  value=read_metadata(sys.stdin.buffer.read(256*1024+1)) if args.action=='inspect-record' else verify(Path(__file__).resolve().parent/'bundle',args.expected_manifest_sha256)
  print(json.dumps({'version':1,'kind':value['kind'],'nativeVersion':value['nativeVersion'],'sourceRevision':value['sourceRevision'],'fileCount':len(value['files'])}))
if __name__=='__main__':main()
