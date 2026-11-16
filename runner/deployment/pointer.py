#!/opt/cc-runtime/bin/python
"""Fixed root-only software pointer transaction; metadata never grants jobs."""
import argparse,errno,fcntl,hashlib,json,os,re,runpy,stat,sys,uuid
from pathlib import Path
CONFIG=Path('/etc/consolecrypt-service');LIBRARY=Path('/opt/consolecrypt/deployment/bundle.py');BUNDLES=Path('/opt/consolecrypt/service-bundles')
class Denied(Exception):
 def __init__(self,code):self.code=code
def require(value,code='pointer_unsupported'):
 if not value:raise Denied(code)
def same(a,b):return a.st_dev==b.st_dev and a.st_ino!=0 and a.st_ino==b.st_ino
def directory():
 fd=os.open('/',os.O_RDONLY|os.O_DIRECTORY|os.O_CLOEXEC)
 try:
  for name in CONFIG.parts[1:]:
   child=os.open(name,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=fd);os.close(fd);fd=child
   info=os.fstat(fd);require(info.st_uid==0 and info.st_mode&0o022==0)
  require(stat.S_IMODE(os.fstat(fd).st_mode)==0o755);return fd
 except BaseException:os.close(fd);raise
def original(fd,name,expected,reader):
 try:opened=os.open(name,os.O_RDONLY|os.O_NONBLOCK|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=fd)
 except FileNotFoundError:require(expected=='absent','pointer_changed');return None
 with os.fdopen(opened,'rb') as stream:
  info=os.fstat(stream.fileno());require(stat.S_ISREG(info.st_mode) and info.st_uid==0 and info.st_nlink==1 and stat.S_IMODE(info.st_mode)==0o444 and info.st_size<=4096)
  raw=stream.read(4097);require(len(raw)<=4096);reader(raw)
  require(expected!='absent' and hashlib.sha256(raw).hexdigest()==expected,'pointer_changed')
  require(same(info,os.stat(name,dir_fd=fd,follow_symlinks=False)),'pointer_changed');return info
def select_pointer(identity,expected,bundle):
 require(os.getuid()==os.geteuid()==0)
 parsed=uuid.UUID(identity);require(parsed.int!=0 and str(parsed)==identity)
 require(expected=='absent' or re.fullmatch('[0-9a-f]{64}',expected));require(re.fullmatch('[0-9a-f]{64}',bundle))
 library=LIBRARY.lstat();require(stat.S_ISREG(library.st_mode) and library.st_uid==0 and library.st_nlink==1 and library.st_mode&0o222==0)
 verifier=runpy.run_path(str(LIBRARY));fd=directory();lock=None;temporary=None;temporary_identity=None;renamed=False
 try:
  parent=os.fstat(fd);name=identity+'.json';lock_name='.'+identity+'.pointer-lock'
  try:lock=os.open(lock_name,os.O_RDWR|os.O_NONBLOCK|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW|os.O_CLOEXEC,0o600,dir_fd=fd)
  except FileExistsError:lock=os.open(lock_name,os.O_RDWR|os.O_NONBLOCK|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=fd)
  info=os.fstat(lock);require(stat.S_ISREG(info.st_mode) and info.st_uid==0 and info.st_nlink==1 and stat.S_IMODE(info.st_mode)==0o600 and info.st_size==0)
  require(same(info,os.stat(lock_name,dir_fd=fd,follow_symlinks=False)))
  try:fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
  except BlockingIOError:raise Denied('pointer_busy') from None
  # Every maintained writer uses this SAME lock inode. Never unlink/adopt it.
  require(same(info,os.stat(lock_name,dir_fd=fd,follow_symlinks=False)))
  before=original(fd,name,expected,verifier['read_pointer'])
  verifier['verify_installed'](BUNDLES/bundle,bundle)
  raw=(json.dumps({'version':1,'bundle':bundle},separators=(',',':'))+'\n').encode();temporary='.'+identity+'-'+uuid.uuid4().hex+'.tmp'
  created=os.open(temporary,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW|os.O_CLOEXEC,0o600,dir_fd=fd)
  with os.fdopen(created,'wb') as stream:
   temporary_identity=os.fstat(stream.fileno());stream.write(raw);stream.flush();os.fchmod(stream.fileno(),0o444);os.fsync(stream.fileno())
  again=original(fd,name,expected,verifier['read_pointer']);require((before is None)==(again is None) and (before is None or same(before,again)),'pointer_changed')
  current=directory()
  try:require(same(parent,os.fstat(current)),'pointer_changed')
  finally:os.close(current)
  require(same(info,os.stat(lock_name,dir_fd=fd,follow_symlinks=False)))
  os.replace(temporary,name,src_dir_fd=fd,dst_dir_fd=fd);renamed=True
  # Failure after replace is uncertain maintenance, never a clean retry claim.
  os.fsync(fd)
  return {'version':1,'bundle':bundle,'pointerSha256':hashlib.sha256(raw).hexdigest(),'selected':True}
 except BaseException as error:
  if renamed:raise Denied('pointer_uncertain') from error
  raise
 finally:
  if temporary is not None and not renamed:
   try:
    value=os.stat(temporary,dir_fd=fd,follow_symlinks=False)
    if temporary_identity is not None and same(value,temporary_identity) and value.st_uid==0 and value.st_nlink==1:os.unlink(temporary,dir_fd=fd)
   except FileNotFoundError:pass
  if lock is not None:os.close(lock)
  os.close(fd)
def main():
 parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--identity',required=True);parser.add_argument('--expected-pointer-sha256',required=True);parser.add_argument('--bundle-sha256',required=True);args=parser.parse_args()
 print(json.dumps(select_pointer(args.identity,args.expected_pointer_sha256,args.bundle_sha256)))
if __name__=='__main__':
 try:main()
 except Denied as error:sys.stderr.write(error.code+'\n');sys.exit(78)
 except Exception:sys.stderr.write('pointer_unsupported\n');sys.exit(78)
