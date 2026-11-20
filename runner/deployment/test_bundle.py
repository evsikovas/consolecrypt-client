import importlib.util,json,os,tempfile,unittest
from pathlib import Path
from unittest.mock import patch
spec=importlib.util.spec_from_file_location('service_bundle',Path(__file__).with_name('bundle.py'));bundle=importlib.util.module_from_spec(spec);spec.loader.exec_module(bundle)
class BundleTests(unittest.TestCase):
 def test_actual_elf_machine_bytes_select_only_amd64_or_arm64(self):
  with tempfile.TemporaryDirectory() as own:
   path=Path(own)/'node'
   for machine,arch in ((62,'x86_64'),(183,'aarch64')):
    header=bytearray(20);header[:6]=b'\x7fELF\x02\x01';header[18:20]=machine.to_bytes(2,'little');path.write_bytes(header)
    with patch.object(bundle.subprocess,'check_output',return_value='Name: GLIBC_2.34\n'):
     self.assertEqual(bundle.elf_identity(path),{'arch':arch,'minGlibc':'2.34'})
   header[18:20]=(40).to_bytes(2,'little');path.write_bytes(header)
   with patch.object(bundle.subprocess,'check_output') as external:
    self.assertRaises(ValueError,bundle.elf_identity,path);external.assert_not_called()
 def test_arm_metadata_is_distinct_and_unknown_architecture_stays_denied(self):
  with tempfile.TemporaryDirectory() as own:
   value,_=self.fixture(Path(own));value['target']['arch']='aarch64'
   self.assertEqual(bundle.read_metadata(json.dumps(value).encode())['target']['arch'],'aarch64')
   value['target']['arch']='arm64';self.assertRaises(ValueError,bundle.read_metadata,json.dumps(value).encode())
 def fixture(self,root):
  for name in bundle.FIXED:(root/name).write_bytes(('owned fixture '+name).encode())
  value={'version':1,'kind':bundle.KIND,'nativeVersion':'0.1.0','sourceRevision':'1'*40,'cargoLockSha256':'2'*64,'runtimePinSha256':'3'*64,
   'target':{'system':'Linux','arch':'x86_64','libc':'glibc','minGlibc':'2.34','distribution':'ubuntu','distributionVersion':'24.04','minSystemd':255},
   'runtime':{'prefix':'/opt/cc-runtime','python':'3.12.3','environmentDigest':'4'*64,'helperDigest':'5'*64,'dependencyFilesDigest':'6'*64,'dependencyLock':'7'*64},
   'files':{name:{'sha256':bundle.sha(root/name),'bytes':(root/name).stat().st_size} for name in bundle.FIXED}}
  path=root/'bundle.json';path.write_text(json.dumps(value));return value,path
 def test_exact_owned_package_data_roundtrip_no_install_or_grant(self):
  with tempfile.TemporaryDirectory() as own:
   root=Path(own);value,path=self.fixture(root);self.assertEqual(bundle.verify(root,bundle.sha(path)),value)
 def test_future_and_unknown_metadata_preserved(self):
  with tempfile.TemporaryDirectory() as own:
   root=Path(own);value,path=self.fixture(root)
   for candidate in [dict(value,version=2),dict(value,approved=True),dict(value,version=True)]:
    path.write_text(json.dumps(candidate));before=path.read_bytes();self.assertRaises(ValueError,bundle.verify,root,bundle.sha(path));self.assertEqual(path.read_bytes(),before)
 def test_decoded_duplicate_rejected_before_destination_work(self):
  self.assertRaises(ValueError,bundle.read_metadata,b'{"version":1,"ver\\u0073ion":1}')
 def test_pointer_is_closed_versioned_routing_data_and_never_repaired(self):
  raw=b'{"version":1,"bundle":"'+b'a'*64+b'"}'
  self.assertEqual(bundle.read_pointer(raw),{'version':1,'bundle':'a'*64})
  for value in (raw.replace(b'1',b'2',1),raw.replace(b'1',b'true',1),raw[:-1]+b',"approved":true}',raw[:-1]+b',"ver\\u0073ion":1}',raw.replace(b'a'*64,b'../outside'),raw+b' '*4096):
   original=bytes(value);self.assertRaises(ValueError,bundle.read_pointer,value);self.assertEqual(value,original)
 def test_traversal_and_quota_rejected_without_writing(self):
  with tempfile.TemporaryDirectory() as own:
   root=Path(own);value,path=self.fixture(root);value['files']['../outside']={'bytes':1,'sha256':'1'*64};path.write_text(json.dumps(value));self.assertRaises(ValueError,bundle.verify,root,bundle.sha(path));self.assertFalse((root.parent/'outside').exists())
   del value['files']['../outside'];value['files']['cc-service-node']['bytes']=64*1024*1024+1;path.write_text(json.dumps(value));self.assertRaises(ValueError,bundle.verify,root,bundle.sha(path))
 def test_extra_entry_and_tamper_never_silently_drop_bytes(self):
  with tempfile.TemporaryDirectory() as own:
   root=Path(own);_,path=self.fixture(root);extra=root/'future';extra.write_bytes(b'preserve');self.assertRaises(ValueError,bundle.verify,root,bundle.sha(path));self.assertEqual(extra.read_bytes(),b'preserve');extra.unlink();(root/'cc-service-node').write_bytes(b'changed');self.assertRaises(ValueError,bundle.verify,root,bundle.sha(path))
 def test_actual_hardlink_is_rejected(self):
  with tempfile.TemporaryDirectory() as own:
   root=Path(own);_,path=self.fixture(root);os.link(root/'LICENSE',root/'linked');self.assertRaises(ValueError,bundle.verify,root,bundle.sha(path))
if __name__=='__main__':unittest.main(verbosity=2)
