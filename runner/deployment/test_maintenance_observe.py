"""Draft/UNRUN: same observer, actual isolated POSIX tree; no installed fixture."""
import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import stat
import sys
from types import SimpleNamespace
import tempfile
import unittest

spec=importlib.util.spec_from_file_location('readonly_maintenance_preflight',Path(__file__).with_name('preflight.py'))
m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
IDENTITY='8734336e-73ee-443a-8e8e-6e2ef5562b3c'

class RequestAndAccountTests(unittest.TestCase):
 def request(self):return {'version':1,'phase':'accountState','operation':'install','identity':IDENTITY,'bootstrap':{}}
 def test_closed_request_rejects_duplicate_keys_caller_paths_modes_and_oversize(self):
  good=self.request();self.assertEqual(m._maintenance_request(json.dumps(good).encode()),good)
  for key in ('root','paths','uid','command','repair','adopt'):
   bad=dict(good);bad[key]='/caller/replacement';self.assertRaises(ValueError,m._maintenance_request,json.dumps(bad).encode())
  self.assertRaises(ValueError,m._maintenance_request,b'{"version":1,"version":1}')
  self.assertRaises(ValueError,m._maintenance_request,b' '*8193)
  for key,value in [('phase','other'),('operation','status'),('identity','00000000-0000-0000-0000-000000000000')]:
   bad=dict(good);bad[key]=value;self.assertRaises(ValueError,m._maintenance_request,json.dumps(bad).encode())
 def test_existing_nonroot_primary_group_only_no_privilege_adoption(self):
  pw=SimpleNamespace(pw_name='consolecrypt-service',pw_uid=10001,pw_gid=10001)
  gr=SimpleNamespace(gr_name='consolecrypt-service',gr_gid=10001)
  self.assertEqual(m._maintenance_account_values(pw,gr,[10001])['uid'],10001)
  for groups in ([0],[10001,0],[10001,10002],[]):self.assertRaises(ValueError,m._maintenance_account_values,pw,gr,groups)
  self.assertRaises(ValueError,m._maintenance_account_values,SimpleNamespace(pw_name=pw.pw_name,pw_uid=0,pw_gid=10001),gr,[10001])
  self.assertRaises(ValueError,m._maintenance_account_values,pw,SimpleNamespace(gr_name=gr.gr_name,gr_gid=10002),[10001])
 def test_bootstrap_hash_and_original_per_file_and_total_quota_are_bounded(self):
  req=self.request();req['phase']='software'
  req['bootstrap']={name:{'sha256':'a'*64,'bytes':1}for name,_ in m._MAINT_BOOTSTRAP}
  self.assertEqual(m._maintenance_request(json.dumps(req).encode()),req)
  for key,value in [('bytes',64*1024*1024+1),('bytes',True),('sha256','A'*64)]:
   bad=copy.deepcopy(req);bad['bootstrap']['bundle.py'][key]=value
   self.assertRaises(ValueError,m._maintenance_request,json.dumps(bad).encode())
  bad=copy.deepcopy(req)
  for r in bad['bootstrap'].values():r['bytes']=32*1024*1024
  self.assertRaises(ValueError,m._maintenance_request,json.dumps(bad).encode())

@unittest.skipUnless(sys.platform=='linux' and hasattr(os,'geteuid') and os.geteuid()==0,'owned Linux root temp-tree gate required')
class PhysicalMetadataTests(unittest.TestCase):
 def setUp(self):
  self.temp=tempfile.TemporaryDirectory(prefix='cc-maintenance-metadata-');self.root=Path(self.temp.name)
  self.account={'uid':10001,'gid':10001,'namedGroupGid':10001,'groups':[10001]}
  for path in (*m._MAINT_PARENTS,'/etc/systemd/system','/var/lib/consolecrypt-service'):
   p=self.path(path);p.mkdir(parents=True,exist_ok=True)
   for parent in (p,*p.parents):
    if parent==self.root.parent:break
    parent.chmod(0o755)
  home=self.path('/var/lib/consolecrypt-service');os.chown(home,10001,10001);home.chmod(0o700)
  self.records={}
  for name,path in m._MAINT_BOOTSTRAP:
   p=self.path(path);body=('reviewed public '+name+'\n').encode();p.write_bytes(body);p.chmod(0o444)
   self.records[name]={'sha256':hashlib.sha256(body).hexdigest(),'bytes':len(body)}
  fd=os.open(self.root,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW)
  try:self.tree=m._MaintenanceTree(fd,10001)
  finally:os.close(fd)
 def tearDown(self):self.tree.close();self.temp.cleanup()
 def path(self,path):return self.root/path.lstrip('/')
 def request(self,phase,operation='install'):
  return m._maintenance_request(json.dumps({'version':1,'phase':phase,'operation':operation,'identity':IDENTITY,
   'bootstrap':self.records if phase=='software' else {}}).encode())
 def observe(self,phase='software',operation='install'):
  return m._maintenance_observe(self.request(phase,operation),self.tree,self.account)
 def snapshot(self):
  out={}
  for p in self.root.rglob('*'):
   s=p.lstat();out[str(p.relative_to(self.root))]=(s.st_mode,s.st_uid,s.st_gid,s.st_nlink,s.st_size,s.st_ino,
     hashlib.sha256(p.read_bytes()).hexdigest()if stat.S_ISREG(s.st_mode)else None)
  return out
 def test_same_valid_tree_is_readonly_and_returns_actual_modes_hashes_and_absence(self):
  before=self.snapshot();account=self.observe('accountState');software=self.observe()
  self.assertFalse(account['privateDirectories']['results'][1]['stat']['exists'])
  self.assertEqual(len(software['softwareParents']['results']),5);self.assertEqual(len(software['bootstrap']['results']),7)
  for row in software['bootstrap']['results']:
   actual=self.path(row['item']['destination']).lstat();self.assertEqual(row['stat']['inode'],actual.st_ino)
   self.assertEqual(row['stat']['checksum'],self.records[row['item']['source']]['sha256'])
  self.assertEqual(before,self.snapshot())
 def test_bootstrap_leaf_symlink_is_refused_without_reading_or_repair(self):
  p=self.path(m._MAINT_BOOTSTRAP[0][1]);p.unlink();p.symlink_to(self.path(m._MAINT_BOOTSTRAP[1][1]))
  self.assertRaises((ValueError,OSError),self.observe);self.assertTrue(p.is_symlink())
 def test_parent_symlink_is_refused_even_for_an_absent_leaf(self):
  p=self.path('/opt/consolecrypt/deployment');moved=p.with_name('original-deployment');p.rename(moved);p.symlink_to(moved,target_is_directory=True)
  self.assertRaises((ValueError,OSError),self.observe);self.assertTrue(p.is_symlink())
 def test_foreign_uid_private_home_or_bootstrap_is_never_adopted(self):
  home=self.path('/var/lib/consolecrypt-service');os.chown(home,10002,10002)
  self.assertRaises(ValueError,self.observe,'accountState');self.assertEqual(home.lstat().st_uid,10002)
  os.chown(home,10001,10001);p=self.path(m._MAINT_BOOTSTRAP[0][1]);os.chown(p,10002,10002)
  self.assertRaises(ValueError,self.observe);self.assertEqual(p.lstat().st_uid,10002)
 def test_checksum_change_and_hardlink_are_refused_without_repair(self):
  p=self.path(m._MAINT_BOOTSTRAP[0][1]);original=p.read_bytes();p.chmod(0o644);p.write_bytes(b'X'+original[1:]);p.chmod(0o444)
  self.assertRaises(ValueError,self.observe);self.assertEqual(p.read_bytes(),b'X'+original[1:])
  p.chmod(0o644);p.write_bytes(original);p.chmod(0o444);os.link(p,p.with_name('unowned-hardlink'))
  self.assertRaises(ValueError,self.observe);self.assertEqual(p.lstat().st_nlink,2)
 def test_unexpected_size_and_mode_are_refused_before_hash_read(self):
  p=self.path(m._MAINT_BOOTSTRAP[0][1]);p.chmod(0o644)
  with p.open('wb')as f:f.truncate(2*1024*1024)
  p.chmod(0o444);self.assertRaises(ValueError,self.observe)
  p.chmod(0o644);self.assertRaises(ValueError,self.observe)
 def test_install_cannot_adopt_existing_private_instance_and_update_cannot_use_absence(self):
  self.assertRaises(ValueError,self.observe,'accountState','update')
  state=self.path('/var/lib/consolecrypt-service/'+IDENTITY);state.mkdir(mode=0o700);os.chown(state,10001,10001)
  self.assertRaises(ValueError,self.observe,'accountState','install')
  result=self.observe('accountState','update');self.assertTrue(result['privateDirectories']['results'][1]['stat']['exists'])
 def test_refusal_closes_all_walk_fds_but_keeps_original_tree_fd(self):
  p=self.path(m._MAINT_BOOTSTRAP[0][1]);os.chown(p,10002,10002)
  before=set(os.listdir('/proc/self/fd'))
  for _ in range(4):self.assertRaises(ValueError,self.observe)
  self.assertEqual(before,set(os.listdir('/proc/self/fd')))
  self.assertTrue(stat.S_ISDIR(os.fstat(self.tree.fd).st_mode))

if __name__=='__main__':unittest.main(verbosity=2)
