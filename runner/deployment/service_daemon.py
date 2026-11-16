#!/opt/cc-runtime/bin/python
"""Fixed nonroot listener custody, then exec SAME service Node; no RPC handler."""
import os,socket,stat,sys,uuid,json,runpy
from pathlib import Path
BASE=Path('/run/consolecrypt-service')
NODE='/opt/consolecrypt/bin/cc-service-node'
PYTHON='/opt/cc-runtime/bin/python'
HELPER='/opt/consolecrypt/runner'
STATE=Path('/var/lib/consolecrypt-service')
CONFIG=Path('/etc/consolecrypt-service')
BUNDLES=Path('/opt/consolecrypt/service-bundles')
LIBRARY=Path('/opt/consolecrypt/deployment/bundle.py')
def denied():raise ValueError('private service unavailable')
def identity(raw):
 try:value=uuid.UUID(raw)
 except (ValueError,AttributeError):denied()
 if value.int==0 or str(value)!=raw:denied()
 return raw
def parent(path):
 fd=os.open('/',os.O_RDONLY|os.O_DIRECTORY|os.O_CLOEXEC)
 try:
  for part in path.parts[1:]:
   if part in ('','..','.'):denied()
   child=os.open(part,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=fd)
   os.close(fd);fd=child
  return fd
 except BaseException:os.close(fd);raise
def same(left,right):return left.st_dev==right.st_dev and left.st_ino!=0 and left.st_ino==right.st_ino
def owned_listener(raw):
 raw=identity(raw);uid=os.geteuid()
 if uid==0 or os.getuid()!=uid or os.getgid()!=os.getegid():denied()
 path=BASE/raw;fd=parent(path);stream=None
 try:
  original=os.fstat(fd)
  if original.st_uid!=uid or stat.S_IMODE(original.st_mode)!=0o700:denied()
  # No unlink/adoption, including stale socket after a crash. systemd owns the
  # exact RuntimeDirectory lifecycle (Preserve=no), never a journal or UI path.
  with os.scandir(fd) as entries:
   if next(entries,None) is not None:denied()
  stream=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM|socket.SOCK_CLOEXEC)
  stream.bind(str(path/'control.sock'))
  named=os.stat('control.sock',dir_fd=fd,follow_symlinks=False)
  if not stat.S_ISSOCK(named.st_mode) or named.st_uid!=uid or named.st_nlink!=1:denied()
  os.chmod('control.sock',0o600,dir_fd=fd,follow_symlinks=False)
  stream.listen(4)
  current=parent(path)
  try:
   if not same(original,os.fstat(current)):denied()
  finally:os.close(current)
  final=os.stat('control.sock',dir_fd=fd,follow_symlinks=False)
  if not same(named,final) or final.st_uid!=uid or stat.S_IMODE(final.st_mode)!=0o600:denied()
  if stream.getsockname()!=str(path/'control.sock'):denied()
  result=stream;stream=None;return result
 finally:
  if stream is not None:stream.close()
  os.close(fd)
def software(raw):
 # Software routing metadata ONLY. The existing identity/policy/epoch/JobStore
 # checks after exec remain the sole service authorization and recovery gate.
 fd=parent(CONFIG)
 try:
  directory=os.fstat(fd)
  if directory.st_uid!=0 or stat.S_IMODE(directory.st_mode)!=0o755:denied()
  pointer=os.open(raw+'.json',os.O_RDONLY|os.O_NONBLOCK|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=fd)
  with os.fdopen(pointer,'rb') as stream:
   info=os.fstat(stream.fileno())
   if not stat.S_ISREG(info.st_mode) or info.st_uid!=0 or info.st_nlink!=1 or stat.S_IMODE(info.st_mode)!=0o444 or info.st_size>4096:denied()
   contents=stream.read(4097)
 finally:os.close(fd)
 library=LIBRARY;metadata=library.lstat()
 if not stat.S_ISREG(metadata.st_mode) or metadata.st_uid!=0 or metadata.st_nlink!=1 or metadata.st_mode&0o222:denied()
 verifier=runpy.run_path(str(library));selection=verifier['read_pointer'](contents)
 root=BUNDLES/selection['bundle']
 verifier['verify_installed'](root,selection['bundle'])
 return root
def main():
 if len(sys.argv)!=2:denied()
 raw=identity(sys.argv[1]);os.umask(0o077)
 if any(name in os.environ for name in ('LISTEN_PID','LISTEN_FDS','LISTEN_FDNAMES')):denied()
 selected=software(raw)
 listener=owned_listener(raw)
 # No child/manager/queue: the same PID/UID that bound the listener becomes Node.
 fd=listener.detach()
 if fd!=3:os.dup2(fd,3,inheritable=True);os.close(fd)
 else:os.set_inheritable(3,True)
 env=dict(os.environ,LISTEN_PID=str(os.getpid()),LISTEN_FDS='1',LISTEN_FDNAMES='control')
 root=STATE/raw
 node=str(selected/'cc-service-node');helper=str(selected/'runner')
 os.execve(node,[node,PYTHON,helper,str(root/'runtime'),'--identity-existing',str(root/'identities'),raw,'--private-ipc'],env)
if __name__=='__main__':
 try:main()
 except BaseException:sys.exit(65)
